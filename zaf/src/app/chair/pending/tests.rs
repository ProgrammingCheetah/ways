use std::time::{Duration, Instant};

use super::{PendingQueue, UNDO_WINDOW};

#[test]
fn an_action_waits_its_window_then_is_due() {
    let start = Instant::now();
    let mut queue = PendingQueue::default();
    queue.push("rule #285", start);

    assert!(
        queue
            .take_due(start + UNDO_WINDOW - Duration::from_millis(1))
            .is_empty()
    );
    assert_eq!(
        queue.newest().map(|pending| pending.seconds_left(start)),
        Some(5)
    );
    assert_eq!(
        queue
            .newest()
            .map(|pending| pending.seconds_left(start + Duration::from_millis(4_100))),
        Some(1),
        "a countdown rounds up"
    );
    assert_eq!(queue.take_due(start + UNDO_WINDOW), ["rule #285"]);
    assert!(queue.is_empty());
}

#[test]
fn undo_takes_back_the_newest_and_it_is_never_due() {
    let start = Instant::now();
    let mut queue = PendingQueue::new(Duration::from_secs(5));
    queue.push(1, start);
    queue.push(2, start + Duration::from_secs(1));

    assert_eq!(queue.undo(), Some(2));
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.take_due(start + Duration::from_secs(60)), [1]);
    assert_eq!(queue.undo(), None, "nothing is left to take back");
}

#[test]
fn due_ones_come_out_oldest_first_and_the_rest_stay() {
    let start = Instant::now();
    let mut queue = PendingQueue::new(Duration::from_secs(5));
    queue.push('a', start);
    queue.push('b', start + Duration::from_secs(2));
    queue.push('c', start + Duration::from_secs(4));

    assert_eq!(queue.take_due(start + Duration::from_secs(7)), ['a', 'b']);
    assert_eq!(
        queue
            .iter()
            .map(|pending| pending.action)
            .collect::<Vec<_>>(),
        ['c']
    );
}

#[test]
fn quitting_drains_everything_at_once() {
    let start = Instant::now();
    let mut queue = PendingQueue::new(Duration::from_secs(5));
    queue.push("first", start);
    queue.push("second", start);

    assert_eq!(queue.drain(), ["first", "second"]);
    assert!(queue.is_empty());
    assert!(queue.newest().is_none());
}
