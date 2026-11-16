//! The hub's controls (docs/zaf-actions.md §5, epic #1 §4): what a keyed `POST /api/<control>`
//! does once the [`Guard`](super::guard::Guard) has admitted it.
//!
//! Each control is a route that takes a JSON body naming a `project` of the zaf home's list and
//! whatever the control needs, calls the app function the matching verb calls, from that project's
//! root, and answers what happened or the refusal with its reason. The controls change state, so
//! they reach no project the list does not hold, and no body is read before the guard admits.
//!
//! [`Controls`] is what the caller hands the server: the zaf home to resolve a project against,
//! and what runs each control. The server never names an adapter; a control with nothing behind
//! it is an unknown route. `POST /api/drop` needs only the home, so it is always built.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::spawn::stop::{StopError, Stopped};
use crate::domain::TicketId;

mod answer;
mod drop;
mod project;
mod stop;
mod tick;
mod watch;

pub use tick::{Ticked, Ticker, valid_version};

pub use answer::rule;

/// What `POST /api/watch` starts and stops watches with: `zaf spawn --watch` for a project at its
/// root, as the caller composes it, run inside the hub. A refusal of the state and a failure of the
/// machine are told apart.
pub trait Watcher: Send + Sync {
    /// Starts the watch of the project at `root` for `version`; a tick runs at once.
    fn start(&self, root: &Path, version: &str) -> Result<WatchStarted, Failure>;
    /// Ends the hub's own watch of the project at `root` after its current tick.
    fn stop(&self, root: &Path) -> Result<WatchStopped, Failure>;
}

pub use crate::app::spawn::watch::{Started as WatchStarted, Stopped as WatchStopped};

/// What `POST /api/answer` runs for a question from a project's root: `zaf board ruling`, as the
/// caller composes it, with the ruling's text. It returns the task the question held, now ready,
/// when there was one. A question that is not open is a refusal of the state.
pub type Ruler = dyn Fn(&Path, TicketId, &str) -> Result<Option<TicketId>, Failure> + Send + Sync;

/// What `POST /api/stop` runs for a task from a project's root: `zaf spawn stop`, as the caller
/// composes it. A refusal of the state and a failure of a backend are told apart.
pub type Stopper = dyn Fn(&Path, TicketId) -> Result<Stopped, Failure> + Send + Sync;

/// Why a control did not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The state refuses it, and nothing was changed: the exit `1` of the verb.
    Refused(String),
    /// A backend failed or is missing: the exit `2` of the verb.
    Failed(String),
}

impl From<StopError> for Failure {
    fn from(error: StopError) -> Self {
        match error {
            StopError::NoInstance(_) => Self::Refused(error.to_string()),
            error => Self::Failed(error.to_string()),
        }
    }
}

impl From<crate::app::spawn::watch::WatchError> for Failure {
    fn from(error: crate::app::spawn::watch::WatchError) -> Self {
        use crate::app::spawn::watch::WatchError;
        match error {
            WatchError::Refused(why) => Self::Refused(why),
            WatchError::Failed(why) => Self::Failed(why),
        }
    }
}

/// The answer to a control: an HTTP status line and the JSON body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The status, such as `200 OK`.
    pub status: &'static str,
    /// The JSON body: what happened, or `{"refused": <why>}`.
    pub body: String,
}

impl Answer {
    fn done(status: &'static str, body: &serde_json::Value) -> Self {
        Self {
            status,
            body: body.to_string(),
        }
    }

    /// The refusal of the JSON interface: an object with one field, `refused`, in plain words.
    pub fn refused(status: &'static str, why: &str) -> Self {
        Self::done(status, &serde_json::json!({ "refused": why }))
    }
}

/// What the controls run on: the zaf home that lists the projects, and one runner for each
/// control built.
#[derive(Clone, Default)]
pub struct Controls {
    home: Option<PathBuf>,
    stop: Option<Arc<Stopper>>,
    tick: Option<Arc<dyn Ticker>>,
    watch: Option<Arc<dyn Watcher>>,
    answer: Option<Arc<Ruler>>,
}

impl Controls {
    /// Controls over the projects of `home`, with none built yet.
    pub fn new(home: Option<PathBuf>) -> Self {
        Self {
            home,
            stop: None,
            tick: None,
            watch: None,
            answer: None,
        }
    }

    /// Builds `POST /api/watch` on `watch`, which starts and stops a project's watch.
    #[must_use]
    pub fn with_watch(mut self, watch: Arc<dyn Watcher>) -> Self {
        self.watch = Some(watch);
        self
    }

    /// Builds `POST /api/answer` on `answer`.
    #[must_use]
    pub fn with_answer(mut self, answer: Arc<Ruler>) -> Self {
        self.answer = Some(answer);
        self
    }

    /// Builds `POST /api/stop` on `stop`.
    #[must_use]
    pub fn with_stop(mut self, stop: Arc<Stopper>) -> Self {
        self.stop = Some(stop);
        self
    }

    /// Builds `POST /api/tick` on `tick`, which runs one tick of a project from its root.
    #[must_use]
    pub fn with_tick(mut self, tick: Arc<dyn Ticker>) -> Self {
        self.tick = Some(tick);
        self
    }

    /// Whether a control is built at `path`.
    pub fn has(&self, path: &str) -> bool {
        match path {
            "/api/drop" => true,
            "/api/stop" => self.stop.is_some(),
            "/api/tick" => self.tick.is_some(),
            "/api/watch" => self.watch.is_some(),
            "/api/answer" => self.answer.is_some(),
            _ => false,
        }
    }

    /// Runs the control at `path` on `body`, as the guard admitted it; none for a path that is not
    /// a control built here.
    pub fn run(&self, path: &str, body: &[u8]) -> Option<Answer> {
        match path {
            "/api/drop" => Some(drop::run(self.home.as_deref(), body)),
            "/api/stop" => self
                .stop
                .as_deref()
                .map(|stop| stop::run(self.home.as_deref(), stop, body)),
            "/api/tick" => self
                .tick
                .as_deref()
                .map(|tick| tick::run(self.home.as_deref(), tick, body)),
            "/api/watch" => self
                .watch
                .as_deref()
                .map(|watch| watch::run(self.home.as_deref(), watch, body)),
            "/api/answer" => self
                .answer
                .as_deref()
                .map(|answer| answer::run(self.home.as_deref(), answer, body)),
            _ => None,
        }
    }
}

impl std::fmt::Debug for Controls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Controls")
            .field("stop", &self.stop.is_some())
            .field("tick", &self.tick.is_some())
            .field("watch", &self.watch.is_some())
            .field("answer", &self.answer.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
