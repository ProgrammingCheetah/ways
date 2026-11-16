use super::*;
use crate::app::schedule::trace::{Did, RanStep, ShortPlace};
use crate::domain::schedule::ActionName;

fn id(number: u64) -> TicketId {
    TicketId::from(number)
}

fn time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

/// 2026-11-12 17:15:00 UTC.
const AT: u64 = 1_794_503_700;

fn moved(ticket: u64, from: Option<&str>, to: Option<&str>) -> Move {
    Move {
        ticket: id(ticket),
        from: from.map(ShortPlace::from),
        to: to.map(ShortPlace::from),
    }
}

fn line(at: u64, moved: Vec<Move>) -> Recorded {
    Recorded {
        at: time(at),
        places: None,
        moved,
        did: Did::default(),
    }
}

/// Three ticks on one day: #7 enters the lane, then starts with #8 new, then a quiet tick.
fn one_day() -> Vec<Recorded> {
    vec![
        line(AT, vec![moved(7, None, Some("lane(implement)"))]),
        line(
            AT + 300,
            vec![
                moved(7, Some("lane(implement)"), Some("running(implement)")),
                moved(8, None, Some("waiting(ticket #7)")),
            ],
        ),
        line(AT + 600, Vec::new()),
    ]
}

#[test]
fn the_moves_print_oldest_first_with_the_time_of_their_tick() {
    assert_eq!(
        lines(&one_day(), &Filter::default(), false, 0),
        [
            "17:15 #7 new -> lane(implement)",
            "17:20 #7 lane(implement) -> running(implement)",
            "17:20 #8 new -> waiting(ticket #7)",
        ]
    );
    // Local time: two hours east of UTC.
    assert_eq!(
        lines(&one_day(), &Filter::default(), false, 2 * 3600)[0],
        "19:15 #7 new -> lane(implement)"
    );
}

#[test]
fn ticks_written_out_of_order_are_listed_by_when_they_began() {
    let mut ticks = one_day();
    ticks.swap(0, 1);
    assert_eq!(
        lines(&ticks, &Filter::default(), false, 0)[0],
        "17:15 #7 new -> lane(implement)"
    );
}

#[test]
fn a_ticket_keeps_its_own_moves() {
    let filter = Filter {
        ticket: Some(id(8)),
        since: None,
    };
    assert_eq!(
        lines(&one_day(), &filter, false, 0),
        ["17:20 #8 new -> waiting(ticket #7)"]
    );
    let none = Filter {
        ticket: Some(id(99)),
        since: None,
    };
    assert_eq!(lines(&one_day(), &none, false, 0), Vec::<String>::new());
}

#[test]
fn since_keeps_the_ticks_from_that_time_on() {
    let filter = Filter {
        ticket: None,
        since: Some(time(AT + 1)),
    };
    assert_eq!(lines(&one_day(), &filter, false, 0).len(), 2);
}

#[test]
fn a_listing_that_crosses_a_day_shows_each_date() {
    let ticks = vec![
        line(AT, vec![moved(7, None, Some("lane(implement)"))]),
        line(
            AT + 7 * 3600,
            vec![moved(
                7,
                Some("lane(implement)"),
                Some("running(implement)"),
            )],
        ),
    ];
    assert_eq!(
        lines(&ticks, &Filter::default(), false, 0),
        [
            "2026-11-12",
            "17:15 #7 new -> lane(implement)",
            "2026-11-13",
            "00:15 #7 lane(implement) -> running(implement)",
        ]
    );
    // In a zone where both are on one day, no date is shown.
    assert_eq!(
        lines(&ticks, &Filter::default(), false, -3 * 3600),
        [
            "14:15 #7 new -> lane(implement)",
            "21:15 #7 lane(implement) -> running(implement)",
        ]
    );
}

#[test]
fn more_adds_what_the_tick_did_under_its_moves() {
    let mut ticks = one_day();
    ticks[0].did = Did {
        ran: vec![RanStep {
            task: id(3),
            action: ActionName::from("merge"),
            exit: 0,
        }],
        ..Did::default()
    };
    assert_eq!(
        lines(&ticks[..1], &Filter::default(), true, 0),
        [
            "17:15 #7 new -> lane(implement)",
            "      ran merge #3: exit 0"
        ]
    );
}

#[test]
fn the_json_lists_each_move_with_its_time() {
    let filter = Filter {
        ticket: Some(id(7)),
        since: None,
    };
    assert_eq!(
        to_json(&one_day(), &filter, false),
        json!({"moves": [
            {"at": "2026-11-12T17:15:00Z", "ticket": 7, "from": null, "to": "lane(implement)"},
            {"at": "2026-11-12T17:20:00Z", "ticket": 7, "from": "lane(implement)",
             "to": "running(implement)"},
        ]})
    );
    let more = to_json(&one_day(), &filter, true);
    assert_eq!(more["moves"][0]["did"]["ran"], json!([]));
    assert_eq!(
        to_json(&[], &Filter::default(), false),
        json!({"moves": []})
    );
}

#[test]
fn since_reads_a_time_or_a_span_back_from_now() {
    let now = time(AT);
    assert_eq!(since("2026-11-12T17:00:00Z", now), Ok(time(AT - 900)));
    assert_eq!(since("2026-11-12 17:00:00", now), Ok(time(AT - 900)));
    assert_eq!(since("2h", now), Ok(time(AT - 7200)));
    assert_eq!(since("3d", now), Ok(time(AT - 3 * 86_400)));
    assert_eq!(since(" 90min ", now), Ok(time(AT - 5400)));
    let refused = since("yesterday", now).expect_err("no such time");
    assert!(refused.contains("`yesterday`"), "{refused}");
}
