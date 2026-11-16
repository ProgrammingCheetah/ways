use super::super::tests::*;
use super::*;

fn entry(ticket: u64, since: u64, priority: Option<u8>) -> LaneEntry {
    LaneEntry {
        ticket: id(ticket),
        next: name("implement"),
        since: time(since),
        size: TaskSize::S,
        weight: weight(time(since), priority.and_then(Priority::new), now()),
    }
}

fn order(lane: &Lane) -> Vec<TicketId> {
    lane.entries().iter().map(|entry| entry.ticket).collect()
}

#[test]
fn weight_is_age_plus_the_priority_worth() {
    assert_eq!(
        weight(time(NOW - 600), None, now()).age(),
        Duration::from_secs(600)
    );
    assert_eq!(
        weight(time(NOW - 600), Priority::new(2), now()).age(),
        Duration::from_secs(600) + 2 * PRIORITY_AGE
    );
    assert_eq!(weight(time(NOW + 5), None, now()), Weight::default());
}

#[test]
fn a_priority_is_a_level_from_one_to_three() {
    for level in 1..=3 {
        let priority = Priority::new(level).expect("a level");
        assert_eq!(priority.level(), level);
        assert_eq!(priority.age(), PRIORITY_AGE * u32::from(4 - level));
        assert_eq!(level.to_string().parse::<Priority>(), Ok(priority));
        assert_eq!(priority.label(), format!("priority/{level}"));
    }
    assert_eq!(Priority::new(0), None);
    assert_eq!(Priority::new(4), None);
    let refused = "high".parse::<Priority>().expect_err("not a level");
    assert_eq!(refused.to_string(), "`high` is not a priority from 1 to 3");
}

#[test]
fn priority_one_is_the_most_urgent() {
    let at = |level: u8| weight(time(NOW - 60), Priority::new(level), now());
    assert!(at(1) > at(2) && at(2) > at(3), "1 is worth the most");
    assert!(
        at(3) > weight(time(NOW - 60), None, now()),
        "3 still counts"
    );
    assert_eq!(Priority::new(1).map(Priority::age), Some(3 * PRIORITY_AGE));
    assert_eq!(Priority::new(3).map(Priority::age), Some(PRIORITY_AGE));
}

#[test]
fn priority_is_worth_a_fixed_age_and_age_wins_in_the_end() {
    // The younger ticket carries priority 1, worth three hours of age.
    let urgent = |at: u64| weight(time(at - 60), Priority::new(1), time(at));
    let patient = |at: u64| weight(time(at - 60 - 3 * 3600 - 1), None, time(at));
    assert!(urgent(NOW) < patient(NOW));
    let level_with = |at: u64| weight(time(at - 60 - 3 * 3600 + 1), None, time(at));
    assert!(urgent(NOW) > level_with(NOW));
}

#[test]
fn a_ticket_s_labels_give_its_priority() {
    let read = |labels: &[&str]| Priority::from_labels(labels.iter().copied());
    assert_eq!(read(&["task", "ready"]), Ok(None), "no priority label");
    assert_eq!(read(&["task", "priority/1"]), Ok(Priority::new(1)));
    assert_eq!(read(&["priority/3", "group:core"]), Ok(Priority::new(3)));
    assert_eq!(
        read(&["priority/9"]),
        Err(PriorityIgnored::Unknown("priority/9".to_owned()))
    );
    assert_eq!(
        read(&["priority/high"]).map_err(|ignored| ignored.to_string()),
        Err("the label `priority/high` is not priority/1, priority/2 or priority/3".to_owned())
    );
    let doubled = read(&["priority/1", "task", "priority/2"]);
    assert_eq!(
        doubled,
        Err(PriorityIgnored::Doubled(vec![
            "priority/1".to_owned(),
            "priority/2".to_owned()
        ]))
    );
    assert_eq!(
        doubled.map_err(|ignored| ignored.to_string()),
        Err("it carries more than one priority label (priority/1, priority/2)".to_owned())
    );
    assert_eq!(read(&["priority"]), Ok(None), "no slash, no priority label");
}

#[test]
fn the_lane_is_heaviest_first_and_ties_go_to_the_lower_ticket() {
    let mut lane = Lane::default();
    assert!(lane.is_empty());
    for entry in [
        entry(7, NOW - 100, None),
        entry(5, NOW - 100, None),
        entry(9, NOW - 50, Some(1)),
        entry(3, NOW - 10, None),
    ] {
        assert!(lane.enqueue(entry));
    }
    assert_eq!(order(&lane), [id(9), id(5), id(7), id(3)]);
    assert_eq!(lane.front().map(|entry| entry.ticket), Some(id(9)));
    assert_eq!(lane.len(), 4);
}

#[test]
fn a_ticket_already_in_the_lane_is_not_placed_again() {
    let mut lane = Lane::default();
    assert!(lane.enqueue(entry(5, NOW - 100, None)));
    assert!(!lane.enqueue(entry(5, NOW - 1000, Some(3))));
    assert_eq!(lane.len(), 1);
    assert!(lane.contains(id(5)));
    assert!(!lane.contains(id(6)));
}
