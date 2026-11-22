//! Where the chair's tabs, states and writes come from: the machine's projects, a gatherer thread
//! per project over the hub's own read (`gather_one`, under machine.rs's guard against hung
//! reads), the reads a key asks for on that same thread ([`look`]), and the app paths of the verbs
//! that write. Names no terminal crate: the runtime hands what comes from here to the
//! [`Chair`](super::model::Chair).

use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

use super::build::Control;
use super::build::machine::dropped;
use super::model::Tab;
use crate::adapters::process;
use crate::app::chair::gatherer::{Cadence, GatherEvent, Gatherer, floor_of};
use crate::app::chair::look::{self, Look, Looked};
use crate::app::chair::{
    BudgetView, ChairAction, ChairActions, ChairState, Fix, Outcome, Ruling, Signing, build,
};
use crate::app::epic::{self, EpicError, SignGuard, SignOutcome};
use crate::app::hub::Mark;
use crate::app::hub::controls::Failure;
use crate::app::hub::next::next_for_project;
use crate::app::hub::state::machine::{READ_BOUND, Reader, budget_of};
use crate::app::hub::state::{gather_one, gather_project};
use crate::app::projects::{ProjectEntry, Projects};
use crate::app::schedule::request;
use crate::cli::{BoardAction, Verb, board, hub};
use crate::composition::{Composition, STATE_DIR, ZAF_HOME_VAR, zaf_home};
use crate::domain::TicketId;
use crate::manifest::{Manifest, ProjectName};
use crate::ports::live_use;

/// What the runtime hears from the threads.
#[derive(Debug)]
pub enum Update {
    /// Tab `index`'s gatherer said this.
    Gathered(usize, GatherEvent<ChairState>),
    /// Tab `index`'s gatherer read what a key asked for.
    Looked(usize, Looked),
    /// This was written, with this outcome.
    Written(ChairAction, Outcome),
    /// This control ran: what it did, or why it did not.
    Controlled(Control, Result<String, String>),
}

/// The projects the chair shows and the one on screen first: the machine's projects, with
/// `project` among them (added at the end when the machine does not list it) and on screen.
pub fn entries(project: Option<&Path>, home: Option<&Path>) -> (Vec<ProjectEntry>, usize) {
    let mut entries = home.map(Projects::read).unwrap_or_default();
    let Some(project) = project else {
        return (entries, 0);
    };
    let same = |folder: &Path| {
        folder == project
            || std::fs::canonicalize(folder).ok() == std::fs::canonicalize(project).ok()
    };
    let at = entries.iter().position(|entry| same(&entry.folder));
    let at = at.unwrap_or_else(|| {
        entries.push(ProjectEntry {
            folder: project.to_owned(),
            name: project.file_name().map_or_else(
                || project.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
            missing: false,
            version: None,
        });
        entries.len() - 1
    });
    (entries, at)
}

/// The tabs of `entries`, none read yet.
pub fn tabs(entries: &[ProjectEntry]) -> Vec<Tab> {
    entries
        .iter()
        .map(|entry| Tab::new(ProjectName::from(entry.name.as_str()), entry.folder.clone()))
        .collect()
}

/// Starts tab `index`'s gatherer over `entry` at `cadence`, telling `updates` each read. It reads
/// the project as the hub's page does and builds the chair's state on its own thread; a change to
/// the project's `.zaf/hub/reports` starts a read soon after.
pub fn gatherer(
    index: usize,
    entry: ProjectEntry,
    cadence: Cadence,
    updates: Sender<Update>,
) -> std::io::Result<Gatherer> {
    let reader: Arc<Reader> = Arc::new(|root: &Path, now: SystemTime| gather_project(root, now));
    let state_dir = entry.folder.join(STATE_DIR);
    let mut mark = Mark::read(&state_dir);
    let name = format!("chair-read-{}", entry.name);
    // A GitHub board is rationed: its project is read at most every 20 s, whatever asks. A
    // manifest that does not read leaves the cadence as it stands.
    let floor = Manifest::load_layers(&entry.folder, zaf_home().as_deref())
        .map_or(Duration::ZERO, |manifest| floor_of(manifest.board.kind));
    Gatherer::spawn(
        &name,
        cadence,
        floor,
        move |at| {
            let project = gather_one(&entry, at, READ_BOUND, &reader);
            let mut state = build(&project, &next_for_project(&project));
            state.now.budget = Some(budget());
            state
        },
        move || {
            let now = Mark::read(&state_dir);
            let changed = now != mark;
            mark = now;
            changed
        },
        move |event| updates.send(Update::Gathered(index, event)).is_ok(),
    )
}

/// The machine's budget and the units its live instances use, as `zaf status` reads them: the
/// process table and the zaf home's `[spawn] budget`. The web page has these on its machine line.
fn budget() -> BudgetView {
    let budget = zaf_home().map(|home| budget_of(&home)).unwrap_or_default();
    BudgetView {
        in_use: live_use(&process::displayed()),
        units: budget.units,
    }
}

/// Reads `look` in the project at `root` off the draw loop, telling `updates` what it read: an
/// epic on tab `index`'s gatherer thread, as every board read is (D3), so it waits for the read
/// under way; a ticket's reports and log, which are the state folder's files only, on a thread of
/// its own, so Live does not wait the length of a board read for them.
pub fn look(gatherer: &Gatherer, index: usize, root: PathBuf, look: Look, updates: Sender<Update>) {
    let job = move || {
        let looked = look::read(&root, look, &process::alive);
        let _ = updates.send(Update::Looked(index, looked));
    };
    match look {
        Look::Epic(_) => gatherer.run(Box::new(job)),
        Look::Ticket(_) => {
            let _ = std::thread::Builder::new()
                .name("zaf chair look".to_owned())
                .spawn(job);
        }
    }
}

/// The chair's writes, through the app paths of the verbs that make them.
#[derive(Debug, Clone)]
pub struct Writes {
    home: Option<PathBuf>,
}

impl Writes {
    /// The writes of the zaf home `home`.
    pub fn new(home: Option<PathBuf>) -> Self {
        Self { home }
    }
}

/// The command that runs `fix` as the binary `zaf`: its verb's own words (R3), in the project's
/// root, reading nothing. As `b` starts the scheduler, none of the variables of `names` that a
/// phase or an instance sets passes (every `ZAF_` one but `ZAF_HOME`); the zaf home `home`, when
/// the chair was given one, does.
fn fix_command(zaf: &Path, fix: &Fix, home: Option<&Path>, names: &[OsString]) -> Command {
    let mut command = Command::new(zaf);
    command
        .args(fix.args())
        .current_dir(&fix.root)
        .stdin(Stdio::null());
    for name in dropped(names.iter().map(OsString::as_os_str)) {
        command.env_remove(name);
    }
    if let Some(home) = home {
        command.env(ZAF_HOME_VAR, home);
    }
    command
}

/// How long a fix may run before it is killed with what it started.
pub const FIX_LIMIT: Duration = Duration::from_secs(60);

/// How often a running fix is looked at.
const FIX_LOOK: Duration = Duration::from_millis(50);

/// How a run with a time limit went.
#[derive(Debug)]
enum Bounded {
    /// It ended: how, and what it printed.
    Ended {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    /// It ran past the limit and was killed, with its process group.
    Killed,
}

/// Runs `command` in a process group of its own, its output read on threads of their own, for at
/// most `limit`: past it the whole group is killed, so a child it started cannot keep its pipes
/// open, and nothing waits for the readers any longer.
fn bounded(mut command: Command, limit: Duration) -> io::Result<Bounded> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().map(read_on_a_thread);
    let stderr = child.stderr.take().map(read_on_a_thread);
    let until = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= until {
            if let Ok(group) = libc::pid_t::try_from(child.id()) {
                // SAFETY: kill takes plain numbers; the group is the child's own, made for it by
                // `process_group(0)`, never zaf's.
                unsafe { libc::kill(-group, libc::SIGKILL) };
            }
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Bounded::Killed);
        }
        std::thread::sleep(FIX_LOOK);
    };
    // A grandchild may hold a pipe open after its parent ended: what came by then is kept.
    let read = |pipe: Option<Receiver<Vec<u8>>>| {
        pipe.and_then(|pipe| pipe.recv_timeout(Duration::from_secs(1)).ok())
            .unwrap_or_default()
    };
    Ok(Bounded::Ended {
        status,
        stdout: read(stdout),
        stderr: read(stderr),
    })
}

/// Reads `pipe` to its end on a thread of its own; what it read comes on the channel.
fn read_on_a_thread(mut pipe: impl io::Read + Send + 'static) -> Receiver<Vec<u8>> {
    let (said, read) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("chair-fix-read".to_owned())
        .spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            let _ = said.send(bytes);
        });
    read
}

/// The last line of `bytes` that holds words.
fn last_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

impl ChairActions for Writes {
    fn rule(&self, ruling: &Ruling) -> Result<Option<TicketId>, Failure> {
        let ready = board::rule_in(
            &ruling.root,
            self.home.as_deref(),
            ruling.question,
            ruling.answer.as_str(),
        )?;
        // As `zaf board ruling` does once it has written: the hub's report, and a tick asked of
        // the project's scheduler so the task the ruling frees starts at once.
        let verb = Verb::Board {
            action: BoardAction::Ruling {
                question: ruling.question,
                text: ruling.answer.to_string(),
                as_written: true,
            },
        };
        hub::leave_report_in(
            &ruling.root.join(STATE_DIR),
            &verb,
            0,
            None,
            SystemTime::now(),
        );
        Ok(ready)
    }

    fn fix(&self, fix: &Fix) -> Result<String, Failure> {
        let zaf = std::env::current_exe()
            .map_err(|error| Failure::Failed(format!("zaf's own path is unknown: {error}")))?;
        let names: Vec<OsString> = std::env::vars_os().map(|(name, _)| name).collect();
        let command = fix_command(&zaf, fix, self.home.as_deref(), &names);
        let ran = bounded(command, FIX_LIMIT)
            .map_err(|error| Failure::Failed(format!("it did not start: {error}")))?;
        let Bounded::Ended {
            status,
            stdout,
            stderr,
        } = ran
        else {
            return Err(Failure::Failed(format!(
                "it ran past {} s and was killed",
                FIX_LIMIT.as_secs()
            )));
        };
        if status.success() {
            let said = last_line(&stdout);
            return Ok(if said.is_empty() {
                last_line(&stderr)
            } else {
                said
            });
        }
        let why = last_line(&stderr);
        Err(Failure::Failed(if why.is_empty() {
            format!("it exited {}", status.code().unwrap_or(-1))
        } else {
            why
        }))
    }

    fn sign(&self, signing: &Signing) -> Result<bool, Failure> {
        let composition = Composition::discover_with_home(&signing.root, self.home.as_deref())
            .map_err(|error| Failure::Failed(error.to_string()))?;
        let board = composition
            .board()
            .map_err(|error| Failure::Failed(error.to_string()))?;
        let state_dir = composition.state_dir();
        // Only the Engineer signs: refused when an instance runs above the chair's process.
        let ancestors = process::ancestors();
        let guard = SignGuard {
            state_dir: &state_dir,
            ancestors: &ancestors,
            alive: &process::alive,
        };
        match epic::sign(board.as_ref(), signing.epic, &guard, SystemTime::now()) {
            Ok(SignOutcome::Signed(_)) => {
                // As `zaf epic sign` does: the running scheduler is asked for a tick, so the
                // epic is planned at once; one that cannot be asked leaves it to its timer.
                let who = format!("sign {}", signing.epic);
                let _ = request::ask(&state_dir, &who, SystemTime::now(), &process::alive);
                Ok(true)
            }
            Ok(SignOutcome::AlreadySigned(_)) => Ok(false),
            Err(error @ (EpicError::NotAnEpic(_) | EpicError::InsideInstance { .. })) => {
                Err(Failure::Refused(error.to_string()))
            }
            Err(error) => Err(Failure::Failed(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests;
