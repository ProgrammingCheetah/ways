use super::{FilingMark, GraphHash, PlanRecord, RecordError};
use crate::domain::{BodyHash, TicketBody, TicketId};

fn graph() -> GraphHash {
    "ab".repeat(32).parse().unwrap()
}

fn record(plan: &str) -> PlanRecord {
    PlanRecord {
        k: 2,
        design: BodyHash::of(&TicketBody::from("a design")),
        dropped: vec![TicketId::from(41), TicketId::from(57)],
        plan: plan.to_owned(),
    }
}

fn round_trip(record: PlanRecord) {
    let text = record.to_string();
    assert_eq!(text.parse::<PlanRecord>(), Ok(record), "{text}");
}

#[test]
fn plan_record_round_trips() {
    let plan = "nodes: []\n```json\na: 1\n```\n";
    let with_fence = record(plan);
    let text = with_fence.to_string();
    assert!(text.starts_with("<!-- zaf:plan-record 2 -->\ndesign sha256:"));
    assert!(text.contains("\ndropped: #41 #57\n"));
    round_trip(with_fence);
    round_trip(PlanRecord {
        dropped: vec![],
        ..record("")
    });
}

#[test]
fn plan_record_with_a_fence_in_the_plan_round_trips() {
    for plan in ["a ``` b", "```\nx\n```", "````\n```\n````\n", "``````", "`"] {
        let record = record(plan);
        let longest = plan.split(|c| c != '`').map(str::len).max().unwrap();
        let text = record.to_string();
        let fence = text.lines().nth(3).unwrap();
        assert!(fence.len() > longest, "{text}");
        round_trip(record);
    }
}

#[test]
fn filing_mark_round_trips() {
    let mark = FilingMark {
        k: 2,
        graph: graph(),
    };
    let text = mark.to_string();
    assert!(text.starts_with("<!-- zaf:plan-filed 2 -->\ngraph sha256:"));
    assert_eq!(text.parse::<FilingMark>(), Ok(mark.clone()));
    assert_eq!(format!("{text}\n").parse::<FilingMark>(), Ok(mark));
}

#[test]
fn unmarked_comment_is_not_a_record() {
    let plain = "plan record 2\ndesign sha256:00\n";
    assert!(matches!(
        plain.parse::<PlanRecord>(),
        Err(RecordError::NotMarked(_))
    ));
    assert!(matches!(
        plain.parse::<FilingMark>(),
        Err(RecordError::NotMarked(_))
    ));
    // A record is not a mark and a mark is not a record.
    assert!(record("p").to_string().parse::<FilingMark>().is_err());
    let mark = FilingMark {
        k: 2,
        graph: graph(),
    };
    assert!(mark.to_string().parse::<PlanRecord>().is_err());
    // A marker whose lines do not read.
    let broken = "<!-- zaf:plan-record 2 -->\ndesign sha256:xyz\ndropped:\n```\np\n```";
    assert!(matches!(
        broken.parse::<PlanRecord>(),
        Err(RecordError::Unreadable { line: 2, .. })
    ));
    let open = record("p").to_string().replace("\n```", "");
    assert!(open.parse::<PlanRecord>().is_err());
    assert!(
        "<!-- zaf:plan-filed two -->\ngraph sha256:"
            .parse::<FilingMark>()
            .is_err()
    );
}

#[test]
fn graph_hash_refuses_bad_hex() {
    let too_short = "a".repeat(63);
    let too_long = "a".repeat(65);
    let not_hex = "g".repeat(64);
    let upper = "A".repeat(64);
    for bad in ["", "abc", &too_short, &too_long, &not_hex, &upper] {
        assert_eq!(
            bad.parse::<GraphHash>(),
            Err(RecordError::GraphHash(bad.to_owned()))
        );
    }
    assert!("0123456789abcdef".repeat(4).parse::<GraphHash>().is_ok());
}
