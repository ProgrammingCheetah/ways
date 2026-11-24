//! Detail's epic views (F6, D8): a short look at the selected epic, and its reading.
//!
//! The short look comes from what the state carries: its title, its squares counted, its stage
//! and release, the selected ticket. Enter reads the epic: its stage, its body, and its release's
//! objectives (or scope) and design from the release folder, without their signature blocks,
//! scrollable with `j`/`k` and Space. The body and the documents are read on the tab's gatherer
//! thread ([`crate::app::chair::look`]), never in the draw loop; until they come back the page
//! says it is reading.
//!
//! `s` signs the epic through `app::epic::sign`, refused inside an instance, after the same 5 s
//! undo as a ruling (`ChairAction::Sign`); while it waits the epic shows as signing. Once every
//! epic of its version is signed, the page says `b` starts the build.

use ratatui::text::Line;

use super::Page;
use crate::app::chair::look::{EpicRead, ReleaseDocument};
use crate::app::chair::{ChairState, EpicView, SquareState};
use crate::cli::chair::panes::epics::{selected, selected_square};
use crate::cli::chair::panes::{Cursor, View};
use crate::domain::EpicStage;
use crate::domain::screen::Meaning;

/// The short look's title: `Epic #224`.
pub fn title(state: &ChairState, cursor: &Cursor) -> String {
    selected(state, cursor).map_or_else(
        || "Epics".to_owned(),
        |epic| format!("Epic {}", epic.number),
    )
}

/// The reading's title: `Epic #224: body, objectives, design`.
pub fn read_title(state: &ChairState, cursor: &Cursor) -> String {
    selected(state, cursor).map_or_else(
        || "Epics".to_owned(),
        |epic| format!("Epic {}: body, objectives, design", epic.number),
    )
}

/// The title, the squares counted, and the stage: what both views open with.
fn head(page: &mut Page<'_, '_>, state: &ChairState, epic: &EpicView) {
    let view = *page.view();
    page.text(Some(Meaning::Bold), &epic.title);
    let count = |states: &[SquareState]| {
        epic.squares
            .iter()
            .filter(|square| states.contains(&square.state))
            .count()
    };
    page.line(Line::from(vec![
        view.paint(
            Meaning::Done,
            format!("■ {} done", count(&[SquareState::Done])),
        ),
        View::plain("  "),
        view.paint(
            Meaning::Verb,
            format!("{} {}", view.spinner(), count(&[SquareState::Running])),
        ),
        View::plain("  "),
        view.paint(
            Meaning::Waiting,
            format!("? {}", count(&[SquareState::Question])),
        ),
        View::plain("  "),
        view.paint(
            Meaning::Failed,
            format!("✗ {}", count(&[SquareState::Failed])),
        ),
        View::plain("  "),
        view.paint(
            Meaning::Dim,
            format!("□ {}", count(&[SquareState::Ready, SquareState::Waiting])),
        ),
    ]));
    let release = epic
        .release
        .as_ref()
        .map_or_else(String::new, |release| format!(" · release {release}"));
    if view.looks.signs(&state.root, epic.number) {
        page.line(Line::from(vec![
            view.paint(Meaning::Waiting, "signing…"),
            view.paint(Meaning::Dim, format!("{release} · u takes it back")),
        ]));
    } else {
        page.text(
            Some(Meaning::Dim),
            &format!("{}{release}", epic.stage_text()),
        );
        if let Some(way) = epic.way_on() {
            page.text(Some(Meaning::Waiting), &way);
        }
    }
}

/// What signing says at the foot of both views: the button while it is unsigned, and once every
/// epic of its version is signed and no scheduler builds it, that `b` starts the build.
fn sign_foot(page: &mut Page<'_, '_>, state: &ChairState, epic: &EpicView) {
    let view = *page.view();
    let signing = view.looks.signs(&state.root, epic.number);
    if epic.stage == EpicStage::Unsigned && !signing {
        page.button("s", "sign epic");
        return;
    }
    let others = view
        .looks
        .signing
        .iter()
        .filter(|(root, _)| *root == state.root)
        .map(|(_, epic)| *epic)
        .collect();
    let building = state.now.watch.as_ref().is_some_and(|watch| watch.running);
    if let Some(version) = &epic.release
        && state.unsigned_of(Some(version), &others) == 0
        && !building
    {
        page.text(
            Some(Meaning::Waiting),
            &format!(
                "Every epic of {version} is signed{}: b starts the build.",
                if signing { " once this one is" } else { "" }
            ),
        );
    } else if !signing {
        page.text(Some(Meaning::Done), "✓ signed");
    }
}

/// A short look at the selected epic.
pub fn page(page: &mut Page<'_, '_>, state: &ChairState, cursor: &Cursor) {
    let Some(epic) = selected(state, cursor) else {
        page.text(Some(Meaning::Dim), "No epic is open.");
        return;
    };
    let view = *page.view();
    head(page, state, epic);
    page.blank();
    if let Some(square) = selected_square(state, cursor) {
        page.line(Line::from(vec![
            view.square(square.state),
            View::plain(" "),
            view.paint(Meaning::Noun, square.ticket.to_string()),
            View::plain(" "),
            view.paint(Meaning::Dim, square.state.word()),
        ]));
        page.text(None, &square.title);
        page.blank();
    }
    page.text(
        Some(Meaning::Dim),
        "Enter reads its body, objectives and design.",
    );
    page.blank();
    sign_foot(page, state, epic);
    page.text(Some(Meaning::Dim), "d graph · w why · l live · Esc back");
}

/// A release document under its heading.
fn document(
    page: &mut Page<'_, '_>,
    heading: &str,
    document: Option<&ReleaseDocument>,
    missing: &str,
) {
    match document {
        Some(document) => {
            page.text(
                Some(Meaning::Bold),
                &format!("{heading} · {}", document.name),
            );
            page.text(None, &document.text);
        }
        None => {
            page.text(Some(Meaning::Bold), heading);
            page.text(Some(Meaning::Dim), missing);
        }
    }
    page.blank();
}

/// The read epic's body and its release's documents.
fn read_body(page: &mut Page<'_, '_>, read: &EpicRead) {
    page.text(Some(Meaning::Bold), &format!("The epic · {}", read.epic));
    match &read.body {
        Ok(body) if body.trim().is_empty() => page.text(Some(Meaning::Dim), "Its body is empty."),
        Ok(body) => page.text(None, body.trim()),
        Err(why) => page.text(
            Some(Meaning::Failed),
            &format!("✗ its body could not be read: {why}"),
        ),
    }
    page.blank();
    match &read.release {
        Some(release) => {
            page.text(
                Some(Meaning::Dim),
                &format!("Release {} · {}", release.version, release.folder),
            );
            page.blank();
            document(
                page,
                "Objectives",
                release.objectives.as_ref(),
                "Its release folder holds no objectives.md or scope.md.",
            );
            document(
                page,
                "Design",
                release.design.as_ref(),
                "Its release folder holds no design.md.",
            );
        }
        None => {
            page.text(
                Some(Meaning::Dim),
                "Its body names no release, so there are no objectives or design to read.",
            );
            page.blank();
        }
    }
}

/// The selected epic's stage and body, and its release's objectives and design.
pub fn read_page(page: &mut Page<'_, '_>, state: &ChairState, cursor: &Cursor) {
    let Some(epic) = selected(state, cursor) else {
        page.text(Some(Meaning::Dim), "No epic is open.");
        return;
    };
    let view = *page.view();
    head(page, state, epic);
    sign_foot(page, state, epic);
    page.blank();
    match view.looks.epic(epic.number) {
        Some(read) => read_body(page, read),
        None => {
            page.line(Line::from(vec![
                view.paint(Meaning::Verb, view.spinner()),
                view.paint(
                    Meaning::Dim,
                    format!(" reading epic {} and its release's documents…", epic.number),
                ),
            ]));
            page.blank();
        }
    }
    page.text(
        Some(Meaning::Dim),
        "j/k Space scroll · d graph · w why · Esc back",
    );
}
