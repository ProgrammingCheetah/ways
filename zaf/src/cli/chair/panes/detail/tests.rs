use super::tree::{Short, layout, short};
use super::why::{cost_words, phase_words, tokens, why};
use crate::app::chair::{EpicView, SquareState};
use crate::app::hub::state::{TicketCost, WaitKind};
use crate::cli::chair::support::{square, wait, ways};
use crate::domain::screen::Meaning;
use crate::domain::{EpicStage, TicketId};

/// The rows of `epic`'s tree as `prefix #ticket ← waits`.
fn drawn(epic: &EpicView) -> Vec<String> {
    layout(epic)
        .into_iter()
        .map(|row| {
            let waits: Vec<String> = row.waits.into_iter().map(|wait| wait.text).collect();
            if waits.is_empty() {
                format!("{}{}", row.prefix, row.ticket)
            } else {
                format!("{}{} ← {}", row.prefix, row.ticket, waits.join(" "))
            }
        })
        .collect()
}

#[test]
fn the_tree_hangs_each_ticket_under_its_first_blocker_in_the_epic() {
    let state = ways();

    assert_eq!(
        drawn(&state.epics[0]),
        [
            "├─#230",
            "│  ├─#233 ← #230",
            "│  │  └─#240 ← #233",
            "│  └─#234 ← ?#285",
            "├─#235",
            "├─#241",
            "└─#243",
        ]
    );
}

#[test]
fn a_cycle_is_drawn_once_from_its_first_ticket() {
    let blocked = |ticket: u64, by: u64| crate::app::chair::Square {
        blocked_by: vec![TicketId::from(by)],
        ..square(ticket, SquareState::Waiting)
    };
    let epic = EpicView {
        number: TicketId::from(9),
        title: "a knot".to_owned(),
        stage: EpicStage::Planned,
        release: None,
        done: 0,
        total: 4,
        question: None,
        squares: vec![
            blocked(1, 2),
            blocked(2, 1),
            square(3, SquareState::Ready),
            blocked(4, 77),
        ],
    };

    assert_eq!(
        drawn(&epic),
        ["├─#3", "└─#4 ← #77", "└─#1 ← #2", "   └─#2 ← #1",],
        "a blocker outside the epic makes a root; the board's blockers stand in for the core's"
    );
}

#[test]
fn a_wait_in_short_marks_the_engineers() {
    assert_eq!(
        short(&wait(WaitKind::Question, 285)),
        Short {
            text: "?#285".to_owned(),
            yours: true
        }
    );
    assert_eq!(short(&wait(WaitKind::Ticket, 230)).text, "#230");
    assert_eq!(short(&wait(WaitKind::Epic, 224)).text, "its epic");
}

/// The words of `why` for `ticket`, unpainted.
fn words(state: &crate::app::chair::ChairState, ticket: u64) -> Vec<String> {
    why(state, TicketId::from(ticket))
        .expect("on an open epic")
        .into_iter()
        .map(|(_, text)| text)
        .collect()
}

#[test]
fn why_says_what_a_ticket_waits_on_in_the_ticks_words() {
    let state = ways();

    assert_eq!(
        words(&state, 233),
        [
            "It waits on:",
            "· #230 is not done (it is orphaned)",
            "",
            "next phase: implement (0 of 4 ended)",
            "no pull request yet",
            "so far: no run of it in the ledger yet",
        ]
    );
    let question = why(&state, TicketId::from(234)).expect("on an open epic");
    assert_eq!(
        question[1],
        (
            Some(Meaning::Waiting),
            "· your question #285 is open: answer it in the inbox".to_owned()
        )
    );
    assert_eq!(why(&state, TicketId::from(999)), None);
}

#[test]
fn why_says_a_running_ticket_its_phase_pull_request_and_cost() {
    let state = ways();

    assert_eq!(
        words(&state, 243),
        [
            "Running: implement. l follows it live.",
            "",
            "phase now: implement (0 of 4 ended)",
            "pull request #337: https://forge.invalid/pulls/337",
            "so far: 12m05s · 1.2M tokens",
        ]
    );
    assert_eq!(
        words(&state, 241)[0],
        "Ready: the next tick starts it (1 of 4 slots busy)."
    );
    assert_eq!(
        words(&state, 235),
        [
            "Done.",
            "",
            "every phase has ended (4 of 4 ended)",
            "so far: no run of it in the ledger yet",
        ],
        "a done ticket's pull request is not in the queue, so none is named"
    );
}

#[test]
fn why_says_an_unsigned_epic_is_the_engineers_to_sign() {
    let mut state = ways();
    state.epics[1].stage = EpicStage::Unsigned;
    state.epics[1].squares[1].waits = vec![wait(WaitKind::Epic, 226)];
    state.epics[1].squares[1].state = SquareState::Waiting;

    assert_eq!(
        words(&state, 257)[1],
        "· its epic #226 is not signed: s on the epic signs it"
    );
}

#[test]
fn phases_tokens_and_time_read_as_words() {
    let mut done = square(235, SquareState::Done);
    done.phases_done = 4;
    done.phases_total = 4;
    assert_eq!(phase_words(&done), "every phase has ended (4 of 4 ended)");
    assert_eq!(tokens(850), "850");
    assert_eq!(tokens(45_300), "45k");
    assert_eq!(tokens(1_250_000), "1.2M");
    assert_eq!(
        cost_words(Some(&TicketCost {
            tokens: 2_000,
            seconds: 59
        })),
        "so far: 59s · 2k tokens"
    );
}

#[test]
fn the_answer_box_starts_with_the_first_paragraph_only() {
    use super::prefill;
    assert_eq!(
        prefill("Yes, keep it.\n\n\nAlso close #12."),
        "Yes, keep it."
    );
    assert_eq!(
        prefill("\n\n  \nYes,\nkeep it.  \n\nNo more."),
        "Yes,\nkeep it."
    );
    assert_eq!(prefill("Yes.\r\n\r\nAnd close #12."), "Yes.");
    let long = format!("Yes {}", "word ".repeat(200));
    let cut = prefill(&long);
    assert!(cut.ends_with("word…"), "{cut}");
    assert!(cut.chars().count() <= super::PREFILL_CAP + 1);
}

#[test]
fn a_run_of_blank_lines_is_one() {
    use super::collapse_blank_lines;
    assert_eq!(
        collapse_blank_lines("\n\na\n\n\n  \nb\r\n\r\nc\n\n"),
        "a\n\nb\n\nc"
    );
}

#[test]
fn the_answer_box_scrolls_to_its_cursor_and_counts_what_is_hidden() {
    use super::AnswerView;
    use crate::cli::chair::panes::AnswerBox;
    let mut answer = AnswerBox::new(TicketId::from(285), "a\nb\nc\nd\ne\nf".to_owned());
    // 30 columns: a heading of two lines, two borders and a line of keys around the box.
    let view = AnswerView::of(&answer, 30, 8);
    assert_eq!((view.lines.len(), view.shown), (6, 3));
    assert_eq!((view.above(), view.below(), view.hidden()), (3, 0, 3));
    assert_eq!(view.lines[view.top + view.shown - 1], "f▏");

    answer.back = 2;
    let view = AnswerView::of(&answer, 30, 8);
    assert_eq!((view.above(), view.below()), (1, 2));

    let nowhere = AnswerView::of(&answer, 0, 0);
    assert_eq!(nowhere.shown, 0, "a Detail not laid out shows none of it");
    assert_eq!(nowhere.hidden(), nowhere.lines.len());
    assert_eq!(AnswerView::of(&answer, 30, 40).hidden(), 0);
}
