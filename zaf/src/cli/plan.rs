//! `zaf plan check <epic>`: the plan on stdin read as a plan of the epic, then its refusals or its
//! measures before and after the optimiser.
//!
//! A refused plan prints one line per refusal, `<kind>: <detail>` (a plan that does not read, one
//! per fault), and exits 1; `--json` gives `{"refused": [{"kind", "detail"}]}`. A good plan prints
//! two lines, `before:` and `after:`, each the plan's nodes, interfaces, chain, width and critical
//! path (`--more` adds the work and the narrow layers), and exits 0; `--json` gives
//! `{"before": …, "after": …}` with the measures' own keys. An epic that is no open epic, a design
//! that is not signed and a plan that does not read exit 1; a board or a version control that
//! cannot answer exits 2. Nothing is written to the board or the repository.
//!
//! `zaf plan file <epic>` files the plan on stdin as the epic's plan (see [`file`]): the record,
//! each ticket once, the filing mark, then one summary line. With empty stdin it finishes the
//! epic's last record that has no mark from the record (see [`finish`]), and says so when there
//! is nothing to finish, exit 0. A refused plan, or a run while the
//! epic's last record has no mark, prints one line per refusal, exits 1 and posts nothing; a board
//! that fails exits 2.
//!
//! `zaf plan show <epic>` prints the epic's graph as its tickets read now (see [`render`]): its
//! layers with their node ids, the chain, the width, the work and `critical <n>: A → C → E`. An
//! epic with no planned tickets, one whose waits go round and a ticket that is no open epic exit 1
//! with one line; a board that cannot answer exits 2. With `--json` it prints the same graph as one
//! document (see [`render_json`]): `epic`, `nodes`, `orders`, `kept`, `layers` and `measures`.
//! `--before` prints the graph as the epic's plan records wrote it, headed `as planned`; an epic
//! with no plan record and no planned ticket (filed before 0.6.0) prints its one view, headed
//! `planned before 0.6.0`, with or without it. A plan record that does not read exits 1.
//!
//! `zaf plan verify <epic>` holds the epic's graph to O3's structural checks (see [`verify`]): a
//! good graph prints nothing and exits 0; otherwise one line for each failure, exit 1. An epic
//! filed before 0.6.0 exits 1 with the one line "planned before 0.6.0: plan verify checks graph
//! epics".

use std::io::Read;

use serde_json::json;

use super::PlanAction;
use super::exit::{self, BACKEND, OK, REFUSED};
use super::{Cli, Composition};
use crate::app::plan::check::{self, CheckError};
use crate::app::plan::file::{self as filing, FileError};
use crate::app::plan::finish::{self, Finished};
use crate::app::plan::show::{self, PlanView, ShowError, ViewKind};
use crate::app::plan::verify::{self, VerifyError};
use crate::app::release::VocabularyLoadError;
use crate::domain::TicketId;
use crate::domain::plan::read::WaitTarget;
use crate::domain::plan::text::MAX_PLAN_BYTES;
use crate::domain::plan::{Measures, NodeId};

/// Runs the verb.
pub fn run(cli: &Cli, action: &PlanAction) -> u8 {
    match action {
        PlanAction::Check { epic } => check(cli, *epic),
        PlanAction::File { epic } => file(cli, *epic),
        PlanAction::Show { epic, before } => show(cli, *epic, *before),
        PlanAction::Verify { epic } => verify(cli, *epic),
    }
}

/// `zaf plan verify <epic>`: nothing and exit 0 for a good graph; one line for each failure on
/// stdout, exit 1; `--json` gives `{"failures": [{"rule", "detail"}]}`, empty for a good graph.
/// An epic filed before 0.6.0, one with no planned ticket and a ticket that is no open epic exit 1
/// with one line; a board or a version control that cannot answer exits 2.
fn verify(cli: &Cli, epic: TicketId) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let vcs = match composition.vcs() {
        Ok(vcs) => vcs,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let verified = verify::verify(
        composition.root(),
        composition.manifest(),
        board.as_ref(),
        vcs.as_ref(),
        epic,
    );
    match verified {
        Ok(failures) => {
            if cli.json {
                let failures: Vec<_> = failures
                    .iter()
                    .map(|failure| json!({ "rule": failure.rule().to_string(), "detail": failure.to_string() }))
                    .collect();
                println!("{}", json!({ "failures": failures }));
            } else {
                for failure in &failures {
                    println!("{failure}");
                }
            }
            if failures.is_empty() { OK } else { REFUSED }
        }
        Err(VerifyError::Board(error)) => exit::fail(&path, &error),
        Err(error @ VerifyError::Vocabulary(VocabularyLoadError::Vcs(_))) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            BACKEND
        }
        Err(error) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            REFUSED
        }
    }
}

/// The plan on stdin; one byte past the limit is read, so a plan over it is refused as too
/// large, never cut. A read that fails has said so and exits 2.
fn read_plan(path: &str) -> Result<Vec<u8>, u8> {
    let mut plan = Vec::new();
    let read = std::io::stdin()
        .take(MAX_PLAN_BYTES as u64 + 1)
        .read_to_end(&mut plan);
    match read {
        Ok(_) => Ok(plan),
        Err(error) => {
            crate::eprintln_zaf!("zaf {path}: reading the plan on stdin: {error}");
            Err(BACKEND)
        }
    }
}

/// `zaf plan file <epic>`: the summary line on success, exit 0; one line per refusal, exit 1;
/// `--json` gives the filing as one document, or `{"refused": [{"kind", "detail"}]}`.
fn file(cli: &Cli, epic: TicketId) -> u8 {
    let path = cli.verb.path();
    if let Some(code) = exit::refuse_challenger_filing(&path) {
        return code;
    }
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let vcs = match composition.vcs() {
        Ok(vcs) => vcs,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let plan = match read_plan(&path) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let outcome = if plan.iter().all(u8::is_ascii_whitespace) {
        finish::finish(
            composition.root(),
            composition.manifest(),
            board.as_ref(),
            vcs.as_ref(),
            epic,
        )
    } else {
        filing::file(
            composition.root(),
            composition.manifest(),
            board.as_ref(),
            vcs.as_ref(),
            epic,
            &plan,
        )
        .map(|filed| Finished::Filed(Box::new(filed)))
    };
    match outcome {
        Ok(Finished::Nothing) => {
            let said = Finished::Nothing.summary();
            if cli.json {
                println!(
                    "{}",
                    json!({ "epic": epic, "nothing": true, "summary": said })
                );
            } else {
                println!("{said}");
            }
            OK
        }
        Ok(Finished::Filed(filed)) => {
            if cli.json {
                let tickets: Vec<_> = filed
                    .filed
                    .iter()
                    .map(|(node, ticket)| json!({ "node": node, "ticket": ticket }))
                    .collect();
                println!(
                    "{}",
                    json!({
                        "epic": epic,
                        "record": filed.record,
                        "filed": tickets,
                        "skipped": filed.skipped,
                        "dropped": filed.dropped,
                        "summary": filed.summary(),
                    })
                );
            } else {
                println!("{}", filed.summary());
            }
            OK
        }
        Err(FileError::Check(CheckError::Board(error)) | FileError::Board(error)) => {
            exit::fail(&path, &error)
        }
        Err(FileError::Check(error @ CheckError::Vocabulary(VocabularyLoadError::Vcs(_)))) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            BACKEND
        }
        Err(error) => {
            let refused = file_refusals(&error);
            if cli.json {
                let refused: Vec<_> = refused
                    .iter()
                    .map(|(kind, detail)| json!({ "kind": kind, "detail": detail }))
                    .collect();
                println!("{}", json!({ "refused": refused }));
            } else {
                for (kind, detail) in &refused {
                    crate::eprintln_zaf!("zaf {path}: {kind}: {detail}");
                }
            }
            REFUSED
        }
    }
}

/// What refuses a filing, as `(kind, detail)`, one for each line the verb prints: the check's
/// own, a ticket a node cannot make, or the state's.
fn file_refusals(error: &FileError) -> Vec<(String, String)> {
    match error {
        FileError::Check(error) => refusals(error),
        FileError::Tickets(errors) => errors
            .iter()
            .map(|error| ("ticket".to_owned(), error.to_string()))
            .collect(),
        FileError::Unmarked { .. } => vec![("unmarked-record".to_owned(), error.to_string())],
        FileError::DesignChanged { .. } => vec![("design-changed".to_owned(), error.to_string())],
        FileError::TooLong { .. } => vec![("record-too-long".to_owned(), error.to_string())],
        FileError::NotItsTask { .. } => vec![("drop".to_owned(), error.to_string())],
        FileError::NoDesign(_) => vec![("design".to_owned(), error.to_string())],
        FileError::Board(_) => vec![("board".to_owned(), error.to_string())],
    }
}

fn show(cli: &Cli, epic: TicketId, before: bool) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let shown = if before {
        show::show_before(board.as_ref(), epic)
    } else {
        show::show(board.as_ref(), epic)
    };
    match shown {
        Ok(view) => {
            if cli.json {
                println!("{}", render_json(&view));
            } else {
                print!("{}", render(&view));
            }
            OK
        }
        Err(ShowError::Board(error)) => exit::fail(&path, &error),
        Err(error) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            REFUSED
        }
    }
}

/// The text of `plan show`: each layer with its node ids, then the chain, the width of each
/// layer, the work, and the critical path with its length.
pub fn render(view: &PlanView) -> String {
    let join = |ids: &[NodeId]| ids.iter().map(NodeId::as_str).collect::<Vec<_>>().join(" ");
    let measures = &view.measures;
    let mut out = String::new();
    match view.kind {
        ViewKind::After => {}
        ViewKind::Before => out.push_str("as planned\n"),
        ViewKind::Older => out.push_str("planned before 0.6.0\n"),
    }
    for (at, layer) in view.layers.iter().enumerate() {
        out.push_str(&format!("layer {}: {}\n", at + 1, join(layer)));
    }
    let width: Vec<String> = measures.width.iter().map(ToString::to_string).collect();
    out.push_str(&format!("chain {}\n", measures.chain));
    out.push_str(&format!("width {}\n", width.join(" ")));
    out.push_str(&format!("work {}\n", measures.work));
    let path: Vec<&str> = measures.critical_path.iter().map(NodeId::as_str).collect();
    out.push_str(&format!(
        "critical {}: {}\n",
        measures.critical,
        path.join(" → ")
    ));
    out
}

/// The JSON of `plan show`: one document with the epic, its `nodes`, its waits (`orders`, as the
/// fixtures' plans key them), its `kept` waits, its `layers`, and its `measures` (the fixtures'
/// `after` keys, the critical path among them). Each list is in the view's own order, so the
/// same epic prints the same bytes.
pub fn render_json(view: &PlanView) -> serde_json::Value {
    let nodes: Vec<_> = view
        .nodes
        .iter()
        .map(|node| {
            json!({
                "id": node.id,
                "ticket": node.ticket,
                "title": node.title,
                "kind": node.kind,
            })
        })
        .collect();
    let kept: Vec<_> = view
        .kept
        .iter()
        .map(|kept| {
            let on = match &kept.on {
                WaitTarget::Ticket { ticket, node, .. } => {
                    json!({ "ticket": ticket, "node": node })
                }
                WaitTarget::Epic(epic) => json!({ "epic": epic }),
            };
            json!({
                "to": kept.to,
                "name": kept.name,
                "line": kept.line.to_string(),
                "on": on,
            })
        })
        .collect();
    json!({
        "epic": view.epic,
        "nodes": nodes,
        "orders": view.orders,
        "kept": kept,
        "layers": view.layers,
        "measures": view.measures,
    })
}

fn check(cli: &Cli, epic: TicketId) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let vcs = match composition.vcs() {
        Ok(vcs) => vcs,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let plan = match read_plan(&path) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let checked = check::check(
        composition.root(),
        composition.manifest(),
        board.as_ref(),
        vcs.as_ref(),
        epic,
        &plan,
    );
    match checked {
        Ok(checked) => {
            if cli.json {
                println!("{}", json!(checked));
            } else {
                println!("before: {}", line(&checked.before, cli.more));
                println!("after: {}", line(&checked.after, cli.more));
            }
            OK
        }
        Err(CheckError::Board(error)) => exit::fail(&path, &error),
        Err(error @ CheckError::Vocabulary(VocabularyLoadError::Vcs(_))) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            BACKEND
        }
        Err(error) => {
            let refused = refusals(&error);
            if cli.json {
                let refused: Vec<_> = refused
                    .iter()
                    .map(|(kind, detail)| json!({ "kind": kind, "detail": detail }))
                    .collect();
                println!("{}", json!({ "refused": refused }));
            } else {
                for (kind, detail) in &refused {
                    crate::eprintln_zaf!("zaf {path}: {kind}: {detail}");
                }
            }
            REFUSED
        }
    }
}

/// What refuses the plan, as `(kind, detail)`, one for each line the verb prints.
fn refusals(error: &CheckError) -> Vec<(String, String)> {
    match error {
        CheckError::Refused(refusals) => refusals
            .iter()
            .map(|refusal| (refusal.code.to_string(), refusal.detail.clone()))
            .collect(),
        CheckError::Unreadable(faults) => faults
            .0
            .iter()
            .map(|fault| ("unreadable".to_owned(), fault.to_string()))
            .collect(),
        CheckError::OlderEpic(older) => older
            .0
            .iter()
            .map(|name| ("older-epic".to_owned(), name.to_string()))
            .collect(),
        CheckError::Drop(_) => vec![("drop".to_owned(), error.to_string())],
        CheckError::NodeFiled(_) => vec![("node-filed".to_owned(), error.to_string())],
        CheckError::NotAnEpic(_) => vec![("not-an-epic".to_owned(), error.to_string())],
        CheckError::Vocabulary(_) => vec![("vocabulary".to_owned(), error.to_string())],
        CheckError::Board(_) => vec![("board".to_owned(), error.to_string())],
    }
}

/// One plan's measures on a line.
fn line(measures: &Measures, more: bool) -> String {
    let path: Vec<&str> = measures.critical_path.iter().map(NodeId::as_str).collect();
    let width: Vec<String> = measures.width.iter().map(ToString::to_string).collect();
    let mut line = format!(
        "nodes {}, interfaces {}, chain {}, width {}, critical {} ({})",
        measures.nodes,
        measures.interfaces,
        measures.chain,
        width.join(" "),
        measures.critical,
        path.join(" "),
    );
    if more {
        let narrow: Vec<String> = measures
            .narrow_layers
            .iter()
            .map(ToString::to_string)
            .collect();
        line.push_str(&format!(
            ", work {}, narrow layers {}",
            measures.work,
            if narrow.is_empty() {
                "none".to_owned()
            } else {
                narrow.join(" ")
            }
        ));
    }
    line
}
