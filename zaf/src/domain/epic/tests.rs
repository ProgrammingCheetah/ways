use std::time::UNIX_EPOCH;

use super::*;

const BODY: &str = "Goal: one ticket shape over every backend.\n";

fn task(n: u64, state: TicketState, standing: Standing) -> EpicTask {
    EpicTask {
        number: TicketId::from(n),
        state: Some(state),
        standing,
        group: None,
    }
}

fn epic() -> Epic {
    Epic {
        id: TicketId::from(1),
        title: "tickets".into(),
        body: BODY.into(),
        tasks: Vec::new(),
        signature: None,
        challenged: false,
    }
}

fn signed(epic: Epic) -> Epic {
    Epic {
        signature: Some(Signature::of(&epic.body, UNIX_EPOCH)),
        ..epic
    }
}

#[test]
fn a_body_hash_is_sha256_in_hex() {
    let hash = BodyHash::of(&"".into());
    assert_eq!(
        hash.as_str(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn line_endings_and_trailing_space_do_not_move_the_hash() {
    let plain = BodyHash::of(&"Goal: g\nSize: s".into());
    assert_eq!(BodyHash::of(&"Goal: g\r\nSize: s\r\n\n  ".into()), plain);
    assert_ne!(BodyHash::of(&"Goal: g\nSize: m".into()), plain);
}

#[test]
fn a_hash_reads_back_from_its_hex_only() {
    let hash = BodyHash::of(&BODY.into());
    assert_eq!(hash.as_str().parse::<BodyHash>(), Ok(hash.clone()));
    assert_eq!(hash.as_str().to_uppercase().parse::<BodyHash>(), Ok(hash));
    assert!("abc".parse::<BodyHash>().is_err());
    assert!("z".repeat(64).parse::<BodyHash>().is_err());
}

#[test]
fn an_epic_walks_its_stages_in_order() {
    let mut epic = epic();
    assert_eq!(epic.stage(), EpicStage::Unsigned);
    epic = signed(epic);
    assert_eq!(epic.stage(), EpicStage::Signed);
    epic.tasks = vec![
        task(2, TicketState::Ready, Standing::Open),
        task(3, TicketState::Blocked, Standing::Open),
    ];
    assert_eq!(epic.stage(), EpicStage::Planned);
    epic.challenged = true;
    assert_eq!(epic.stage(), EpicStage::Challenged);
    epic.tasks = vec![
        task(2, TicketState::Done, Standing::Done),
        task(3, TicketState::Ready, Standing::ClosedNotPlanned),
    ];
    assert_eq!(epic.stage(), EpicStage::Done);
    assert_eq!(epic.tally(), (1, 1), "a dropped task counts for nothing");
}

#[test]
fn an_edit_voids_the_signature() {
    let mut epic = signed(epic());
    assert!(epic.signed());
    epic.body = format!("{BODY}Also: Projects.").into();
    assert!(!epic.signed());
    assert!(epic.signature_void());
    assert_eq!(epic.stage(), EpicStage::Unsigned);
}

#[test]
fn a_stage_prints_as_its_word() {
    assert_eq!(EpicStage::Challenged.to_string(), "challenged");
    assert_eq!(
        serde_json::to_value(EpicStage::Unsigned).expect("serializes"),
        "unsigned"
    );
}
