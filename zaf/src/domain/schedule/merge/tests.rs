use super::super::tests::*;
use super::*;
use crate::domain::schedule::{
    BaseFacts, CheckFact, CheckName, CheckResult, Dependency, EpicFacts, Phase, Place, Placement,
    PullFacts, WaitingOn, place,
};

/// The pack's list with the merge failing to `fix`, as a conflict does.
fn list() -> Vec<Phase> {
    let mut list = pack();
    list[3].on_fail = Some(name("fix"));
    list
}

/// Ticket `number` with a pull request at `head`, opened at 20, with no check, review or run.
fn opened(number: u64, head: &str) -> TicketFacts {
    let mut facts = facts(list());
    facts.ticket = id(number);
    facts.pull = Some(pull(head, 20));
    facts
}

/// `facts` with a green check and a clean review at its head: the next run is the merge.
fn reviewed(mut facts: TicketFacts) -> TicketFacts {
    let head = facts.head().map(|head| head.as_str().to_owned());
    let head = head.as_deref();
    if let Some(pull) = facts.pull.as_mut() {
        pull.checks = vec![passed(70)];
    }
    facts
        .runs
        .push(run("review", head, 80).exits(0, 85).leaves(head));
    facts
}

/// Ticket `number` standing at its merge at `h<number>`, with no fix behind it.
fn at_its_merge(number: u64) -> TicketFacts {
    reviewed(opened(number, &format!("h{number}")))
}

/// Ticket `number` whose `fix` took it from `old` to `f<number>`, exited 0 at `ended` and raised
/// no question; its checks at the new head are still pending.
fn fixed(number: u64, ended: u64) -> TicketFacts {
    let head = format!("f{number}");
    let mut facts = opened(number, &head);
    if let Some(pull) = facts.pull.as_mut() {
        pull.checks = vec![pending()];
    }
    facts.runs.push(
        run("fix", Some("old"), ended - 5)
            .exits(0, ended)
            .leaves(Some(&head)),
    );
    facts
}

/// Each of `tickets` placed at [`now`], once the merges are ordered across them.
fn placed(mut tickets: Vec<TicketFacts>) -> Vec<Place> {
    order_merges(&mut tickets);
    tickets.iter().map(|facts| place(facts, now())).collect()
}

fn on_merge_of(number: u64) -> Place {
    waiting(&[WaitingOn::Merge(id(number))])
}

fn edit_pull(facts: &mut TicketFacts, edit: impl FnOnce(&mut PullFacts)) {
    if let Some(pull) = facts.pull.as_mut() {
        edit(pull);
    }
}

#[test]
fn a_ticket_stands_at_its_merge_when_its_next_run_is_the_merge_itself() {
    assert!(at_merge(&at_its_merge(1)));
    assert!(!at_merge(&opened(1, "h1")), "its checks come first");
    // A merge that met a conflict runs its on-fail next, which is no merge.
    let mut conflicted = at_its_merge(1);
    conflicted
        .runs
        .push(run("merge", Some("h1"), 90).exits(1, 91).leaves(Some("h1")));
    assert!(!at_merge(&conflicted));
    // One that failed on something no on-fail answers runs again, and is a merge.
    let mut refused = at_its_merge(1);
    refused
        .runs
        .push(run("merge", Some("h1"), 90).exits(2, 91).leaves(Some("h1")));
    assert!(at_merge(&refused));
}

#[test]
fn a_ticket_whose_head_a_fix_left_is_fresh_from_it_until_it_merges() {
    let fresh = fixed(12, 50);
    assert_eq!(fresh_from_fix(&fresh), Some(time(50)));
    // Its checks passing and its review running do not end it: only the merge does.
    let reviewed = reviewed(fresh);
    assert_eq!(fresh_from_fix(&reviewed), Some(time(50)));
    let mut merged = reviewed;
    edit_pull(&mut merged, |pull| pull.merged_at = Some(time(95)));
    assert_eq!(fresh_from_fix(&merged), None);
}

#[test]
fn a_ticket_no_fix_brought_to_its_head_is_not_fresh() {
    assert_eq!(fresh_from_fix(&at_its_merge(1)), None);
    assert_eq!(fresh_from_fix(&facts(list())), None, "no pull request");
    // A fix that left the head where it was brought the ticket nowhere.
    let mut stayed = opened(1, "h1");
    stayed
        .runs
        .push(run("fix", Some("h1"), 40).exits(0, 45).leaves(Some("h1")));
    assert_eq!(fresh_from_fix(&stayed), None);
    // Nor is a run of an action no phase names as its on-fail.
    let mut implemented = opened(1, "h1");
    implemented.runs.push(
        run("implement", Some("old"), 40)
            .exits(0, 45)
            .leaves(Some("h1")),
    );
    assert_eq!(fresh_from_fix(&implemented), None);
}

#[test]
fn a_fix_that_failed_asked_or_was_stopped_leaves_no_fresh_ticket() {
    let moved = |exit: Option<i32>, asks: bool| {
        let mut facts = opened(1, "f1");
        let mut fix = run("fix", Some("old"), 40).leaves(Some("f1"));
        if let Some(code) = exit {
            fix = fix.exits(code, 45);
        }
        if asks {
            fix = fix.asks(61);
        }
        facts.runs.push(fix);
        facts
    };
    assert_eq!(fresh_from_fix(&moved(Some(0), false)), Some(time(45)));
    assert_eq!(fresh_from_fix(&moved(Some(1), false)), None, "it failed");
    assert_eq!(fresh_from_fix(&moved(Some(0), true)), None, "it asked");
    assert_eq!(fresh_from_fix(&moved(Some(143), false)), None, "stopped");
    assert_eq!(fresh_from_fix(&moved(None, false)), None, "no exit");
}

#[test]
fn a_ticket_at_its_merge_waits_on_the_ticket_fresh_from_a_fix() {
    let places = placed(vec![at_its_merge(3), fixed(12, 50), at_its_merge(14)]);
    assert_eq!(places[0], on_merge_of(12));
    assert_eq!(places[2], on_merge_of(12));
    // The fresh ticket waits on its own checks, and on nobody's merge.
    assert_eq!(
        places[1],
        waiting(&[WaitingOn::Check(id(12), commit("f12"))])
    );
}

#[test]
fn a_ticket_not_at_its_merge_is_never_held_by_a_fresh_one() {
    // #3 is in the lane at its review, #4 has no pull request yet, #5 waits on its checks.
    let mut reviewing = opened(3, "h3");
    edit_pull(&mut reviewing, |pull| pull.checks = vec![passed(70)]);
    let mut starting = facts(list());
    starting.ticket = id(4);
    let places = placed(vec![reviewing, starting, opened(5, "h5"), fixed(12, 50)]);
    assert_eq!(places[0], in_lane("review", 70));
    assert_eq!(places[1], in_lane("implement", 1));
    assert_eq!(places[2], waiting(&[WaitingOn::Check(id(5), commit("h5"))]));
}

#[test]
fn the_hold_ends_when_the_fresh_ticket_merges() {
    let mut merged = reviewed(fixed(12, 50));
    edit_pull(&mut merged, |pull| pull.merged_at = Some(time(95)));
    let places = placed(vec![at_its_merge(3), merged]);
    assert_eq!(places[0], in_lane("merge", 85));
    assert_eq!(places[1], Place::Done);
}

#[test]
fn the_hold_ends_when_the_fresh_tickets_phase_fails_at_that_head() {
    // A red check at the head the fix left.
    let mut red = fixed(12, 50);
    edit_pull(&mut red, |pull| pull.checks = vec![failed(60)]);
    assert_eq!(fresh_from_fix(&red), None);
    let places = placed(vec![at_its_merge(3), red]);
    assert_eq!(places[0], in_lane("merge", 85));
    assert_eq!(places[1], in_lane("fix", 60));

    // A new conflict: its merge exited 1 there.
    let mut conflicted = reviewed(fixed(12, 50));
    conflicted.runs.push(
        run("merge", Some("f12"), 90)
            .exits(1, 91)
            .leaves(Some("f12")),
    );
    assert_eq!(fresh_from_fix(&conflicted), None);
    let places = placed(vec![at_its_merge(3), conflicted]);
    assert_eq!(places[0], in_lane("merge", 85));
    assert_eq!(places[1], in_lane("fix", 91));
}

#[test]
fn the_hold_ends_when_a_question_opens_on_the_fresh_ticket() {
    let mut asked = fixed(12, 50);
    asked.open_questions.insert(id(61));
    assert_eq!(fresh_from_fix(&asked), None);
    let places = placed(vec![at_its_merge(3), asked]);
    assert_eq!(places[0], in_lane("merge", 85));

    // An epic that lost its signature is the Engineer's too.
    let mut unsigned = fixed(12, 50);
    unsigned.epic = Some(EpicFacts {
        id: id(1),
        signed: false,
        challenged: true,
    });
    assert_eq!(fresh_from_fix(&unsigned), None);
}

#[test]
fn the_hold_ends_when_the_fresh_tickets_head_moves_by_anything_but_a_fix() {
    // A push by hand: no run on record brought the pull request to its head.
    let mut pushed = fixed(12, 50);
    edit_pull(&mut pushed, |pull| pull.head = Some(commit("by-hand")));
    assert_eq!(fresh_from_fix(&pushed), None);
    let places = placed(vec![at_its_merge(3), pushed]);
    assert_eq!(places[0], in_lane("merge", 85));

    // Another action's run moved it on.
    let mut moved = fixed(12, 50);
    edit_pull(&mut moved, |pull| pull.head = Some(commit("later")));
    moved.runs.push(
        run("implement", Some("f12"), 60)
            .exits(0, 65)
            .leaves(Some("later")),
    );
    assert_eq!(fresh_from_fix(&moved), None);

    // A second fix moving it keeps it fresh, from when that one ended.
    let mut again = fixed(12, 50);
    edit_pull(&mut again, |pull| pull.head = Some(commit("f12b")));
    again.runs.push(
        run("fix", Some("f12"), 60)
            .exits(0, 65)
            .leaves(Some("f12b")),
    );
    assert_eq!(fresh_from_fix(&again), Some(time(65)));
}

#[test]
fn a_fresh_ticket_that_waits_on_a_ticket_holds_nobody() {
    let mut depends = fixed(12, 50);
    depends.dependencies = vec![Dependency {
        ticket: id(3),
        standing: Standing::Open,
    }];
    assert_eq!(fresh_from_fix(&depends), None);
    let places = placed(vec![at_its_merge(3), depends]);
    assert_eq!(places[0], in_lane("merge", 85), "or the two would wait");
}

#[test]
fn two_fresh_tickets_do_not_wait_on_each_other() {
    // Both stand at their merge; #12's fix ended first.
    let tickets = vec![
        reviewed(fixed(9, 60)),
        reviewed(fixed(12, 50)),
        at_its_merge(14),
    ];
    assert_eq!(merges_first(&tickets), Some(id(12)));
    let places = placed(tickets);
    assert_eq!(places[0], on_merge_of(12));
    assert_eq!(places[1], in_lane("merge", 85));
    assert_eq!(places[2], on_merge_of(12));

    // Two fixes that ended together: the lower ticket goes first, whatever the order given.
    for tickets in [
        vec![reviewed(fixed(12, 50)), reviewed(fixed(9, 50))],
        vec![reviewed(fixed(9, 50)), reviewed(fixed(12, 50))],
    ] {
        assert_eq!(merges_first(&tickets), Some(id(9)));
        let first = tickets
            .iter()
            .position(|facts| facts.ticket == id(9))
            .expect("#9 is there");
        let places = placed(tickets);
        assert_eq!(places[first], in_lane("merge", 85));
        assert_eq!(places[1 - first], on_merge_of(9));
    }
}

#[test]
fn once_the_first_fresh_ticket_merges_the_second_goes() {
    let mut merged = reviewed(fixed(12, 50));
    edit_pull(&mut merged, |pull| pull.merged_at = Some(time(95)));
    let places = placed(vec![reviewed(fixed(9, 60)), merged, at_its_merge(14)]);
    assert_eq!(places[0], in_lane("merge", 85));
    assert_eq!(places[2], on_merge_of(9));
}

#[test]
fn the_placement_lists_a_merge_wait_under_the_ticket_waited_on() {
    let mut tickets = vec![at_its_merge(3), fixed(12, 50)];
    order_merges(&mut tickets);
    let placement = Placement::of(&tickets, now());
    assert_eq!(
        placement.waiting.get(&WaitingOn::Merge(id(12))),
        Some(&[id(3)].into())
    );
    assert!(placement.lane.entries().is_empty());
}

/// A base at `base1` with these checks.
fn base(checks: Vec<CheckFact>) -> BaseFacts {
    BaseFacts {
        head: Some(commit("base1")),
        checks,
    }
}

/// Where a ticket at its merge sits over `base`.
fn merging_over(base: BaseFacts) -> Place {
    let mut facts = at_its_merge(3);
    facts.merging.base = base;
    place(&facts, now())
}

#[test]
fn a_merge_waits_while_the_integration_branch_has_a_check_pending() {
    assert_eq!(
        merging_over(base(vec![pending()])),
        waiting(&[WaitingOn::Base(commit("base1"))])
    );
    assert_eq!(
        merging_over(base(vec![
            check("lint", CheckResult::Passed(time(90))),
            pending()
        ])),
        waiting(&[WaitingOn::Base(commit("base1"))])
    );
}

#[test]
fn a_merge_does_not_wait_on_a_base_with_no_check_every_check_ended_or_unknown() {
    let lane = in_lane("merge", 85);
    assert_eq!(merging_over(base(Vec::new())), lane, "no check there");
    assert_eq!(merging_over(base(vec![passed(90)])), lane, "all passed");
    assert_eq!(merging_over(BaseFacts::default()), lane, "unknown");
    // An unknown base holds nothing even if checks came with it.
    let headless = BaseFacts {
        head: None,
        checks: vec![pending()],
    };
    assert_eq!(merging_over(headless), lane);
}

#[test]
fn a_red_integration_branch_does_not_hold_a_merge() {
    let lane = in_lane("merge", 85);
    assert_eq!(merging_over(base(vec![failed(90)])), lane);
    // Not even with another check still pending: a held merge could never land the fix.
    let red = base(vec![failed(90), check("lint", CheckResult::Pending)]);
    assert_eq!(red.awaited(), None);
    assert_eq!(red.failed_checks(), [CheckName::from("ci")]);
    assert_eq!(red.pending_checks(), [CheckName::from("lint")]);
    assert_eq!(merging_over(red), lane);
}

#[test]
fn the_base_holds_only_a_ticket_that_would_be_in_the_lane_at_its_merge() {
    let busy = base(vec![pending()]);
    // At its review: in the lane, whatever the base.
    let mut reviewing = opened(3, "h3");
    edit_pull(&mut reviewing, |pull| pull.checks = vec![passed(70)]);
    reviewing.merging.base = busy.clone();
    assert_eq!(place(&reviewing, now()), in_lane("review", 70));
    // At its merge with a question open: it waits on the question alone.
    let mut asked = at_its_merge(3);
    asked.open_questions.insert(id(61));
    asked.merging.base = busy.clone();
    assert_eq!(
        place(&asked, now()),
        waiting(&[WaitingOn::Question(id(61))])
    );
    // A merge that met a conflict: its fix runs, base or not.
    let mut conflicted = at_its_merge(3);
    conflicted
        .runs
        .push(run("merge", Some("h3"), 90).exits(1, 91).leaves(Some("h3")));
    conflicted.merging.base = busy;
    assert_eq!(place(&conflicted, now()), in_lane("fix", 91));
}

#[test]
fn a_merge_waits_on_the_fresh_ticket_and_on_the_base_together() {
    let mut tickets = vec![at_its_merge(3), fixed(12, 50)];
    tickets[0].merging.base = base(vec![pending()]);
    let places = placed(tickets);
    assert_eq!(
        places[0],
        waiting(&[WaitingOn::Merge(id(12)), WaitingOn::Base(commit("base1"))])
    );
}

#[test]
fn a_ticket_is_never_held_by_its_own_number() {
    // Facts filled by hand with the ticket itself: read as nobody.
    let mut facts = at_its_merge(3);
    facts.merging.first = Some(id(3));
    assert_eq!(place(&facts, now()), in_lane("merge", 85));
}
