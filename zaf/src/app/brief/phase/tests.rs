use std::path::Path;
use std::time::SystemTime;

use super::*;
use crate::app::actions::Caller;
use crate::app::brief::{ActionsView, BriefPorts, BriefQuery, brief_for_phase, brief_with_actions};
use crate::domain::{Location, Ticket, TicketId, TicketState};
use crate::fakes::{FakeBoard, FakeForge, FakeVcs, PrRequest};
use crate::ports::{Check, CheckState, CommitId, ReviewComment, SliceName};

const HUNKS: &str = "@@ -1 +1 @@\n-old header\n+new header";
const HANDOFF: &str = "Left off at the CSV header; the footer is next.";

/// A pull request on a slice with one changed file, one comment at `head` and one on an older
/// head, one failed check and a hand-off.
struct Scene {
    board: FakeBoard,
    forge: FakeForge,
    vcs: FakeVcs,
    pr: PrNumber,
    base: BaseName,
    workspace: &'static Path,
}

fn scene() -> Scene {
    let board = FakeBoard::default();
    board.add_ticket(Ticket {
        id: TicketId::from(12),
        epic: Some(TicketId::from(10)),
        title: "Export a reading list".into(),
        body: "A body off the template.".into(),
        state: Some(TicketState::Claimed),
        url: "".into(),
        kind: None,
    });
    let slice = SliceName::from("feature/12");
    let base = BaseName::from("feature/10");
    let workspace = Path::new("/work/12");
    let vcs = FakeVcs::default();
    vcs.open(&slice, &base);
    vcs.place(&slice, workspace).expect("place");
    vcs.edit(&slice, "src/export.rs", 1, 1, HUNKS)
        .expect("edit");
    let forge = FakeForge::default();
    let pr = forge
        .open_pr(&PrRequest {
            slice,
            base: base.clone(),
            title: "Export".to_owned(),
            body: String::new(),
        })
        .expect("pr");
    forge.set_head(pr, Some("new-head")).expect("head");
    forge
        .set_checks(
            pr,
            vec![
                Check {
                    name: "ci".into(),
                    state: CheckState::Failed,
                    finished_at: Some(SystemTime::UNIX_EPOCH),
                },
                Check {
                    name: "lint".into(),
                    state: CheckState::Passed,
                    finished_at: Some(SystemTime::UNIX_EPOCH),
                },
            ],
        )
        .expect("checks");
    forge.set_handoff(pr, HANDOFF).expect("handoff");
    Scene {
        board,
        forge,
        vcs,
        pr,
        base,
        workspace,
    }
}

fn comment(body: &str, head: Option<&str>) -> ReviewComment {
    ReviewComment {
        author: "reviewer".into(),
        body: body.to_owned(),
        location: Some(Location {
            path: "src/export.rs".to_owned(),
            line: Some(7),
        }),
        head: head.map(CommitId::from),
        posted_at: SystemTime::UNIX_EPOCH,
    }
}

impl Scene {
    fn view(&self) -> PhaseView<'_> {
        PhaseView {
            pr: self.pr,
            workspace: Some(self.workspace),
            base: &self.base,
            forge: &self.forge,
            vcs: &self.vcs,
        }
    }

    /// The pack for the task, resumed when `resumed`, as written.
    fn pack(&self, resumed: bool) -> String {
        let state = tempfile::tempdir().expect("temp dir");
        let root = tempfile::tempdir().expect("temp dir");
        let caller =
            Caller::from_value(Some("programmer instance 1 on #10".as_ref())).expect("caller");
        let query = BriefQuery {
            task: TicketId::from(12),
            instance: "programmer-3",
            state_dir: state.path(),
            style: None,
        };
        let ports = BriefPorts {
            board: &self.board,
            design: None,
            nodes: None,
        };
        let view = ActionsView {
            project_root: root.path(),
            home: None,
            caller: &caller,
        };
        let phase = self.view();
        let written = if resumed {
            brief_for_phase(&query, &ports, view, Some(&phase))
        } else {
            brief_with_actions(&query, &ports, view)
        }
        .expect("brief");
        std::fs::read_to_string(written.path).expect("pack")
    }
}

fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let start = text
        .find(&format!("## {heading}\n"))
        .unwrap_or_else(|| panic!("no {heading} section in:\n{text}"));
    let rest = &text[start..];
    let end = rest[3..]
        .find("\n## ")
        .map_or(rest.len(), |offset| offset + 3);
    &rest[..end]
}

#[test]
fn the_pack_has_diff_findings_and_handoff() {
    let scene = scene();
    scene
        .forge
        .add_review_comment(
            scene.pr,
            comment("Name the header const.", Some("new-head")),
        )
        .expect("comment");

    let text = scene.pack(true);

    let diff = section(&text, "Diff");
    assert!(diff.contains("src/export.rs +1 -1"), "{diff}");
    assert!(diff.contains(HUNKS), "{diff}");
    let findings = section(&text, "Findings");
    assert!(findings.contains("At head new-head."), "{findings}");
    assert!(
        findings
            .contains("- review comment by reviewer on src/export.rs:7: Name the header const."),
        "{findings}"
    );
    assert!(findings.contains("- failed check: ci"), "{findings}");
    assert!(!findings.contains("lint"), "{findings}");
    let handoff = section(&text, "Hand-off");
    assert!(handoff.contains(HANDOFF), "{handoff}");
    // Today's parts stay, and the Interface page still ends the pack.
    assert!(text.contains("## Ticket\n"), "{text}");
    assert!(text.contains("## Style\n"), "{text}");
    assert!(text.contains("## Interface\n"), "{text}");
}

#[test]
fn old_findings_are_left_out() {
    let scene = scene();
    scene
        .forge
        .add_review_comment(scene.pr, comment("Stale remark.", Some("old-head")))
        .expect("comment");
    scene
        .forge
        .add_review_comment(scene.pr, comment("Remark with no head.", None))
        .expect("comment");
    scene
        .forge
        .add_review_comment(scene.pr, comment("Fresh remark.", Some("new-head")))
        .expect("comment");

    let text = scene.pack(true);

    assert!(!text.contains("Stale remark."), "{text}");
    assert!(!text.contains("Remark with no head."), "{text}");
    assert!(text.contains("Fresh remark."), "{text}");
}

#[test]
fn no_pr_gives_todays_pack() {
    let scene = scene();
    scene
        .forge
        .add_review_comment(
            scene.pr,
            comment("Name the header const.", Some("new-head")),
        )
        .expect("comment");

    // The cli passes no phase view for a task with no recorded pull request.
    let today = scene.pack(false);

    for heading in ["Diff", "Findings", "Hand-off"] {
        assert!(!today.contains(&format!("## {heading}\n")), "{today}");
    }
    assert!(!today.contains(HUNKS), "{today}");
    assert!(!today.contains(HANDOFF), "{today}");
    let headings: Vec<&str> = today
        .lines()
        .filter(|line| line.starts_with("## "))
        .collect();
    assert_eq!(
        headings,
        [
            "## Ticket",
            "## Rulings",
            "## Design pages",
            "## Owned paths",
            "## Style",
            "## Interface"
        ]
    );
}
