//! The captain's chair's read model (release 0.5.3, D2 and D3): what `zaf` alone shows for one
//! project, built from the read the hub's page is served from, and nothing that knows a terminal.
//!
//! [`ChairState`] is one project as the chair draws it: what waits on the Engineer, most blocking
//! first ([`InboxItem`], the hub's Next for you list for the project); what is wrong
//! ([`ProblemItem`]); what runs now and what is due ([`NowView`]); and each open epic with a
//! square per ticket ([`EpicView`]). [`build`] makes it from the hub's [`ProjectState`] and that
//! project's [`NextItem`]s, and every string a board, a file or a process gave is made
//! [`printable`](crate::domain::printable) on the way in, so nothing read can drive the terminal.
//!
//! [`gatherer::Gatherer`] reads a project again on a thread of its own and hands each state to
//! whoever draws, so the screen never waits on a read; what a key asks to read besides (an epic's
//! body and its release's documents, a ticket's reports and log) is a [`look::Look`], read on that
//! same thread. [`pending::PendingQueue`] holds what the chair is about to write for a few
//! seconds, so it can be taken back; [`ChairAction`] is what it holds (a ruling, a signature) and
//! [`commit`] writes one through [`ChairActions`].
//!
//! [`ProjectState`]: crate::app::hub::state::ProjectState
//! [`NextItem`]: crate::app::hub::next::NextItem

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::app::hub::controls::Failure;
use crate::app::hub::next::NextKind;
pub use crate::app::hub::state::FixVerb;
use crate::app::hub::state::{ProblemKind, TicketCost, WaitState};
use crate::domain::{EpicStage, TicketId, TypedRulingError, check_typed_ruling, printable};
use crate::manifest::ProjectName;
use crate::newtype::text_newtype;

mod build;
pub mod gatherer;
pub mod look;
pub mod pending;

pub use build::build;

text_newtype! {
    /// A release's version as the chair names it: `0.5.3`.
    ReleaseVersion
}

text_newtype! {
    /// The Engineer's ruling on a question, as typed into the answer box.
    RulingText
}

text_newtype! {
    /// A problem's fix as it reads: `cd /srv/ways && zaf profile init`.
    FixCommand
}

/// One project as the chair draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChairState {
    /// The project's name, from its manifest; its folder's name when the manifest cannot be read.
    pub project: ProjectName,
    /// The project's root.
    pub root: PathBuf,
    /// When the read began: the time everything on it is of.
    pub gathered_at: SystemTime,
    /// Why part of it could not be read, in plain words; none when everything was.
    pub reason: Option<String>,
    /// What waits on the Engineer, most blocking first.
    pub inbox: Vec<InboxItem>,
    /// What is wrong: what keeps the project from running first.
    pub problems: Vec<ProblemItem>,
    /// What runs now and what is due.
    pub now: NowView,
    /// The open epics, in issue order.
    pub epics: Vec<EpicView>,
}

impl ChairState {
    /// The question `question` in the inbox, when it is open.
    pub fn question(&self, question: TicketId) -> Option<&InboxItem> {
        self.inbox
            .iter()
            .find(|item| item.question.is_some() && item.number == Some(question))
    }

    /// The epic `epic`, when it is open.
    pub fn epic(&self, epic: TicketId) -> Option<&EpicView> {
        self.epics.iter().find(|view| view.number == epic)
    }

    /// The inbox items that wait on the Engineer: all but the offer to begin the next version,
    /// which waits on nobody.
    pub fn waiting(&self) -> usize {
        self.inbox
            .iter()
            .filter(|item| item.begins.is_none())
            .count()
    }

    /// The open epics of `version` that are not signed, `except` those left out.
    pub fn unsigned_of(
        &self,
        version: Option<&ReleaseVersion>,
        except: &BTreeSet<TicketId>,
    ) -> usize {
        self.epics
            .iter()
            .filter(|epic| epic.release.as_ref() == version)
            .filter(|epic| epic.stage == EpicStage::Unsigned && !except.contains(&epic.number))
            .count()
    }

    /// The questions in the inbox: what the tab bar counts beside the project's name.
    pub fn questions(&self) -> usize {
        self.inbox
            .iter()
            .filter(|item| item.question.is_some())
            .count()
    }

    /// The state with the questions of `hidden` taken out of the inbox: the ones whose ruling is
    /// waiting to be written, or was written since this was read.
    #[must_use]
    pub fn without(&self, hidden: &BTreeSet<TicketId>) -> Self {
        let mut shown = self.clone();
        shown.inbox.retain(|item| {
            item.question.is_none() || item.number.is_none_or(|number| !hidden.contains(&number))
        });
        shown
    }
}

/// One thing that waits on the Engineer: an item of the hub's Next for you list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItem {
    /// What it is.
    pub kind: NextKind,
    /// What it is, in a line: `Question #285: …`, `Sign epic #224: …`.
    pub title: String,
    /// The tasks it holds back.
    pub blocks: u32,
    /// How to do it, in a sentence or two.
    pub how: String,
    /// The command that does it by hand, run as written; none when no command does.
    pub command: Option<String>,
    /// Where a person opens it; none when the board gave no link.
    pub link: Option<String>,
    /// The issue it is about: the question to rule or the epic to sign.
    pub number: Option<TicketId>,
    /// The question in full, for a question, a door or a failed phase's question.
    pub question: Option<QuestionView>,
    /// The version it offers to begin, for the offer the inbox makes when no release is due
    /// (F16); none for everything else.
    pub begins: Option<ReleaseVersion>,
}

impl InboxItem {
    /// Whether the answer box can rule it: it is a question.
    pub fn answerable(&self) -> bool {
        self.question.is_some() && self.number.is_some()
    }
}

/// A question in full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionView {
    /// The whole question, without the `Raised on` line.
    pub text: String,
    /// The issue it was raised on, when its body says.
    pub raised_on: Option<TicketId>,
    /// Whether it was raised as a door, a one-way decision.
    pub door: bool,
    /// The answer it proposes, which the answer box starts with; none when it proposes none.
    pub proposal: Option<String>,
    /// The waiting tickets that name it among what they wait on, lowest first.
    pub holds: Vec<TicketId>,
}

impl QuestionView {
    /// Its first line that holds words.
    pub fn headline(&self) -> &str {
        self.text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_default()
    }
}

/// What kind of trouble a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TroubleKind {
    /// The project cannot run: a manifest, a token, a board, stale profiles, a folder gone.
    Project(ProblemKind),
    /// A claim held by an instance that has ended.
    Orphaned,
    /// A phase that got nowhere, waiting on its question.
    FailedPhase,
    /// A task the spawner skips or can never start.
    Task(TaskTrouble),
}

/// Why the spawner skips a task, or can never start it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskTrouble {
    /// The board lists it ready, but its body does not follow the task template.
    Unparsed,
    /// It is on a blocker cycle: it waits, through its blockers, on itself.
    Cycle,
    /// It names a blocker that never closes: not an issue, or closed as not planned.
    DeadBlocker,
}

/// One thing that is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemItem {
    /// What kind it is.
    pub kind: TroubleKind,
    /// The ticket it is about, when it is about one.
    pub ticket: Option<TicketId>,
    /// The question to rule, for a failed phase.
    pub question: Option<TicketId>,
    /// What is wrong, in one line.
    pub headline: String,
    /// What is wrong and what happens next, in plain words.
    pub detail: String,
    /// The command that fixes it, as a person runs it; none when no command does.
    pub fix: Option<String>,
    /// The zaf verb the chair runs to fix it, when one does (R3): never parsed from `fix`.
    pub verb: Option<FixVerb>,
    /// The waiting tickets it holds back.
    pub holds: usize,
}

/// What runs now and what is due.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NowView {
    /// The instances running now.
    pub running: Vec<RunningLine>,
    /// The tasks that may start, where the core places them when it does.
    pub ready: usize,
    /// The tasks that wait on a ticket or a question.
    pub waiting: usize,
    /// The releases whose objectives or design are due, oldest first.
    pub due: Vec<ReleaseDue>,
    /// Whether the project's scheduler runs; none when it could not be told.
    pub watch: Option<WatchView>,
    /// The slots and how many are in use; none when they could not be told.
    pub slots: Option<SlotsView>,
    /// The guard as `zaf guard` last wrote it; none when it never ran.
    pub guard: Option<GuardView>,
    /// The machine's budget and its use, read by the chair beside the project; none until read.
    pub budget: Option<BudgetView>,
}

/// The slots: `[spawn] slots` and how many are in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotsView {
    /// The manifest's `[spawn] slots`.
    pub total: u32,
    /// How many hold an instance or a live model run.
    pub in_use: u32,
}

/// The guard as `zaf guard` last wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardView {
    /// Its line as `zaf status` prints it: `green 12% ($30 of $250)`, `no budget set`.
    pub line: String,
    /// Whether it holds the spawner back: past its first threshold.
    pub tight: bool,
}

/// The machine's budget and the units its live instances use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetView {
    /// The units in use: every live instance's memory, summed.
    pub in_use: crate::domain::Units,
    /// The zaf home's `[spawn] budget`; none is no machine cap.
    pub units: Option<u32>,
}

/// An instance that runs now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningLine {
    /// The ticket it is named by: its task, else its epic; none when neither could be told.
    pub ticket: Option<TicketId>,
    /// That ticket's title, when the board holds it.
    pub title: Option<String>,
    /// The instance, as `<role> instance <n> on #<epic>`.
    pub instance: String,
    /// Its task's Team Phase, when it could be read.
    pub phase: Option<String>,
    /// Seconds since it started.
    pub run_seconds: Option<u64>,
    /// The word of its last reported tool call.
    pub doing: Option<String>,
}

/// A release whose objectives or design are due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseDue {
    /// The version.
    pub version: ReleaseVersion,
    /// The phase due: `objectives` or `design`.
    pub phase: String,
    /// The command that runs it by hand.
    pub command: String,
}

/// Whether the project's scheduler (`zaf spawn --watch`) runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchView {
    /// Whether it runs now.
    pub running: bool,
    /// The process its lock names, while it runs: what the chair's `B` sends SIGTERM to.
    pub pid: Option<u32>,
    /// The version it builds, while it runs.
    pub version: Option<ReleaseVersion>,
}

/// An open epic and a square for each of its tickets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicView {
    /// Its issue.
    pub number: TicketId,
    /// Its title, whole.
    pub title: String,
    /// Where it stands.
    pub stage: EpicStage,
    /// The version its body's `Release:` line names.
    pub release: Option<ReleaseVersion>,
    /// Its tickets done.
    pub done: u32,
    /// Its tickets that count, done ones included.
    pub total: u32,
    /// The first question open on the epic itself.
    pub question: Option<TicketId>,
    /// One square per ticket that counts, in id order.
    pub squares: Vec<Square>,
}

/// A ticket as its square shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Square {
    /// The ticket.
    pub ticket: TicketId,
    /// Its title.
    pub title: String,
    /// Its state.
    pub state: SquareState,
    /// The phase it is in, by name; none when done, not placed, or every phase has ended.
    pub phase: Option<String>,
    /// The tickets its `Blocked by:` line names.
    pub blocked_by: Vec<TicketId>,
    /// The first question open on it.
    pub question: Option<TicketId>,
    /// What the core says it waits on; empty when it does not wait or is not placed.
    pub waits: Vec<WaitState>,
    /// The phases of its type that have ended.
    pub phases_done: u32,
    /// All the phases of its type; `0` for a ticket the core does not place.
    pub phases_total: u32,
    /// Its pull request, once one is recorded, and where a person opens it.
    pub pull_request: Option<(u64, Option<String>)>,
    /// What its runs cost, as the ledger held them at the read; none when no run named it.
    pub cost: Option<TicketCost>,
}

/// The state a square shows: each has a symbol as well as a colour where it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SquareState {
    /// Done.
    Done,
    /// A phase run of it is live.
    Running,
    /// A question open on it waits on the Engineer.
    Question,
    /// It is claimed by an instance that has ended.
    Failed,
    /// Nothing holds it: it starts when a slot frees, or at the next tick.
    Ready,
    /// It waits on another ticket, its epic or a check.
    Waiting,
}

impl SquareState {
    /// The state in plain words.
    pub fn word(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Running => "running",
            Self::Question => "waits on you",
            Self::Failed => "orphaned",
            Self::Ready => "ready",
            Self::Waiting => "waiting",
        }
    }
}

/// What the chair writes: a ruling once its time to be taken back has passed, a fix at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChairAction {
    /// A ruling on a question.
    Rule(Ruling),
    /// A problem's fix: a zaf verb run in the project.
    Fix(Fix),
    /// The Engineer's signature on an epic, once its time to be taken back has passed.
    Sign(Signing),
}

impl ChairAction {
    /// The question it rules, for the inbox to hide while it waits.
    pub fn question(&self) -> Option<TicketId> {
        match self {
            Self::Rule(ruling) => Some(ruling.question),
            Self::Fix(_) | Self::Sign(_) => None,
        }
    }

    /// The epic it signs, for the chair to show as signing while it waits.
    pub fn signs(&self) -> Option<TicketId> {
        match self {
            Self::Sign(signing) => Some(signing.epic),
            Self::Rule(_) | Self::Fix(_) => None,
        }
    }

    /// The project it changes.
    pub fn root(&self) -> &Path {
        match self {
            Self::Rule(ruling) => &ruling.root,
            Self::Fix(fix) => &fix.root,
            Self::Sign(signing) => &signing.root,
        }
    }

    /// What it does, in a few words: `#285 ruled`.
    pub fn done_words(&self) -> String {
        match self {
            Self::Rule(ruling) => format!("{} ruled", ruling.question),
            Self::Fix(fix) => format!("zaf {} runs", fix.args().join(" ")),
            Self::Sign(Signing {
                epic,
                version: Some(version),
                last: true,
                ..
            }) => format!("epic {epic} signed, the last of {version}: then b starts the build"),
            Self::Sign(signing) => format!("epic {} signed", signing.epic),
        }
    }

    /// What taking it back did, in a few words: `#285 is back in your inbox`.
    pub fn undone_words(&self) -> String {
        match self {
            Self::Rule(ruling) => format!("{} is back in your inbox", ruling.question),
            Self::Fix(fix) => format!("zaf {} taken back", fix.args().join(" ")),
            Self::Sign(signing) => format!("epic {} not signed", signing.epic),
        }
    }
}

/// The Engineer's signature on an epic of a project, as `zaf epic sign` writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signing {
    /// The project's root.
    pub root: PathBuf,
    /// The epic.
    pub epic: TicketId,
    /// The version its `Release:` line names.
    pub version: Option<ReleaseVersion>,
    /// Whether it is the last unsigned epic of that version, so the build may start once it is
    /// signed.
    pub last: bool,
}

/// A problem's fix the chair runs itself (R3): a [`FixVerb`] the hub named, whose arguments are
/// fixed in zaf, run in the project's root. Nothing a board or a file wrote is ever run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// The project's root: the folder it runs in.
    pub root: PathBuf,
    /// The verb.
    pub verb: FixVerb,
    /// The fix as the problem shows it, for what the chair says.
    pub command: FixCommand,
}

impl Fix {
    /// `verb` for the project at `root`, said as `command`.
    pub fn new(root: PathBuf, verb: FixVerb, command: &str) -> Self {
        Self {
            root,
            verb,
            command: FixCommand::from(command),
        }
    }

    /// Its arguments after `zaf`.
    pub fn args(&self) -> &'static [&'static str] {
        self.verb.args()
    }
}

/// The Engineer's ruling on a question of a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ruling {
    /// The project's root.
    pub root: PathBuf,
    /// The question.
    pub question: TicketId,
    /// The ruling.
    pub answer: RulingText,
}

impl Ruling {
    /// A ruling on `question` of the project at `root`; one with no words is refused with the
    /// refusal `zaf board ruling` gives. The text is sent as written: it never passed a shell.
    pub fn new(root: PathBuf, question: TicketId, answer: &str) -> Result<Self, TypedRulingError> {
        check_typed_ruling(answer, true)?;
        Ok(Self {
            root,
            question,
            answer: RulingText::from(answer.trim()),
        })
    }
}

/// What writes the chair's actions: the app paths of the verbs that make them.
pub trait ChairActions: Send + Sync {
    /// Records `ruling` as `zaf board ruling` does: the question closes and the task it held is
    /// handed back, now ready, when there was one. A question that is no longer open is refused.
    fn rule(&self, ruling: &Ruling) -> Result<Option<TicketId>, Failure>;

    /// Runs `fix` as the Engineer would at a shell, with nothing to read from; what it printed
    /// last when it ran, or why it did not.
    fn fix(&self, fix: &Fix) -> Result<String, Failure>;

    /// Signs the epic as `zaf epic sign` does, refused inside an instance: true when it is signed
    /// now, false when its signature already held its body.
    fn sign(&self, signing: &Signing) -> Result<bool, Failure>;
}

/// How writing an action went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The question is ruled; `ready` is the task it held, now ready.
    Ruled {
        /// The question.
        question: TicketId,
        /// The task it held, now ready.
        ready: Option<TicketId>,
    },
    /// The board refused it and nothing changed.
    Refused {
        /// The question.
        question: TicketId,
        /// Why, in plain words.
        why: String,
    },
    /// A backend failed.
    Failed {
        /// The question.
        question: TicketId,
        /// Why, in plain words.
        why: String,
    },
    /// A fix ran and exited 0.
    Fixed {
        /// The fix.
        command: FixCommand,
        /// Its last line of output, when it printed one.
        said: String,
    },
    /// A fix did not run, or exited with an error.
    NotFixed {
        /// The fix.
        command: FixCommand,
        /// Why, in plain words.
        why: String,
    },
    /// The epic is signed, now or before.
    Signed {
        /// The epic.
        epic: TicketId,
        /// Whether its signature already held its body, so nothing was written.
        already: bool,
        /// The version whose last unsigned epic it was: its build may start.
        builds: Option<ReleaseVersion>,
    },
    /// The epic was not signed: refused inside an instance, not an open epic, or a backend.
    NotSigned {
        /// The epic.
        epic: TicketId,
        /// Why, in plain words.
        why: String,
    },
}

impl Outcome {
    /// `action` was not written, for `why`: a ruling failed, a fix not run, an epic not signed.
    pub fn failed(action: &ChairAction, why: String) -> Self {
        match action {
            ChairAction::Rule(ruling) => Self::Failed {
                question: ruling.question,
                why,
            },
            ChairAction::Fix(fix) => Self::NotFixed {
                command: fix.command.clone(),
                why,
            },
            ChairAction::Sign(signing) => Self::NotSigned {
                epic: signing.epic,
                why,
            },
        }
    }

    /// Whether the action was written.
    pub fn written(&self) -> bool {
        matches!(
            self,
            Self::Ruled { .. } | Self::Fixed { .. } | Self::Signed { .. }
        )
    }

    /// The question it was about, for a ruling.
    pub fn question(&self) -> Option<TicketId> {
        match self {
            Self::Ruled { question, .. }
            | Self::Refused { question, .. }
            | Self::Failed { question, .. } => Some(*question),
            Self::Fixed { .. }
            | Self::NotFixed { .. }
            | Self::Signed { .. }
            | Self::NotSigned { .. } => None,
        }
    }

    /// What happened, in one line: `#285 ruled; #273 ready`, or why it was not.
    pub fn line(&self) -> String {
        match self {
            Self::Ruled {
                question,
                ready: Some(task),
            } => format!("{question} ruled; {task} ready"),
            Self::Ruled {
                question,
                ready: None,
            } => format!("{question} ruled"),
            Self::Refused { question, why } | Self::Failed { question, why } => {
                format!("{question} was not ruled: {why}")
            }
            Self::Fixed { command, said } if said.is_empty() => format!("{command} ran"),
            Self::Fixed { command, said } => format!("{command} ran: {said}"),
            Self::NotFixed { command, why } => format!("{command} failed: {why}"),
            Self::Signed {
                epic,
                already: true,
                ..
            } => format!("epic {epic} was already signed"),
            Self::Signed {
                epic,
                builds: Some(version),
                ..
            } => {
                format!("epic {epic} signed; every epic of {version} is signed: b starts the build")
            }
            Self::Signed { epic, .. } => format!("epic {epic} signed; zaf plans its tickets"),
            Self::NotSigned { epic, why } => format!("epic {epic} was not signed: {why}"),
        }
    }
}

/// Writes `action` through `actions`, as [`commit`] does, and says a panic on the way as a failed
/// write: the thread that writes goes on with the next action.
pub fn commit_guarded(actions: &dyn ChairActions, action: &ChairAction) -> Outcome {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| commit(actions, action)))
        .unwrap_or_else(|_| {
            Outcome::failed(
                action,
                "zaf failed while writing it: the chair's log says why, the board what was \
                 written"
                    .to_owned(),
            )
        })
}

/// Writes `action` through `actions`.
pub fn commit(actions: &dyn ChairActions, action: &ChairAction) -> Outcome {
    match action {
        ChairAction::Rule(ruling) => match actions.rule(ruling) {
            Ok(ready) => Outcome::Ruled {
                question: ruling.question,
                ready,
            },
            Err(Failure::Refused(why)) => Outcome::Refused {
                question: ruling.question,
                why: printable(&why),
            },
            Err(Failure::Failed(why)) => Outcome::Failed {
                question: ruling.question,
                why: printable(&why),
            },
        },
        ChairAction::Fix(fix) => match actions.fix(fix) {
            Ok(said) => Outcome::Fixed {
                command: fix.command.clone(),
                said: printable(&said),
            },
            Err(Failure::Refused(why) | Failure::Failed(why)) => Outcome::NotFixed {
                command: fix.command.clone(),
                why: printable(&why),
            },
        },
        ChairAction::Sign(signing) => match actions.sign(signing) {
            Ok(signed) => Outcome::Signed {
                epic: signing.epic,
                already: !signed,
                builds: signing.version.clone().filter(|_| signing.last && signed),
            },
            Err(Failure::Refused(why) | Failure::Failed(why)) => Outcome::NotSigned {
                epic: signing.epic,
                why: printable(&why),
            },
        },
    }
}

#[cfg(test)]
mod tests;
