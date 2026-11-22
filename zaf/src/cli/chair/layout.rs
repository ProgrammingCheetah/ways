//! The chair's panes, split both ways like vim's windows (D4). Pure: rectangles in, rectangles
//! out, nothing drawn.
//!
//! [`Layout`] is a tree whose leaves are panes and whose inner nodes split their area in two:
//! side by side ([`Axis::Columns`]) or stacked ([`Axis::Rows`]), the first child taking `ratio`
//! percent of what is left after the one-cell border between them. The chair's tree is the
//! mock's: a left column (Inbox over Problems over Now) beside a right column (Epics over Detail
//! beside Live). Focus moves to the nearest pane in a direction, resizing moves the nearest split
//! of the right kind by 5% while every pane keeps 3 rows and 16 columns, and one pane can take the
//! whole area and give it back. While a release phase runs, its pane takes the place of the split
//! of Detail and Live, which comes back as it was when the phase pane closes.

use std::collections::BTreeMap;

use ratatui::layout::Rect;

/// The least rows a pane keeps, its title included.
pub const MIN_ROWS: u16 = 3;
/// The least columns a pane keeps.
pub const MIN_COLUMNS: u16 = 16;
/// How far one resize moves a split, in percent.
pub const STEP: i16 = 5;

/// A pane of the chair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaneId {
    /// What waits on the Engineer.
    Inbox,
    /// What is wrong.
    Problems,
    /// What runs now.
    Now,
    /// The open epics and their squares.
    Epics,
    /// The selected thing in full, and the answer box.
    Detail,
    /// A run, followed live.
    Live,
    /// A release phase's Claude session, over Detail and Live while it runs.
    Phase,
}

impl PaneId {
    /// The panes in the order `Tab` visits them.
    pub const ORDER: [Self; 7] = [
        Self::Inbox,
        Self::Problems,
        Self::Now,
        Self::Epics,
        Self::Phase,
        Self::Detail,
        Self::Live,
    ];

    /// Its name, as the tab bar shows a maximised pane.
    pub fn name(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Problems => "problems",
            Self::Now => "now",
            Self::Epics => "epics",
            Self::Detail => "detail",
            Self::Live => "live",
            Self::Phase => "claude",
        }
    }
}

/// How a split lays out its two children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Side by side, a border column between: `<` and `>` move it.
    Columns,
    /// Stacked, a border row between: `+` and `-` move it.
    Rows,
}

/// A direction focus moves in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `h`.
    Left,
    /// `j`.
    Down,
    /// `k`.
    Up,
    /// `l`.
    Right,
}

/// Why a resize was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No split of that kind holds the pane.
    NoSplit,
    /// A pane would get smaller than 3 rows or 16 columns.
    TooSmall,
    /// One pane takes the whole area.
    Maximised,
}

impl Refusal {
    /// The refusal in plain words, for the status line.
    pub fn words(self) -> &'static str {
        match self {
            Self::NoSplit => "no split to resize that way",
            Self::TooSmall => "that pane cannot get smaller",
            Self::Maximised => "Ctrl-w o gives the other panes back first",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Leaf(PaneId),
    Split {
        axis: Axis,
        ratio: u16,
        default: u16,
        first: Box<Node>,
        second: Box<Node>,
    },
}

impl Node {
    fn split(axis: Axis, ratio: u16, first: Self, second: Self) -> Self {
        Self::Split {
            axis,
            ratio,
            default: ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    fn holds(&self, pane: PaneId) -> bool {
        match self {
            Self::Leaf(leaf) => *leaf == pane,
            Self::Split { first, second, .. } => first.holds(pane) || second.holds(pane),
        }
    }

    /// The smallest node that holds both `one` and `other`.
    fn holding_both(&mut self, one: PaneId, other: PaneId) -> Option<&mut Self> {
        if !(self.holds(one) && self.holds(other)) {
            return None;
        }
        let both = |node: &Self| node.holds(one) && node.holds(other);
        let side = match self {
            Self::Split { first, .. } if both(first) => Some(true),
            Self::Split { second, .. } if both(second) => Some(false),
            _ => None,
        };
        match (side, self) {
            (Some(true), Self::Split { first, .. }) => first.holding_both(one, other),
            (Some(false), Self::Split { second, .. }) => second.holding_both(one, other),
            (_, node) => Some(node),
        }
    }
}

/// Where a border cell joins: up 1, down 2, left 4, right 8.
type Joins = u8;

/// The glyph of a border cell from where it joins.
fn glyph(joins: Joins) -> char {
    match joins {
        1..=3 => '│',
        5 => '┘',
        6 => '┐',
        7 => '┤',
        9 => '└',
        10 => '┌',
        11 => '├',
        13 => '┴',
        14 => '┬',
        15 => '┼',
        _ => '─',
    }
}

/// The chair's panes and how they share the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    root: Node,
    maximised: Option<PaneId>,
    /// The split of Detail and Live while the phase pane stands in its place.
    held: Option<Box<Node>>,
}

impl Default for Layout {
    fn default() -> Self {
        Self::chair()
    }
}

impl Layout {
    /// The mock's layout: Inbox, Problems and Now in a column of 35%, beside Epics over Detail and
    /// Live.
    pub fn chair() -> Self {
        let left = Node::split(
            Axis::Rows,
            55,
            Node::Leaf(PaneId::Inbox),
            Node::split(
                Axis::Rows,
                40,
                Node::Leaf(PaneId::Problems),
                Node::Leaf(PaneId::Now),
            ),
        );
        let right = Node::split(
            Axis::Rows,
            45,
            Node::Leaf(PaneId::Epics),
            Node::split(
                Axis::Columns,
                60,
                Node::Leaf(PaneId::Detail),
                Node::Leaf(PaneId::Live),
            ),
        );
        Self {
            root: Node::split(Axis::Columns, 35, left, right),
            maximised: None,
            held: None,
        }
    }

    /// Lays the phase pane over Detail and Live: it takes the place of their split, which is kept
    /// to come back as it was. Nothing changes while it is already open.
    pub fn open_phase(&mut self) {
        if self.held.is_some() {
            return;
        }
        if let Some(node) = self.root.holding_both(PaneId::Detail, PaneId::Live) {
            let split = std::mem::replace(node, Node::Leaf(PaneId::Phase));
            self.held = Some(Box::new(split));
        }
        if matches!(self.maximised, Some(PaneId::Detail | PaneId::Live)) {
            self.maximised = Some(PaneId::Phase);
        }
    }

    /// Gives Detail and Live their place back; nothing changes while the phase pane is not open.
    pub fn close_phase(&mut self) {
        let Some(split) = self.held.take() else {
            return;
        };
        if let Some(node) = self.root.holding_both(PaneId::Phase, PaneId::Phase) {
            *node = *split;
        }
        if self.maximised == Some(PaneId::Phase) {
            self.maximised = None;
        }
    }

    /// Whether the phase pane is open.
    pub fn phase_open(&self) -> bool {
        self.held.is_some()
    }

    /// The pane that takes the whole area, when one does.
    pub fn maximised(&self) -> Option<PaneId> {
        self.maximised
    }

    /// The shown panes and their rectangles in `area`, in the tree's order.
    pub fn panes(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        let mut panes = Vec::new();
        match self.maximised {
            Some(pane) => panes.push((pane, area)),
            None => lay_out(&self.root, area, &mut panes, &mut BTreeMap::new()),
        }
        panes
    }

    /// The border cells as `(x, y, glyph)`: the rule along the row above `area` and the borders
    /// between panes, joined where they meet.
    pub fn borders(&self, area: Rect) -> Vec<(u16, u16, char)> {
        let mut joins = BTreeMap::new();
        if area.y > 0 {
            for x in area.left()..area.right() {
                joins.insert((x, area.y - 1), 12);
            }
        }
        if self.maximised.is_none() {
            let mut panes = Vec::new();
            lay_out(&self.root, area, &mut panes, &mut joins);
        }
        let top = area.y.saturating_sub(1);
        joins
            .into_iter()
            .filter(|((x, y), _)| {
                (top..area.bottom()).contains(y) && (area.left()..area.right()).contains(x)
            })
            .map(|((x, y), joins)| (x, y, glyph(joins)))
            .collect()
    }

    /// Whether every shown pane keeps its 3 rows and 16 columns in `area`.
    pub fn fits(&self, area: Rect) -> bool {
        self.panes(area)
            .iter()
            .all(|(_, rect)| rect.height >= MIN_ROWS && rect.width >= MIN_COLUMNS)
    }

    /// The pane at `(x, y)` in `area`, when one is there.
    pub fn pane_at(&self, area: Rect, x: u16, y: u16) -> Option<PaneId> {
        self.panes(area)
            .into_iter()
            .find(|(_, rect)| rect.contains((x, y).into()))
            .map(|(pane, _)| pane)
    }

    /// The pane `Tab` moves to from `from`: the next shown one, round again at the end.
    pub fn next(&self, from: PaneId, area: Rect) -> PaneId {
        let shown: Vec<PaneId> = PaneId::ORDER
            .into_iter()
            .filter(|pane| self.panes(area).iter().any(|(shown, _)| shown == pane))
            .collect();
        let at = shown.iter().position(|pane| *pane == from);
        at.map_or(from, |at| shown[(at + 1) % shown.len()])
    }

    /// The nearest pane from `from` in `direction`: of the panes wholly that way, the closest,
    /// then the one nearest the middle of `from`; none when no pane is that way.
    pub fn toward(&self, from: PaneId, direction: Direction, area: Rect) -> Option<PaneId> {
        let panes = self.panes(area);
        let (_, here) = panes.iter().find(|(pane, _)| *pane == from)?;
        let (top, left, height, width) = (
            i32::from(here.y),
            i32::from(here.x),
            i32::from(here.height),
            i32::from(here.width),
        );
        let middle_row = top + height / 2;
        let middle_column = left + width / 2;
        let gap = |near: i32, start: i32, length: i32| {
            if near < start {
                start - near
            } else if near > start + length - 1 {
                near - start - length + 1
            } else {
                0
            }
        };
        panes
            .iter()
            .filter(|(pane, _)| *pane != from)
            .filter_map(|(pane, rect)| {
                let (t, l, h, w) = (
                    i32::from(rect.y),
                    i32::from(rect.x),
                    i32::from(rect.height),
                    i32::from(rect.width),
                );
                let along = match direction {
                    Direction::Left if l + w <= left => left - l - w,
                    Direction::Right if l >= left + width => l - left - width,
                    Direction::Up if t + h <= top => top - t - h,
                    Direction::Down if t >= top + height => t - top - height,
                    _ => return None,
                };
                let across = match direction {
                    Direction::Left | Direction::Right => gap(middle_row, t, h),
                    Direction::Up | Direction::Down => gap(middle_column, l, w),
                };
                Some((along * 100 + across, *pane))
            })
            .min_by_key(|(score, _)| *score)
            .map(|(_, pane)| pane)
    }

    /// Moves the nearest split of `axis` that holds `focused` by `delta` percent, growing the
    /// focused pane's side for a positive `delta`. Refused, and nothing changed, when no such
    /// split holds it, when a pane would get too small in `area`, or while a pane is maximised.
    pub fn resize(
        &mut self,
        focused: PaneId,
        axis: Axis,
        delta: i16,
        area: Rect,
    ) -> Result<(), Refusal> {
        if self.maximised.is_some() {
            return Err(Refusal::Maximised);
        }
        let before = self.clone();
        if !nudge(&mut self.root, focused, axis, delta) {
            return Err(Refusal::NoSplit);
        }
        if self == &before {
            return Err(Refusal::TooSmall);
        }
        if !self.fits(area) {
            *self = before;
            return Err(Refusal::TooSmall);
        }
        Ok(())
    }

    /// Every split back to its first ratio.
    pub fn reset(&mut self) {
        fn reset(node: &mut Node) {
            if let Node::Split {
                ratio,
                default,
                first,
                second,
                ..
            } = node
            {
                *ratio = *default;
                reset(first);
                reset(second);
            }
        }
        reset(&mut self.root);
    }

    /// `pane` takes the whole area; none brings every pane back.
    pub fn set_maximised(&mut self, pane: Option<PaneId>) {
        self.maximised = pane;
    }

    /// `focused` takes the whole area, or, when a pane already does, every pane comes back.
    pub fn toggle_maximised(&mut self, focused: PaneId) {
        self.maximised = match self.maximised {
            Some(_) => None,
            None => Some(focused),
        };
    }
}

/// Lays `node` out in `area`, adding its panes to `panes` and its borders to `joins`.
fn lay_out(
    node: &Node,
    area: Rect,
    panes: &mut Vec<(PaneId, Rect)>,
    joins: &mut BTreeMap<(u16, u16), Joins>,
) {
    let mut mark = |x: u16, y: u16, joins_here: Joins| {
        *joins.entry((x, y)).or_insert(0) |= joins_here;
    };
    match node {
        Node::Leaf(pane) => panes.push((*pane, area)),
        Node::Split {
            axis: Axis::Columns,
            ratio,
            first,
            second,
            ..
        } => {
            let first_width = area.width.saturating_sub(1) * ratio / 100;
            let second_width = area.width.saturating_sub(1 + first_width);
            let border = area.x + first_width;
            for y in area.top()..area.bottom() {
                mark(border, y, 3);
            }
            if area.y > 0 {
                mark(border, area.y - 1, 2);
            }
            mark(border, area.bottom(), 1);
            lay_out(
                first,
                Rect {
                    width: first_width,
                    ..area
                },
                panes,
                joins,
            );
            lay_out(
                second,
                Rect {
                    x: border + 1,
                    width: second_width,
                    ..area
                },
                panes,
                joins,
            );
        }
        Node::Split {
            axis: Axis::Rows,
            ratio,
            first,
            second,
            ..
        } => {
            let first_height = area.height.saturating_sub(1) * ratio / 100;
            let second_height = area.height.saturating_sub(1 + first_height);
            let border = area.y + first_height;
            for x in area.left()..area.right() {
                mark(x, border, 12);
            }
            if area.x > 0 {
                mark(area.x - 1, border, 8);
            }
            mark(area.right(), border, 4);
            lay_out(
                first,
                Rect {
                    height: first_height,
                    ..area
                },
                panes,
                joins,
            );
            lay_out(
                second,
                Rect {
                    y: border + 1,
                    height: second_height,
                    ..area
                },
                panes,
                joins,
            );
        }
    }
}

/// Moves the split of `axis` nearest above `pane` by `delta`, bounded to 10–90%; false when no
/// split of `axis` holds the pane.
fn nudge(node: &mut Node, pane: PaneId, axis: Axis, delta: i16) -> bool {
    let Node::Split {
        axis: here,
        ratio,
        first,
        second,
        ..
    } = node
    else {
        return false;
    };
    let in_first = first.holds(pane);
    if !in_first && !second.holds(pane) {
        return false;
    }
    let child = if in_first { first } else { second };
    if nudge(child, pane, axis, delta) {
        return true;
    }
    if *here != axis {
        return false;
    }
    let change = if in_first { delta } else { -delta };
    let moved = i32::from(*ratio) + i32::from(change);
    *ratio = u16::try_from(moved.clamp(10, 90)).unwrap_or(50);
    true
}

#[cfg(test)]
mod tests;
