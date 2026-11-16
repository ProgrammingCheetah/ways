//! `zaf hub --port <n>`: the hub's frame as a web page on the LAN (docs/zaf-actions.md §5).
//!
//! The page's server is `app::hub`'s; this is the loop that feeds it. It looks at the reports
//! file's metadata, as the terminal does, and a new report is the only thing that reads the board
//! and publishes a frame. Between reports it reads neither the board nor the network, and it
//! publishes nothing: the page's clocks do not move.

use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use super::super::board;
use super::super::exit::{BACKEND, REFUSED};
use super::super::spawn_stop;
use super::tick::Subprocess;
use super::watch::Hub;
use super::{DEFAULT_WIDTH, POLL, Pace};
use crate::app::hub::controls::Controls;
use crate::app::hub::guard::Guard;
use crate::app::hub::link as hub_link;
use crate::app::hub::state::gather_machine;
use crate::app::hub::{
    BindError, Changes, Dashboard, HubError, Mark, Page, Snapshot, StateReader, Watched, draw,
    lan_url,
};
use crate::app::spawn::watch::WatchHandle;
use crate::ports::Holder;

/// Binds every interface on `port`, or says why not on stderr. A port in use is a refusal of the
/// state; any other failure is the machine's.
pub fn bind(port: u16, path: &str) -> Result<Dashboard, u8> {
    Dashboard::bind(port).map_err(|error| {
        eprintln!("zaf {path}: {error}");
        match error {
            BindError::InUse { .. } => REFUSED,
            BindError::Failed { .. } => BACKEND,
        }
    })
}

/// What `GET /api/state` reads: the machine state of the zaf home `home`, every listed project,
/// as of the request. No zaf home is a refusal in plain words.
pub fn machine_reader(home: Option<PathBuf>) -> Arc<StateReader> {
    Arc::new(move || {
        home.as_deref()
            .map(|home| gather_machine(home, SystemTime::now()))
            .ok_or_else(|| "this machine has no zaf home to read the projects from".to_owned())
    })
}

/// The controls the hub serves over the projects of the zaf home `home`: each is the verb's own
/// app function, run from the root of the project the request names. The watches it starts are
/// `watches`', which end when their owner does.
pub fn controls(home: Option<PathBuf>, watches: WatchHandle) -> Controls {
    let stop_home = home.clone();
    let answer_home = home.clone();
    let watch = Hub::new(watches, home.clone());
    Controls::new(home)
        .with_watch(Arc::new(watch))
        .with_stop(Arc::new(move |root, task| {
            spawn_stop::stop_in(root, stop_home.as_deref(), task)
        }))
        .with_tick(Arc::new(Subprocess::this()))
        .with_answer(Arc::new(move |root, question, ruling| {
            board::rule_in(root, answer_home.as_deref(), question, ruling)
        }))
}

/// What the page is served with besides its frames: where the LAN reaches it, the guard that
/// admits its controls and the controls it runs.
pub struct Serving {
    /// The address of this machine that the LAN reaches it by.
    pub address: IpAddr,
    /// The hub's key, and whether it serves controls.
    pub guard: Guard,
    /// What a control the guard admits runs.
    pub controls: Controls,
    /// Whether to print the link that carries the key on `out`, which only a terminal may be asked
    /// for; the plain URL is printed whatever this says.
    pub show_link: bool,
    /// The zaf home whose `hub.sock` this hub answers `zaf hub link` on; none leaves no socket.
    pub link_home: Option<PathBuf>,
}

/// Serves `dashboard` until `pace` says to stop: the first frame from `gather`, then one more for
/// each new report, with no project watched for changes. The page's LAN URL is printed on `out`
/// once the server is up, and under it, for a hub with controls and a `serving` that says so, the
/// link that carries the key; `state` answers each `GET /api/state`.
#[cfg(test)]
pub fn serve<G, W>(
    out: &mut W,
    dashboard: Dashboard,
    serving: Serving,
    state: Arc<StateReader>,
    gather: G,
    state_dir: &Path,
    pace: &mut dyn Pace,
) -> Result<(), String>
where
    G: FnMut(SystemTime) -> Result<Snapshot, HubError>,
    W: Write,
{
    serve_watching(
        out,
        dashboard,
        serving,
        state,
        gather,
        state_dir,
        pace,
        &mut Vec::new,
        &|_| false,
    )
}

/// [`serve`], and one `change` event on `/api/events` for each report that arrives, instance that
/// starts and instance that ends in the projects `listed` names (read again at every look, so a
/// project that joins is watched). The look reads their reports files and run folders against
/// `alive`; it reads no board and no network.
#[allow(clippy::too_many_arguments)]
pub fn serve_watching<G, W>(
    out: &mut W,
    dashboard: Dashboard,
    serving: Serving,
    state: Arc<StateReader>,
    mut gather: G,
    state_dir: &Path,
    pace: &mut dyn Pace,
    listed: &mut dyn FnMut() -> Vec<Watched>,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<(), String>
where
    G: FnMut(SystemTime) -> Result<Snapshot, HubError>,
    W: Write,
{
    let io = |error: std::io::Error| error.to_string();
    let width = pace.width();
    // The mark is taken before the first read of the board, so a report made during it is seen.
    let mark = Mark::read(state_dir);
    let mut changes = Changes::new();
    changes.poll(&listed(), alive);
    let first = gather(pace.now()).map_err(|error| error.to_string())?;
    let port = dashboard.port();
    let url = lan_url(serving.address, port);
    let link = serving.guard.link(&url);
    // The key goes out by one more way only: the socket `zaf hub link` asks, which answers a caller
    // outside any instance. A socket that cannot be made leaves the hub up, without that way.
    let socket = serving.link_home.as_deref().and_then(|home| {
        hub_link::LinkSocket::serve(home, link.clone(), hub_link::judge_over(home.to_owned()))
            .map_err(|error| eprintln!("zaf hub: cannot offer the link to `zaf hub link`: {error}"))
            .ok()
    });
    let page = dashboard.start_with(&draw(&first, width), state, serving.guard, serving.controls);
    writeln!(out, "{url}").map_err(io)?;
    if let Some(link) = link.filter(|_| serving.show_link) {
        writeln!(out, "controls: {link}").map_err(io)?;
    }
    out.flush().map_err(io)?;
    let outcome = watch(
        pace,
        &mut gather,
        state_dir,
        &page,
        width,
        mark,
        (&mut changes, listed, alive),
    );
    if let Some(socket) = socket {
        socket.release();
    }
    outcome
}

/// Reads the projects the hub watches.
type Listed<'a> = dyn FnMut() -> Vec<Watched> + 'a;

/// Tells whether a holder is still alive.
type Alive<'a> = dyn Fn(&Holder) -> bool + 'a;

/// Publishes a frame for each new report until `pace` says to stop.
fn watch<G>(
    pace: &mut dyn Pace,
    gather: &mut G,
    state_dir: &Path,
    page: &Page,
    width: usize,
    mut mark: Mark,
    (changes, listed, alive): (&mut Changes, &mut Listed<'_>, &Alive<'_>),
) -> Result<(), String>
where
    G: FnMut(SystemTime) -> Result<Snapshot, HubError>,
{
    while pace.wait() {
        let seen = Mark::read(state_dir);
        if seen != mark {
            // The mark is taken first: a board that fails is tried again at the next report, not
            // at every look, and the page keeps the last frame.
            mark = seen;
            if let Ok(snapshot) = gather(pace.now()) {
                page.publish(&draw(&snapshot, width));
            }
        }
        for change in changes.poll(&listed(), alive) {
            page.announce(&change);
        }
    }
    Ok(())
}

/// The dashboard's wait: no keys, no terminal; it runs until the process is stopped.
pub struct Forever;

impl Pace for Forever {
    fn now(&mut self) -> SystemTime {
        SystemTime::now()
    }

    fn width(&mut self) -> usize {
        DEFAULT_WIDTH
    }

    fn wait(&mut self) -> bool {
        std::thread::sleep(POLL);
        true
    }
}

#[cfg(test)]
mod tests;
