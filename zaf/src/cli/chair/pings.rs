//! A ping when something needs the Engineer (F10, D12).
//!
//! A new item in a project's inbox rings the terminal's bell and posts a desktop notification
//! with its first line; so does a version whose epics are all done. New is by id against the
//! project's previous read: an issue's number where it has one, the kind for the one "ready work
//! no scheduler ticks" item a project has (its title counts the tasks, and a count that moves is
//! no new item), the title for the rest. The first read of a project pings nothing, and neither
//! does a read that could not reach the board, nor the one after it, since a read without the
//! board holds no inbox to compare with.
//!
//! The desktop part runs `notify-send` on Linux or `osascript` on macOS, when it is on PATH, with
//! the text as arguments: AppleScript reads it from `on run argv`, never from inside the script.
//! `ZAF_NOTIFY=0` turns the desktop part off; no manifest key, since older binaries refuse unknown
//! keys. The text is already printable: the read model strips control characters.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::text::fit;
use crate::app::chair::{ChairState, InboxItem, ReleaseVersion};
use crate::app::hub::next::NextKind;
use crate::domain::{EpicStage, TicketId};

/// The variable that turns desktop notifications off when it is `0`.
pub const NOTIFY_VAR: &str = "ZAF_NOTIFY";

/// The most notifications one read posts; the bell rings once whatever their number.
pub const MOST: usize = 3;

/// The longest a notification's text runs, in cells.
const LONGEST: usize = 200;

/// A desktop notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Its title: `zaf · <project>`.
    pub title: String,
    /// Its text: the item's first line.
    pub body: String,
}

/// A ping to give.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ping {
    /// The terminal's bell.
    Bell,
    /// A desktop notification.
    Notify(Notice),
}

/// What tells two reads' items apart.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Id {
    /// An item about an issue: a question, a door, an epic to sign, a failed phase.
    Issue(TicketId),
    /// A kind a project lists once: ready work no scheduler ticks.
    Once(&'static str),
    /// Anything else, by its title.
    Titled(String),
}

fn id(item: &InboxItem) -> Id {
    match (item.number, item.kind) {
        (Some(number), _) => Id::Issue(number),
        (None, NextKind::Unticked) => Id::Once("unticked"),
        (None, _) => Id::Titled(item.title.clone()),
    }
}

/// An item's first line that holds words.
fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

/// Whether `version`'s open epics in `state` are all done; none when it has none.
fn finished(state: &ChairState, version: &ReleaseVersion) -> Option<bool> {
    let mut epics = state
        .epics
        .iter()
        .filter(|epic| epic.release.as_ref() == Some(version))
        .peekable();
    epics.peek()?;
    Some(
        epics.all(|epic| {
            epic.stage == EpicStage::Done || (epic.total > 0 && epic.done >= epic.total)
        }),
    )
}

/// The pings a read that turned `previous` into `next` calls for: one bell for anything new, a
/// notification for each new item (the first [`MOST`]) and for each version just finished.
pub fn pings(previous: Option<&ChairState>, next: &ChairState) -> Vec<Ping> {
    let Some(previous) = previous else {
        return Vec::new();
    };
    if previous.reason.is_some() || next.reason.is_some() {
        return Vec::new();
    }
    let title = format!("zaf · {}", next.project);
    let notice = |body: &str| Notice {
        title: title.clone(),
        body: fit(first_line(body), LONGEST),
    };
    let known: BTreeSet<Id> = previous.inbox.iter().map(id).collect();
    let mut notices: Vec<Notice> = next
        .inbox
        .iter()
        .filter(|item| !known.contains(&id(item)))
        // The offer to begin the next version waits on no one, so it rings nothing.
        .filter(|item| item.begins.is_none())
        .map(|item| notice(&item.title))
        .collect();
    let versions: BTreeSet<&ReleaseVersion> = previous
        .epics
        .iter()
        .filter_map(|epic| epic.release.as_ref())
        .collect();
    notices.extend(
        versions
            .into_iter()
            .filter(|version| {
                finished(previous, version) == Some(false) && finished(next, version) == Some(true)
            })
            .map(|version| notice(&format!("Release {version}: every epic is done"))),
    );
    if notices.is_empty() {
        return Vec::new();
    }
    std::iter::once(Ping::Bell)
        .chain(notices.into_iter().take(MOST).map(Ping::Notify))
        .collect()
}

/// What posts a desktop notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notifier {
    /// libnotify's `notify-send`, at this path.
    NotifySend(PathBuf),
    /// macOS's `osascript`, at this path.
    Osascript(PathBuf),
}

/// The AppleScript that shows a notification from its arguments: the title first, then the text.
/// The text never enters the script, so nothing in it is read as AppleScript.
pub const SCRIPT: [&str; 3] = [
    "on run argv",
    "display notification (item 2 of argv) with title (item 1 of argv)",
    "end run",
];

impl Notifier {
    /// The notifier of the system `os` (as `std::env::consts::OS` names it) on the PATH value
    /// `path`: `osascript` on macOS, `notify-send` elsewhere; none when it is not on PATH.
    pub fn find(os: &str, path: Option<&OsStr>) -> Option<Self> {
        let name = if os == "macos" {
            "osascript"
        } else {
            "notify-send"
        };
        let found = std::env::split_paths(path?)
            .map(|folder| folder.join(name))
            .find(|candidate| candidate.is_file())?;
        Some(if os == "macos" {
            Self::Osascript(found)
        } else {
            Self::NotifySend(found)
        })
    }

    /// The command line that posts `notice`: the program and its arguments, the text among them
    /// as one argument of its own, the title cut to [`TITLE_MOST`] characters and the text to
    /// [`BODY_MOST`]. notify-send reads its text as markup, so `&`, `<` and `>` go as entities.
    pub fn command_line(&self, notice: &Notice) -> (PathBuf, Vec<OsString>) {
        let title = capped(&notice.title, TITLE_MOST);
        let body = capped(&notice.body, BODY_MOST);
        match self {
            Self::NotifySend(program) => (
                program.clone(),
                vec![
                    "--app-name=zaf".into(),
                    "--".into(),
                    title.into(),
                    markup_escaped(&body).into(),
                ],
            ),
            Self::Osascript(program) => {
                let mut args: Vec<OsString> = SCRIPT
                    .iter()
                    .flat_map(|line| ["-e".into(), OsString::from(*line)])
                    .collect();
                // The title opens with `zaf`, so osascript reads no option past the script.
                args.push(title.into());
                args.push(body.into());
                (program.clone(), args)
            }
        }
    }
}

/// The most characters a notification's title keeps.
pub const TITLE_MOST: usize = 80;
/// The most characters a notification's text keeps.
pub const BODY_MOST: usize = 300;

/// `text` in at most `most` characters, ended with `…` when it was cut.
fn capped(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(most.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// `text` as notify-send's body markup reads it: `&`, `<` and `>` as entities, so nothing in it is
/// taken for markup (a link, an image, a tag that hides the rest).
fn markup_escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Whether desktop notifications are on, `value` being [`NOTIFY_VAR`]'s: anything but `0`.
pub fn desktop_on(value: Option<&OsStr>) -> bool {
    value.is_none_or(|value| value.to_str().map(str::trim) != Some("0"))
}

/// Where the chair posts its notifications: nowhere when they are off or no notifier is on PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desktop {
    notifier: Option<Notifier>,
}

impl Desktop {
    /// The desktop of this process's environment.
    pub fn from_environment() -> Self {
        let on = desktop_on(std::env::var_os(NOTIFY_VAR).as_deref());
        Self {
            notifier: on
                .then(|| Notifier::find(std::env::consts::OS, std::env::var_os("PATH").as_deref()))
                .flatten(),
        }
    }

    /// Posts `notice`, waiting for nothing: the notifier runs on, and a thread reaps it.
    pub fn post(&self, notice: &Notice) {
        let Some(notifier) = &self.notifier else {
            return;
        };
        let (program, args) = notifier.command_line(notice);
        let started = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = started {
            let _ = std::thread::Builder::new()
                .name("chair-notify".to_owned())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    }
}

#[cfg(test)]
mod tests;
