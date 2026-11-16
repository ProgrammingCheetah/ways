//! `zaf hub link`: how the Engineer's terminal gets the running hub's control link (docs/zaf-actions.md
//! §5, the Engineer's ruling on the door question of epic #1).
//!
//! The key lives in the hub's memory only ([`super::guard`]). The one way to it is a Unix socket in
//! the zaf home, [`SOCKET_FILE`], that the hub serves while it runs. A connection is the whole
//! request: the hub reads who is on the other end from the kernel ([`PeerCredentials`]), writes one
//! line, `link <url>` or `refused <why>`, and closes.
//!
//! The hub refuses a caller it cannot tell, one of another user, and one whose process ancestry holds
//! a live instance's run folder, in any project on the zaf home's list: the same judgment as
//! `zaf epic sign` ([`crate::app::epic::instance_above`]). The client asks the same question of
//! itself before it connects, so a refused run never reaches the socket. The socket file is the
//! user's alone (mode 0600). This is a guard against a model's mistake, as the modes are: a
//! process that reparents itself away from its instance is not seen.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::adapters::process::{self, PeerCredentials};
use crate::app::epic::instance_above;
use crate::app::projects::Projects;
use crate::app::spawn::runs;
use crate::composition::STATE_DIR;

/// The socket's file name in the zaf home.
pub const SOCKET_FILE: &str = "hub.sock";

/// How long either end waits on the other.
const WAIT: Duration = Duration::from_secs(5);

/// The most a reply may hold: a link is a URL and a key.
const MAX_REPLY: u64 = 4096;

/// The socket file's mode: its user only.
const SOCKET_MODE: u32 = 0o600;

/// The first word of a reply that carries the link.
const LINK_WORD: &str = "link";

/// The first word of a reply that refuses.
const REFUSED_WORD: &str = "refused";

/// Where the hub of the zaf home `home` listens for a request.
pub fn socket_path(home: &Path) -> PathBuf {
    home.join(SOCKET_FILE)
}

/// Whether a process runs inside an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// No live instance is above it.
    Outside,
    /// A live instance is above it, named as `<role> instance <n> on #<epic>`.
    Inside(String),
    /// It could not be read: the process is gone, or a run folder cannot be read. Never taken for
    /// outside.
    Unknown,
}

/// Judges a process by its pid.
pub type Judge = dyn Fn(u32) -> Standing + Send + Sync;

/// A judge that reads the process table and the run folders of every project on `home`'s list.
pub fn judge_over(home: PathBuf) -> Arc<Judge> {
    Arc::new(move |pid| {
        let ancestors = process::ancestors_of(pid);
        if ancestors.is_empty() {
            return Standing::Unknown;
        }
        for project in Projects::read(&home).iter().filter(|entry| !entry.missing) {
            let Ok(all) = runs::read_all(&project.folder.join(STATE_DIR)) else {
                return Standing::Unknown;
            };
            if let Some(instance) = instance_above(&ancestors, &all, &process::alive) {
                return Standing::Inside(instance);
            }
        }
        Standing::Outside
    })
}

/// What the hub answers a request with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The link that carries the key.
    Link(String),
    /// Why there is none for this caller.
    Refused(String),
}

impl Reply {
    fn render(&self) -> String {
        match self {
            Self::Link(link) => format!("{LINK_WORD} {link}\n"),
            Self::Refused(why) => format!("{REFUSED_WORD} {why}\n"),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        let line = text.lines().next()?;
        let (word, rest) = line.split_once(' ')?;
        match word {
            LINK_WORD => Some(Self::Link(rest.to_owned())),
            REFUSED_WORD => Some(Self::Refused(rest.to_owned())),
            _ => None,
        }
    }
}

/// What the hub answers `peer` with: the `link` it holds (none for a read-only hub), unless it
/// cannot tell who asks, `peer` is another user than `our_uid`, or `judge` says an instance is above.
pub fn answer(
    link: Option<&str>,
    peer: Option<PeerCredentials>,
    our_uid: u32,
    judge: &Judge,
) -> Reply {
    let Some(peer) = peer else {
        return Reply::Refused("the hub cannot tell who asks".to_owned());
    };
    if peer.uid != our_uid {
        return Reply::Refused("the hub serves its own user only".to_owned());
    }
    match judge(peer.pid) {
        Standing::Outside => {}
        Standing::Inside(instance) => return Reply::Refused(inside_text(&instance)),
        Standing::Unknown => {
            return Reply::Refused("the hub cannot tell whether an instance runs above".to_owned());
        }
    }
    link.map_or_else(
        || Reply::Refused("this hub is read-only: it serves no controls".to_owned()),
        |link| Reply::Link(link.to_owned()),
    )
}

/// The refusal for a caller inside an instance: the same words from the hub and from the client.
fn inside_text(instance: &str) -> String {
    format!("refused inside an instance ({instance}): the control key is the Engineer's")
}

/// The hub's end: a listening socket that answers each request through [`answer`].
#[derive(Debug)]
pub struct LinkSocket {
    path: PathBuf,
}

impl LinkSocket {
    /// Binds [`socket_path`] in `home` and serves `link` to callers `judge` clears, on a thread of
    /// its own until the process ends. A file left there by a hub that was killed is replaced: the
    /// caller holds the hub lock, so no other hub is serving.
    pub fn serve(home: &Path, link: Option<String>, judge: Arc<Judge>) -> io::Result<Self> {
        let path = socket_path(home);
        fs::create_dir_all(home)?;
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(SOCKET_MODE))?;
        let our_uid = process::effective_uid();
        thread::Builder::new()
            .name("zaf-hub-link".to_owned())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let reply =
                        answer(link.as_deref(), process::peer_of(&stream), our_uid, &*judge);
                    // A caller that left has no answer coming; the next one is served.
                    let _ = stream.set_write_timeout(Some(WAIT));
                    let _ = (&stream).write_all(reply.render().as_bytes());
                }
            })?;
        Ok(Self { path })
    }

    /// Removes the socket file, so a caller after the hub has ended finds none runs.
    pub fn release(&self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Why `zaf hub link` printed no link.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// An instance runs above this process, or it could not be told that none does.
    #[error("{0}")]
    Inside(String),
    /// No hub listens.
    #[error("no hub runs: start one with `zaf hub --port <n>`")]
    NoHub,
    /// The hub answered that it gives this caller none.
    #[error("{0}")]
    Refused(String),
    /// The socket could not be used, or the hub's answer was not one.
    #[error("cannot ask the hub: {0}")]
    Io(String),
}

/// The running hub's link, asked for from `home` by a process `judge` says is `me`. Refused
/// before it connects when an instance is above. No socket, or one nobody listens on, is
/// [`LinkError::NoHub`].
pub fn fetch(home: &Path, me: &Standing) -> Result<String, LinkError> {
    match me {
        Standing::Outside => {}
        Standing::Inside(instance) => {
            return Err(LinkError::Inside(inside_text(instance)));
        }
        Standing::Unknown => {
            return Err(LinkError::Inside(
                "cannot tell whether an instance runs above this process".to_owned(),
            ));
        }
    }
    let mut stream = match UnixStream::connect(socket_path(home)) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(LinkError::NoHub);
        }
        Err(error) => return Err(LinkError::Io(error.to_string())),
    };
    let io = |error: io::Error| LinkError::Io(error.to_string());
    stream.set_read_timeout(Some(WAIT)).map_err(io)?;
    let mut text = String::new();
    (&mut stream)
        .take(MAX_REPLY)
        .read_to_string(&mut text)
        .map_err(io)?;
    match Reply::parse(&text) {
        Some(Reply::Link(link)) => Ok(link),
        Some(Reply::Refused(why)) => Err(LinkError::Refused(why)),
        None => Err(LinkError::Io("the hub's answer was not one".to_owned())),
    }
}

#[cfg(test)]
mod tests;
