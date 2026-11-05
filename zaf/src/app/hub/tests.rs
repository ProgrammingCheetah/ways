use std::time::Duration;

use super::*;

fn ticket(number: u64, phase: Phase) -> TicketRow {
    TicketRow {
        number,
        who: "sonnet programmer".to_owned(),
        phase,
        word: "testing".to_owned(),
        since: Some(Duration::from_secs(40)),
    }
}

fn epic() -> EpicRow {
    EpicRow {
        number: 103,
        done: 17,
        total: 82,
        dollars: 39.83,
        slots_free: 1,
        slots_total: 2,
        ready: 63,
        questions: 0,
        tickets: vec![ticket(143, Phase::Build)],
        groups: vec![GroupRow {
            name: "hub".to_owned(),
            done: 3,
            total: 9,
            tickets: vec![ticket(250, Phase::Check)],
        }],
    }
}

fn frame_of(epic: EpicRow, width: usize) -> String {
    draw(&Snapshot { epics: vec![epic] }, width)
}

#[test]
fn frame_shows_epic_group_and_ticket_rows() {
    let frame = frame_of(epic(), 100);
    let lines: Vec<&str> = frame.lines().collect();
    assert!(lines[0].starts_with("epic #103"), "{frame}");
    assert!(lines[0].contains("17 of 82"), "{frame}");
    assert!(lines[0].contains("$39.83"), "{frame}");
    assert!(lines[0].contains('['), "a bar: {frame}");
    assert!(lines[1].contains("1 of 2 slots free"), "{frame}");
    assert!(lines[1].contains("63 ready"), "{frame}");
    assert!(lines[1].contains("no questions"), "{frame}");
    let ticket = lines
        .iter()
        .find(|line| line.contains("#143"))
        .expect("ticket");
    assert!(ticket.contains("sonnet programmer"), "{frame}");
    assert!(ticket.contains("build"), "{frame}");
    assert!(ticket.contains("testing"), "{frame}");
    assert!(ticket.ends_with("40s"), "{frame}");
    let group = lines
        .iter()
        .find(|line| line.starts_with("group: hub"))
        .expect("group");
    assert!(group.contains("3 of 9"), "{frame}");
    assert!(group.contains('['), "{frame}");
    let grouped = lines
        .iter()
        .position(|line| line.contains("#250"))
        .expect("grouped ticket");
    let at = lines
        .iter()
        .position(|line| line.starts_with("group: hub"))
        .expect("group");
    assert!(
        grouped > at,
        "a grouped ticket sits under its group: {frame}"
    );
}

fn filled(line: &str) -> usize {
    let open = line.find('[').expect("bar");
    let close = line.find(']').expect("bar");
    line[open + 1..close].chars().filter(|c| *c == '#').count()
}

fn bar_cells(line: &str) -> usize {
    let open = line.find('[').expect("bar");
    let close = line.find(']').expect("bar");
    line[open + 1..close].chars().count()
}

#[test]
fn ticket_bar_follows_its_phase() {
    let mut steps = Vec::new();
    for phase in Phase::DECLARABLE {
        let mut row = epic();
        row.groups.clear();
        row.tickets = vec![ticket(7, phase)];
        let frame = frame_of(row, 100);
        let line = frame
            .lines()
            .find(|line| line.contains("#7 "))
            .expect("ticket");
        steps.push((filled(line), bar_cells(line)));
    }
    let fills: Vec<usize> = steps.iter().map(|(fill, _)| *fill).collect();
    assert!(
        fills.windows(2).all(|pair| pair[0] < pair[1]),
        "each later phase fills more: {fills:?}"
    );
    let cells = steps[0].1;
    assert!(steps.iter().all(|(_, width)| *width == cells));
    assert_eq!(
        *fills.last().unwrap(),
        cells,
        "the last phase fills the bar"
    );
    let mut unclaimed = epic();
    unclaimed.groups.clear();
    unclaimed.tickets = vec![ticket(7, Phase::Unclaimed)];
    let frame = frame_of(unclaimed, 100);
    let line = frame
        .lines()
        .find(|line| line.contains("#7 "))
        .expect("ticket");
    assert_eq!(filled(line), 0);
}

#[test]
fn no_line_passes_the_width() {
    let mut long = epic();
    long.dollars = 123456.78;
    long.tickets[0].who = "a very long model name and a role".to_owned();
    long.tickets[0].word = "wordwordwordwordword".to_owned();
    for width in [0, 1, 10, 20, 40, 60, 80, 100, 200] {
        let frame = frame_of(long.clone(), width);
        for line in frame.lines() {
            assert!(
                line.chars().count() <= width,
                "width {width}, line {line:?}"
            );
        }
    }
}

#[test]
fn nothing_running_still_shows_slots() {
    let mut idle = epic();
    idle.tickets.clear();
    idle.groups.clear();
    idle.slots_free = 4;
    idle.slots_total = 4;
    let frame = frame_of(idle, 100);
    assert!(frame.contains("4 of 4 slots free"), "{frame}");
    assert!(!frame.contains("group:"), "{frame}");
    assert!(!frame.contains("  #"), "no ticket row: {frame}");
}

#[test]
fn a_frame_with_no_epic_says_so() {
    assert_eq!(draw(&Snapshot::default(), 80), "no open epics\n");
}
