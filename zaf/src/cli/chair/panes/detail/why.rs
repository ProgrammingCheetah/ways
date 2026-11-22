//! Detail's why view (F7, D9): `w` on a ticket says in plain words what it waits on, its next
//! phase, its pull request, and its time and tokens as the ledger held them at the read.
//!
//! Everything comes from what the state carries: what the core says the ticket waits on (its
//! square's waits, from the same placement its colour comes from), its phase and how many of its
//! phases have ended, its pull request from the queue, its cost from the ledger. Nothing here asks
//! where a ticket sits. The words are the tick's own (`app::schedule::tick`'s waiting words): 0.5.2's
//! stage words are not on this branch. [`why`] is the pure part; [`page`] draws it.

use ratatui::text::Line;

use super::Page;
use crate::app::chair::{ChairState, Square, SquareState};
use crate::app::hub::state::{TicketCost, WaitKind, WaitState};
use crate::cli::chair::panes::View;
use crate::cli::chair::panes::now::age;
use crate::domain::screen::Meaning;
use crate::domain::{EpicStage, TicketId};

/// The title: `Why #233`.
pub fn title(ticket: TicketId) -> String {
    format!("Why {ticket}")
}

/// The square of `ticket`, in whichever epic it is.
pub fn square(state: &ChairState, ticket: TicketId) -> Option<&Square> {
    state
        .epics
        .iter()
        .flat_map(|epic| &epic.squares)
        .find(|square| square.ticket == ticket)
}

/// A commit as a reader takes it in: its first eight characters.
fn commit(wait: &WaitState) -> String {
    wait.commit.as_deref().map_or_else(
        || "its head".to_owned(),
        |commit| commit.chars().take(8).collect(),
    )
}

/// One wait as a sentence, in the tick's words, with what the Engineer may do about it.
fn wait_sentence(state: &ChairState, wait: &WaitState) -> (Option<Meaning>, String) {
    let number = wait.number.map(TicketId::from);
    let id = number.map_or_else(String::new, |number| number.to_string());
    match wait.on {
        WaitKind::Epic => match number.and_then(|epic| state.epic(epic)) {
            Some(epic) if epic.stage == EpicStage::Unsigned => (
                Some(Meaning::Waiting),
                format!("its epic {id} is not signed: s on the epic signs it"),
            ),
            _ => (None, format!("its epic {id} is not challenged yet")),
        },
        WaitKind::Question => (
            Some(Meaning::Waiting),
            format!("your question {id} is open: answer it in the inbox"),
        ),
        WaitKind::Ticket => {
            let word = number
                .and_then(|ticket| square(state, ticket))
                .map_or_else(String::new, |blocker| {
                    format!(" (it is {})", blocker.state.word())
                });
            (None, format!("{id} is not done{word}"))
        }
        WaitKind::Check => (
            None,
            format!("the checks at {} have not ended", commit(wait)),
        ),
        WaitKind::Merge => (None, format!("{id} is fresh from a fix and merges first")),
        WaitKind::Base => (
            None,
            format!(
                "the integration branch's checks at {} have not ended",
                commit(wait)
            ),
        ),
    }
}

/// `tokens` as a reader takes them in: `850`, `45k`, `1.2M`.
pub fn tokens(tokens: u64) -> String {
    match tokens {
        0..1_000 => tokens.to_string(),
        1_000..1_000_000 => format!("{}k", tokens / 1_000),
        _ => format!("{}.{}M", tokens / 1_000_000, tokens % 1_000_000 / 100_000),
    }
}

/// The cost line: `so far: 12m05s · 1.2M tokens`.
pub fn cost_words(cost: Option<&TicketCost>) -> String {
    cost.map_or_else(
        || "so far: no run of it in the ledger yet".to_owned(),
        |cost| {
            format!(
                "so far: {} · {} tokens",
                age(cost.seconds),
                tokens(cost.tokens)
            )
        },
    )
}

/// Why `ticket` waits, line by line, each with how it is painted (none: plain); none when it is
/// on no open epic.
pub fn why(state: &ChairState, ticket: TicketId) -> Option<Vec<(Option<Meaning>, String)>> {
    let this = square(state, ticket)?;
    let mut lines: Vec<(Option<Meaning>, String)> = Vec::new();
    match this.state {
        SquareState::Done => lines.push((Some(Meaning::Done), "Done.".to_owned())),
        SquareState::Running => lines.push((
            None,
            format!(
                "Running{}. l follows it live.",
                this.phase
                    .as_deref()
                    .map_or_else(String::new, |phase| format!(": {phase}"))
            ),
        )),
        SquareState::Failed => lines.push((
            Some(Meaning::Failed),
            "Its instance is gone and it still holds the claim; the next tick frees it and \
             starts it again."
                .to_owned(),
        )),
        SquareState::Ready => lines.push((
            None,
            match state.now.slots {
                Some(slots) if slots.in_use >= slots.total => format!(
                    "Ready: it starts when a slot frees ({} of {} busy).",
                    slots.in_use, slots.total
                ),
                Some(slots) => format!(
                    "Ready: the next tick starts it ({} of {} slots busy).",
                    slots.in_use, slots.total
                ),
                None => "Ready: it starts when a slot frees.".to_owned(),
            },
        )),
        SquareState::Question | SquareState::Waiting => {}
    }
    if !this.waits.is_empty() {
        lines.push((None, "It waits on:".to_owned()));
        for wait in &this.waits {
            let (meaning, words) = wait_sentence(state, wait);
            lines.push((meaning, format!("· {words}")));
        }
    } else if matches!(this.state, SquareState::Question | SquareState::Waiting) {
        // The core does not place it: what its board says holds it.
        let open: Vec<TicketId> = this
            .blocked_by
            .iter()
            .copied()
            .filter(|blocker| {
                square(state, *blocker).is_none_or(|other| other.state != SquareState::Done)
            })
            .collect();
        if let Some(question) = this.question {
            lines.push((
                Some(Meaning::Waiting),
                format!("Held by your question {question}: answer it in the inbox."),
            ));
        }
        if !open.is_empty() {
            lines.push((None, "It waits on:".to_owned()));
            for blocker in open {
                let word = square(state, blocker).map_or_else(String::new, |other| {
                    format!(" (it is {})", other.state.word())
                });
                lines.push((None, format!("· {blocker} is not done{word}")));
            }
        }
        if this.question.is_none() && this.blocked_by.is_empty() {
            lines.push((
                Some(Meaning::Dim),
                "The core does not place it, and its board names nothing it waits on.".to_owned(),
            ));
        }
    }
    lines.push((None, String::new()));
    lines.push((Some(Meaning::Dim), phase_words(this)));
    // The queue holds the open tickets' pull requests only: a done ticket's is not known here.
    let pull_request = match &this.pull_request {
        Some((number, Some(url))) => Some(format!("pull request #{number}: {url}")),
        Some((number, None)) => Some(format!("pull request #{number}")),
        None if this.state == SquareState::Done => None,
        None => Some("no pull request yet".to_owned()),
    };
    lines.extend(pull_request.map(|words| (Some(Meaning::Dim), words)));
    lines.push((Some(Meaning::Dim), cost_words(this.cost.as_ref())));
    Some(lines)
}

/// Its phase in words: `next phase: ci (1 of 4 ended)`.
pub fn phase_words(square: &Square) -> String {
    let ended = format!("{} of {} ended", square.phases_done, square.phases_total);
    match (&square.phase, square.state) {
        (Some(phase), SquareState::Running) => format!("phase now: {phase} ({ended})"),
        (Some(phase), _) => format!("next phase: {phase} ({ended})"),
        (None, _) if square.phases_total > 0 && square.phases_done >= square.phases_total => {
            format!("every phase has ended ({ended})")
        }
        (None, _) => "no phase: the core does not place it".to_owned(),
    }
}

/// Why `ticket` waits.
pub fn page(page: &mut Page<'_, '_>, state: &ChairState, ticket: TicketId) {
    let (Some(this), Some(lines)) = (square(state, ticket), why(state, ticket)) else {
        page.text(
            Some(Meaning::Dim),
            &format!("{ticket} is not on an open epic."),
        );
        return;
    };
    let view = *page.view();
    page.text(Some(Meaning::Bold), &this.title);
    page.line(Line::from(vec![
        view.square(this.state),
        View::plain(" "),
        View::plain(this.state.word()),
    ]));
    page.blank();
    for (meaning, text) in lines {
        if text.is_empty() {
            page.blank();
        } else {
            page.text(meaning, &text);
        }
    }
}
