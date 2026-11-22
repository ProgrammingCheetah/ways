use ratatui::layout::Rect;

use super::{Axis, Direction, Layout, PaneId, Refusal};

/// The panes' area of an 80×24 terminal: below the tab bar and its rule, above the status line
/// and the key bar.
const SMALL: Rect = Rect {
    x: 0,
    y: 2,
    width: 80,
    height: 20,
};

/// The panes' area of a 120×32 terminal.
const LARGE: Rect = Rect {
    x: 0,
    y: 2,
    width: 120,
    height: 28,
};

fn rect(x: u16, y: u16, width: u16, height: u16) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn area_of(layout: &Layout, pane: PaneId, area: Rect) -> Rect {
    layout
        .panes(area)
        .into_iter()
        .find(|(shown, _)| *shown == pane)
        .map(|(_, rect)| rect)
        .expect("shown")
}

#[test]
fn the_chair_splits_as_the_mock_does_at_80_by_24() {
    let layout = Layout::chair();

    assert_eq!(
        layout.panes(SMALL),
        [
            (PaneId::Inbox, rect(0, 2, 27, 10)),
            (PaneId::Problems, rect(0, 13, 27, 3)),
            (PaneId::Now, rect(0, 17, 27, 5)),
            (PaneId::Epics, rect(28, 2, 52, 8)),
            (PaneId::Detail, rect(28, 11, 30, 11)),
            (PaneId::Live, rect(59, 11, 21, 11)),
        ]
    );
    assert!(layout.fits(SMALL));
}

#[test]
fn borders_join_where_they_meet() {
    let layout = Layout::chair();
    let borders = layout.borders(SMALL);
    let at = |x: u16, y: u16| {
        borders
            .iter()
            .find(|(bx, by, _)| (*bx, *by) == (x, y))
            .map(|(_, _, glyph)| *glyph)
    };

    assert_eq!(at(0, 1), Some('─'), "the rule under the tabs");
    assert_eq!(at(27, 1), Some('┬'), "the left column meets the rule");
    assert_eq!(at(27, 5), Some('│'));
    assert_eq!(
        at(27, 12),
        Some('┤'),
        "the inbox's border meets the column's"
    );
    assert_eq!(
        at(27, 10),
        Some('├'),
        "the epics' border meets the column's"
    );
    assert_eq!(
        at(58, 10),
        Some('┬'),
        "detail and live part under the epics"
    );
    assert_eq!(at(58, 21), Some('│'), "the last pane row");
    assert_eq!(at(27, 22), None, "nothing is drawn below the panes");
    assert_eq!(at(10, 12), Some('─'));
    assert_eq!(at(10, 5), None, "inside a pane");
}

#[test]
fn focus_moves_to_the_nearest_pane_that_way() {
    let layout = Layout::chair();
    let toward = |from, direction| layout.toward(from, direction, SMALL);

    assert_eq!(toward(PaneId::Inbox, Direction::Right), Some(PaneId::Epics));
    assert_eq!(
        toward(PaneId::Inbox, Direction::Down),
        Some(PaneId::Problems)
    );
    assert_eq!(toward(PaneId::Inbox, Direction::Up), None);
    assert_eq!(toward(PaneId::Inbox, Direction::Left), None);
    assert_eq!(toward(PaneId::Now, Direction::Right), Some(PaneId::Detail));
    assert_eq!(toward(PaneId::Live, Direction::Left), Some(PaneId::Detail));
    assert_eq!(toward(PaneId::Live, Direction::Up), Some(PaneId::Epics));
    // Problems and Now are as near the middle of Detail: the first in the tree wins, as in vim.
    assert_eq!(
        toward(PaneId::Detail, Direction::Left),
        Some(PaneId::Problems)
    );
    assert_eq!(toward(PaneId::Epics, Direction::Down), Some(PaneId::Detail));
}

#[test]
fn tab_visits_every_shown_pane_in_order() {
    let mut layout = Layout::chair();

    assert_eq!(layout.next(PaneId::Inbox, SMALL), PaneId::Problems);
    assert_eq!(layout.next(PaneId::Live, SMALL), PaneId::Inbox);

    layout.toggle_maximised(PaneId::Epics);
    assert_eq!(
        layout.next(PaneId::Epics, SMALL),
        PaneId::Epics,
        "only one shows"
    );
}

#[test]
fn resizing_moves_the_nearest_split_of_its_kind() {
    let mut layout = Layout::chair();
    let before = area_of(&layout, PaneId::Epics, LARGE);

    layout
        .resize(PaneId::Epics, Axis::Rows, 5, LARGE)
        .expect("taller");
    let taller = area_of(&layout, PaneId::Epics, LARGE);
    assert!(taller.height > before.height, "{before:?} → {taller:?}");

    layout
        .resize(PaneId::Detail, Axis::Rows, 5, LARGE)
        .expect("detail is below the epics");
    assert_eq!(area_of(&layout, PaneId::Epics, LARGE).height, before.height);

    let narrow = area_of(&layout, PaneId::Inbox, LARGE);
    layout
        .resize(PaneId::Epics, Axis::Columns, 5, LARGE)
        .expect("wider");
    assert!(area_of(&layout, PaneId::Inbox, LARGE).width < narrow.width);

    layout.reset();
    assert_eq!(
        layout,
        Layout::chair(),
        "= gives every split its first share"
    );
}

#[test]
fn a_resize_that_would_crush_a_pane_is_refused_and_changes_nothing() {
    let mut layout = Layout::chair();

    assert_eq!(
        layout.resize(PaneId::Problems, Axis::Rows, -5, SMALL),
        Err(Refusal::TooSmall)
    );
    assert_eq!(layout, Layout::chair());
    for _ in 0..20 {
        let _ = layout.resize(PaneId::Inbox, Axis::Columns, -5, LARGE);
    }
    assert!(layout.fits(LARGE));
    assert!(area_of(&layout, PaneId::Inbox, LARGE).width >= 16);
    assert_eq!(
        Layout::chair().resize(PaneId::Inbox, Axis::Columns, 500, LARGE),
        Err(Refusal::TooSmall),
        "a share past 90% is held at 90%, which leaves the right column too narrow"
    );
}

#[test]
fn a_maximised_pane_takes_the_area_and_gives_it_back() {
    let mut layout = Layout::chair();

    layout.toggle_maximised(PaneId::Detail);

    assert_eq!(layout.maximised(), Some(PaneId::Detail));
    assert_eq!(layout.panes(SMALL), [(PaneId::Detail, SMALL)]);
    assert!(
        layout.borders(SMALL).iter().all(|(_, y, _)| *y == 1),
        "only the rule"
    );
    assert_eq!(
        layout.resize(PaneId::Detail, Axis::Rows, 5, SMALL),
        Err(Refusal::Maximised)
    );

    layout.toggle_maximised(PaneId::Inbox);
    assert_eq!(layout, Layout::chair());
}

#[test]
fn the_pane_under_a_point_is_found() {
    let layout = Layout::chair();

    assert_eq!(layout.pane_at(SMALL, 3, 4), Some(PaneId::Inbox));
    assert_eq!(layout.pane_at(SMALL, 70, 15), Some(PaneId::Live));
    assert_eq!(layout.pane_at(SMALL, 27, 4), None, "a border");
    assert_eq!(layout.pane_at(SMALL, 3, 0), None, "the tab bar");
}

#[test]
fn every_resize_keeps_the_least_size_at_80_by_24() {
    for pane in PaneId::ORDER {
        for axis in [Axis::Rows, Axis::Columns] {
            for delta in [5, -5] {
                let mut layout = Layout::chair();
                for _ in 0..30 {
                    let _ = layout.resize(pane, axis, delta, SMALL);
                }
                assert!(layout.fits(SMALL), "{pane:?} {axis:?} {delta}");
            }
        }
    }
}

#[test]
fn the_phase_pane_takes_the_place_of_detail_and_live_and_gives_it_back() {
    let mut layout = Layout::chair();
    layout
        .resize(PaneId::Detail, Axis::Columns, 10, LARGE)
        .expect("Detail widens");
    let before = layout.clone();
    let detail_and_live = |layout: &Layout| {
        let panes = layout.panes(LARGE);
        let find = |id| panes.iter().find(|(pane, _)| *pane == id).map(|(_, r)| *r);
        let (detail, live) = (find(PaneId::Detail).expect("Detail"), find(PaneId::Live));
        detail.union(live.expect("Live"))
    };
    let area = detail_and_live(&layout);

    layout.open_phase();

    assert!(layout.phase_open());
    let panes = layout.panes(LARGE);
    assert_eq!(
        panes.iter().map(|(pane, _)| *pane).collect::<Vec<_>>(),
        [
            PaneId::Inbox,
            PaneId::Problems,
            PaneId::Now,
            PaneId::Epics,
            PaneId::Phase
        ]
    );
    assert_eq!(panes[4].1, area, "the phase pane has both their room");
    assert!(layout.fits(SMALL));
    assert_eq!(layout.next(PaneId::Epics, LARGE), PaneId::Phase);
    assert_eq!(
        layout.toward(PaneId::Phase, Direction::Up, LARGE),
        Some(PaneId::Epics)
    );
    layout.open_phase();
    assert!(layout.phase_open(), "opening twice changes nothing");

    layout.close_phase();

    assert_eq!(layout, before, "Detail and Live come back as they were");
    layout.close_phase();
    assert_eq!(layout, before);
}

#[test]
fn a_maximised_detail_gives_way_to_the_phase_pane() {
    let mut layout = Layout::chair();
    layout.toggle_maximised(PaneId::Live);
    layout.open_phase();
    assert_eq!(layout.maximised(), Some(PaneId::Phase));
    layout.close_phase();
    assert_eq!(layout.maximised(), None);
}
