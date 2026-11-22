//! A project's state for the chair's tests: ways on 2026-11-19, shortened.

use std::path::PathBuf;
use std::time::Instant;

use super::model::{Chair, Tab};
use super::theme::Theme;
use crate::app::chair::gatherer::GatherEvent;
use crate::app::chair::{
    ChairState, EpicView, InboxItem, NowView, ProblemItem, QuestionView, ReleaseDue,
    ReleaseVersion, RunningLine, SlotsView, Square, SquareState, TroubleKind, WatchView,
};
use crate::app::hub::next::NextKind;
use crate::app::hub::state::{FixVerb, ProblemKind, TicketCost, WaitKind, WaitState};
use crate::domain::{EpicStage, TicketId};
use crate::manifest::ProjectName;

/// The root of the test project.
pub fn root() -> PathBuf {
    PathBuf::from("/srv/ways")
}

fn ask(
    number: u64,
    on: u64,
    blocks: u32,
    text: &str,
    proposal: Option<&str>,
    holds: &[u64],
) -> InboxItem {
    InboxItem {
        kind: NextKind::Question,
        title: format!("Question #{number}: {text}"),
        blocks,
        how: "Rule it.".to_owned(),
        command: Some(format!(
            "cd /srv/ways && zaf board ruling {number} '<ruling>'"
        )),
        link: None,
        number: Some(TicketId::from(number)),
        question: Some(QuestionView {
            text: text.to_owned(),
            raised_on: Some(TicketId::from(on)),
            door: false,
            proposal: proposal.map(str::to_owned),
            holds: holds.iter().copied().map(TicketId::from).collect(),
        }),
        begins: None,
    }
}

/// A square of `ticket` in `state`, titled after it, with nothing else known.
pub fn square(ticket: u64, state: SquareState) -> Square {
    Square {
        ticket: TicketId::from(ticket),
        title: format!("Ticket {ticket}"),
        state,
        phase: None,
        blocked_by: Vec::new(),
        question: None,
        waits: Vec::new(),
        phases_done: 0,
        phases_total: 0,
        pull_request: None,
        cost: None,
    }
}

/// What a square waits on, as the core names it.
pub fn wait(on: WaitKind, number: u64) -> WaitState {
    WaitState {
        on,
        number: Some(number),
        commit: None,
    }
}

/// The squares of epic #224: #230 orphaned; #233 and #240 under it, #240 under #233 too; #234
/// held by question #285; #235 done; #241 ready; #243 running its implement phase.
fn squares_224() -> Vec<Square> {
    let blocked = |ticket: u64, state, by: &[u64]| Square {
        blocked_by: by.iter().copied().map(TicketId::from).collect(),
        phases_total: 4,
        ..square(ticket, state)
    };
    vec![
        square(230, SquareState::Failed),
        Square {
            waits: vec![wait(WaitKind::Ticket, 230)],
            phase: Some("implement".to_owned()),
            ..blocked(233, SquareState::Waiting, &[230])
        },
        Square {
            waits: vec![wait(WaitKind::Question, 285)],
            question: Some(TicketId::from(285)),
            ..blocked(234, SquareState::Question, &[230])
        },
        Square {
            phases_done: 4,
            phases_total: 4,
            ..square(235, SquareState::Done)
        },
        Square {
            waits: vec![wait(WaitKind::Ticket, 233)],
            ..blocked(240, SquareState::Waiting, &[233, 230])
        },
        Square {
            phase: Some("implement".to_owned()),
            phases_total: 4,
            ..square(241, SquareState::Ready)
        },
        Square {
            phase: Some("implement".to_owned()),
            phases_total: 4,
            pull_request: Some((337, Some("https://forge.invalid/pulls/337".to_owned()))),
            cost: Some(TicketCost {
                tokens: 1_250_000,
                seconds: 725,
            }),
            ..square(243, SquareState::Running)
        },
    ]
}

/// The ways project: two questions and a due design in the inbox, an orphaned claim and stale
/// profiles, one instance running, two epics.
pub fn ways() -> ChairState {
    ChairState {
        project: ProjectName::from("ways"),
        root: root(),
        gathered_at: humantime::parse_rfc3339("2026-11-19T03:00:00Z").expect("time"),
        reason: None,
        inbox: vec![
            ask(
                285,
                228,
                2,
                "With no terminal, should zaf update change nothing after printing its plan?",
                Some("Change nothing: print the plan and exit."),
                &[234, 240],
            ),
            ask(
                278,
                235,
                1,
                "May this task add a tag listing to the Vcs port?",
                None,
                &[],
            ),
            InboxItem {
                kind: NextKind::Phase,
                title: "Release 0.5.3: its design is due".to_owned(),
                blocks: 0,
                how: "Run the phase.".to_owned(),
                command: Some("cd /srv/ways && zaf begin --version 0.5.3".to_owned()),
                link: None,
                number: None,
                question: None,
                begins: None,
            },
        ],
        problems: vec![
            ProblemItem {
                kind: TroubleKind::Project(ProblemKind::Profile),
                ticket: None,
                question: None,
                headline: "profiles lean and build are older than the manifest".to_owned(),
                detail: "profiles lean and build are older than the manifest".to_owned(),
                fix: Some("cd /srv/ways && zaf profile init".to_owned()),
                verb: Some(FixVerb::ProfileInit),
                holds: 0,
            },
            ProblemItem {
                kind: TroubleKind::Orphaned,
                ticket: Some(TicketId::from(230)),
                question: None,
                headline: "#230 orphaned".to_owned(),
                detail: "#230 is still claimed by team instance 3 on #224.".to_owned(),
                fix: None,
                verb: None,
                holds: 1,
            },
        ],
        now: NowView {
            running: vec![RunningLine {
                ticket: Some(TicketId::from(243)),
                title: Some("setup plan before writing".to_owned()),
                instance: "team instance 1 on #224".to_owned(),
                phase: Some("implement".to_owned()),
                run_seconds: Some(95),
                doing: Some("testing".to_owned()),
            }],
            ready: 1,
            waiting: 2,
            due: vec![ReleaseDue {
                version: ReleaseVersion::from("0.5.3"),
                phase: "design".to_owned(),
                command: "zaf begin --version 0.5.3".to_owned(),
            }],
            watch: Some(WatchView {
                running: true,
                pid: Some(4242),
                version: Some(ReleaseVersion::from("0.5.2")),
            }),
            slots: Some(SlotsView {
                total: 4,
                in_use: 1,
            }),
            guard: None,
            budget: None,
        },
        epics: vec![
            EpicView {
                number: TicketId::from(224),
                title: "the screens and setup".to_owned(),
                stage: EpicStage::Challenged,
                release: Some(ReleaseVersion::from("0.5.2")),
                done: 1,
                total: 7,
                question: None,
                squares: squares_224(),
            },
            EpicView {
                number: TicketId::from(226),
                title: "begin and the build start".to_owned(),
                stage: EpicStage::Planned,
                release: Some(ReleaseVersion::from("0.5.2")),
                done: 1,
                total: 2,
                question: None,
                squares: vec![
                    square(255, SquareState::Done),
                    square(257, SquareState::Ready),
                ],
            },
        ],
    }
}

/// The chair over ways and an empty dapple, ways read, in a terminal of `size`.
pub fn chair(size: (u16, u16)) -> Chair {
    let tabs = vec![
        Tab::new(ProjectName::from("ways"), root()),
        Tab::new(ProjectName::from("dapple"), PathBuf::from("/srv/dapple")),
    ];
    let mut chair = Chair::new(tabs, 0, Theme::new(true), size);
    chair.gathered(0, GatherEvent::Gathered(Box::new(ways())));
    chair
}

/// A moment to count from.
pub fn start() -> Instant {
    Instant::now()
}
