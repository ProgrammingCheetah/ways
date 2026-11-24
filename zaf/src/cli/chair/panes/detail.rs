//! Detail: the selected thing in full. A question with Claude's proposed answer and the tickets
//! it holds, a problem and its fix, and the answer box (D6), here; the epic views and the tree
//! (F6) and the why view (F7) in `detail/`, each its own file.

pub mod epic;
pub mod graph;
pub mod why;

use ratatui::text::{Line, Span};

use super::{AnswerBox, Context, Cursor, DetailView, Effect, Hint, Pane, Title, View, hint};
use crate::app::chair::{ChairState, InboxItem, ProblemItem, TroubleKind};
use crate::app::hub::next::NextKind;
use crate::cli::chair::keys::Key;
use crate::cli::chair::layout::PaneId;
use crate::cli::chair::text::{width, wrap};
use crate::domain::printable;
use crate::domain::screen::Meaning;

/// The Detail pane.
pub struct Detail;

/// Lines laid out to a width: what each view of Detail writes into.
pub struct Page<'v, 'a> {
    view: &'v View<'a>,
    width: usize,
    lines: Vec<Line<'static>>,
}

impl<'v, 'a> Page<'v, 'a> {
    /// An empty page `width` cells wide.
    pub fn new(view: &'v View<'a>, width: usize) -> Self {
        Self {
            view,
            width,
            lines: Vec::new(),
        }
    }

    /// The view it is drawn for.
    pub fn view(&self) -> &View<'a> {
        self.view
    }

    /// The cells a line holds.
    pub fn width(&self) -> usize {
        self.width
    }

    /// `text` wrapped to the width, painted as `meaning` (unpainted for none).
    pub fn text(&mut self, meaning: Option<Meaning>, text: &str) {
        for line in wrap(text, self.width) {
            self.lines.push(Line::from(match meaning {
                Some(meaning) => self.view.paint(meaning, line),
                None => View::plain(line),
            }));
        }
    }

    /// A line as it is.
    pub fn line(&mut self, line: Line<'static>) {
        self.lines.push(line);
    }

    /// An empty line.
    pub fn blank(&mut self) {
        self.lines.push(Line::default());
    }

    /// A button: the key reversed, then what it does in bold.
    pub fn button(&mut self, key: &str, what: &str) {
        let key = self.view.paint(Meaning::Verb, format!(" {key} "));
        self.lines.push(Line::from(vec![
            Span::styled(key.content, key.style.patch(self.view.theme.reverse())),
            View::plain(" "),
            Span::styled(what.to_owned(), self.view.theme.bold()),
        ]));
    }

    /// The lines written.
    pub fn lines(self) -> Vec<Line<'static>> {
        self.lines
    }
}

/// Where Esc in Detail goes back to.
fn origin(detail: DetailView) -> PaneId {
    match detail {
        DetailView::Item | DetailView::Answer => PaneId::Inbox,
        DetailView::Problem => PaneId::Problems,
        DetailView::Epic | DetailView::Read | DetailView::Graph | DetailView::Why(_) => {
            PaneId::Epics
        }
    }
}

fn item_title(item: &InboxItem) -> String {
    match (item.kind, &item.question, item.number) {
        (NextKind::Door, Some(question), Some(number)) => question.raised_on.map_or_else(
            || format!("Door {number}"),
            |on| format!("Door {number} on {on}"),
        ),
        (_, Some(question), Some(number)) => question.raised_on.map_or_else(
            || format!("Question {number}"),
            |on| format!("Question {number} on {on}"),
        ),
        (NextKind::Sign, _, Some(number)) => format!("Sign epic {number}"),
        (NextKind::Phase, _, _) => item
            .title
            .split_once(':')
            .map_or(item.title.clone(), |(release, _)| release.to_owned()),
        (NextKind::Unticked, _, _) => "Ready work".to_owned(),
        (NextKind::Broken, _, _) => "The project cannot run".to_owned(),
        _ => item.title.clone(),
    }
}

fn item_page(page: &mut Page<'_, '_>, item: &InboxItem) {
    if let Some(question) = &item.question {
        page.text(None, &question.text);
        page.blank();
        if let Some(proposal) = &question.proposal {
            page.text(Some(Meaning::Dim), "Claude's answer:");
            page.text(Some(Meaning::Waiting), &collapse_blank_lines(proposal));
            page.blank();
        }
        if !question.holds.is_empty() {
            let held: Vec<String> = question.holds.iter().map(ToString::to_string).collect();
            page.text(
                Some(Meaning::Dim),
                &format!("Holds {} tickets: {}", held.len(), held.join(" ")),
            );
            page.blank();
        }
        if question.door {
            page.text(
                Some(Meaning::Waiting),
                "A door, a one-way decision: read it twice.",
            );
            page.blank();
        }
        page.button("Enter", "answer");
        return;
    }
    page.text(None, &item.title);
    page.blank();
    page.text(Some(Meaning::Dim), &item.how);
    if let Some(command) = &item.command {
        page.blank();
        page.text(Some(Meaning::Verb), command);
    }
    page.blank();
    match item.kind {
        NextKind::Phase => page.button("o", "start the phase"),
        NextKind::Sign => page.button("s", "sign the epic"),
        NextKind::Unticked => page.button("b", "start the build"),
        NextKind::Broken => page.button("Enter", "see the problems"),
        NextKind::Question | NextKind::Door | NextKind::Failed => {}
    }
}

fn problem_title(problem: &ProblemItem) -> String {
    match problem.ticket {
        Some(ticket) => format!("Problem {ticket}"),
        None => "Problem".to_owned(),
    }
}

fn problem_page(page: &mut Page<'_, '_>, problem: &ProblemItem) {
    page.text(None, &problem.detail);
    page.blank();
    match (problem.kind, &problem.fix) {
        (TroubleKind::Orphaned, _) => page.text(
            Some(Meaning::Dim),
            "The next tick frees it; nothing to do here.",
        ),
        (TroubleKind::FailedPhase, _) => page.button("Enter", "answer its question"),
        (TroubleKind::Project(_), Some(fix)) => {
            page.text(Some(Meaning::Dim), "Fix:");
            page.text(Some(Meaning::Verb), fix);
            page.blank();
            if problem.verb.is_some() {
                page.button("Enter", "run the fix");
            } else {
                page.text(Some(Meaning::Dim), "Run it by hand.");
            }
        }
        (TroubleKind::Project(_), None) => {}
        (TroubleKind::Task(_), _) => page.text(
            Some(Meaning::Dim),
            "Fix it on the board; nothing here runs a fix for it.",
        ),
    }
}

/// The heading over the answer box.
const ANSWER_HEADING: &str = "Your ruling (Claude's answer, yours to change)";
/// The line under the box.
const ANSWER_KEYS: &str = "Enter rules it · Esc cancels";
/// About the most of Claude's proposal the answer box starts with, in characters.
pub const PREFILL_CAP: usize = 400;

/// What the answer box starts with for `proposal`: its first paragraph only, its lines' trailing
/// spaces gone, cut at a space near [`PREFILL_CAP`] characters and ended with `…` when it is
/// longer. What follows a blank line is never ruled unseen, and nor is a character that draws
/// nothing: the proposal is made printable first, whoever read it.
pub fn prefill(proposal: &str) -> String {
    let collapsed = printable(&collapse_blank_lines(proposal));
    let first = collapsed
        .lines()
        .map(str::trim_end)
        .skip_while(|line| line.trim().is_empty())
        .take_while(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if first.chars().count() <= PREFILL_CAP {
        return first;
    }
    let cut: String = first.chars().take(PREFILL_CAP).collect();
    let cut = match cut.rfind(char::is_whitespace) {
        Some(at) if at > cut.len() / 2 => &cut[..at],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end())
}

/// `text` with each run of blank lines made one blank line, `\r\n` and `\r` read as `\n`, and no
/// blank line at either end: twenty empty lines never push what follows out of sight.
pub fn collapse_blank_lines(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut kept: Vec<&str> = Vec::new();
    for line in text.split('\n') {
        let blank = line.trim().is_empty();
        if blank && kept.last().is_none_or(|last| last.trim().is_empty()) {
            continue;
        }
        kept.push(if blank { "" } else { line });
    }
    while kept.last().is_some_and(|last| last.is_empty()) {
        kept.pop();
    }
    kept.join("\n")
}

/// The answer box as Detail lays it out in a pane `width` columns wide with `rows` rows below its
/// title: the ruling wrapped to the box with the cursor ending it, and the lines of it in view.
/// The box scrolls to the cursor, `back` lines up from it when the Engineer scrolls; what does not
/// fit is said in its borders, and Enter rules only once [`hidden`](Self::hidden) is 0. A Detail
/// not laid out (another pane maximised, the phase pane over it) shows none of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerView {
    /// The ruling wrapped to the box, the cursor `▏` ending the last line.
    pub lines: Vec<String>,
    /// The first line in view.
    pub top: usize,
    /// How many lines are in view.
    pub shown: usize,
    /// The cells inside the box's borders.
    pub inner: usize,
}

impl AnswerView {
    /// `answer` in a Detail `width` by `rows` (below its title).
    pub fn of(answer: &AnswerBox, width: u16, rows: u16) -> Self {
        let page = usize::from(width).saturating_sub(2).max(1);
        let inner = page.saturating_sub(2).max(1);
        let lines = wrap(&format!("{}▏", answer.text), inner);
        let chrome = wrap(ANSWER_HEADING, page).len() + 2 + wrap(ANSWER_KEYS, page).len();
        let room = if width == 0 {
            0
        } else {
            usize::from(rows).saturating_sub(chrome)
        };
        let shown = lines.len().min(room);
        let top = (lines.len() - shown).saturating_sub(answer.back);
        Self {
            lines,
            top,
            shown,
            inner,
        }
    }

    /// The lines above the view.
    pub fn above(&self) -> usize {
        self.top
    }

    /// The lines below it.
    pub fn below(&self) -> usize {
        self.lines.len() - self.top - self.shown
    }

    /// The lines out of view: Enter is refused while any is.
    pub fn hidden(&self) -> usize {
        self.lines.len() - self.shown
    }
}

/// `lines more lines` as a border says it, `arrow` first.
fn more_lines(arrow: char, lines: usize) -> String {
    let plural = if lines == 1 { "" } else { "s" };
    format!(" {arrow} {lines} more line{plural} ")
}

/// A border of the box: `left`, the rule with `label` in it when it fits, `right`.
fn border(left: char, label: Option<String>, inner: usize, right: char) -> String {
    match label.filter(|label| width(label) < inner) {
        Some(label) => {
            let rest = inner - 1 - width(&label);
            format!("{left}─{label}{}{right}", "─".repeat(rest))
        }
        None => format!("{left}{}{right}", "─".repeat(inner)),
    }
}

fn answer_page(page: &mut Page<'_, '_>, state: &ChairState, columns: u16, rows: u16) {
    let Some(answer) = page.view().answer else {
        return;
    };
    let answer = answer.clone();
    let view = AnswerView::of(&answer, columns, rows);
    page.text(Some(Meaning::Bold), ANSWER_HEADING);
    let inner = view.inner;
    let dim = |page: &Page<'_, '_>, text: String| page.view().paint(Meaning::Dim, text);
    let above = (view.above() > 0).then(|| more_lines('↑', view.above()));
    let top = dim(page, border('┌', above, inner, '┐'));
    page.line(Line::from(top));
    for line in view.lines.iter().skip(view.top).take(view.shown) {
        let gap = inner.saturating_sub(width(line));
        let left = dim(page, "│".to_owned());
        let right = dim(page, "│".to_owned());
        page.line(Line::from(vec![
            left,
            View::plain(line.clone()),
            View::plain(" ".repeat(gap)),
            right,
        ]));
    }
    let below = (view.below() > 0).then(|| more_lines('↓', view.below()));
    let bottom = dim(page, border('└', below, inner, '┘'));
    page.line(Line::from(bottom));
    page.text(Some(Meaning::Dim), ANSWER_KEYS);
    page.blank();
    if let Some(question) = state
        .question(answer.question)
        .and_then(|item| item.question.as_ref())
    {
        page.text(Some(Meaning::Dim), &question.text);
    }
}

impl Pane for Detail {
    fn title(&self, view: &View<'_>) -> Title {
        let Some(state) = view.state else {
            return Title::new("Detail");
        };
        let cursor = view.cursor;
        let text = match cursor.detail {
            DetailView::Item => super::inbox::selected(state, cursor)
                .map_or_else(|| "All clear".to_owned(), item_title),
            DetailView::Problem => state
                .problems
                .get(cursor.problem)
                .map_or_else(|| "No problems".to_owned(), problem_title),
            DetailView::Epic => epic::title(state, cursor),
            DetailView::Read => epic::read_title(state, cursor),
            DetailView::Graph => graph::title(state, cursor),
            DetailView::Why(ticket) => why::title(ticket),
            DetailView::Answer => view.answer.map_or_else(
                || "Answer".to_owned(),
                |answer| format!("Answer {}", answer.question),
            ),
        };
        Title::new(text)
    }

    fn body(&self, view: &View<'_>, width: u16, rows: u16) -> Vec<Line<'static>> {
        let Some(state) = view.state else {
            return vec![view.reading()];
        };
        let cursor = view.cursor;
        let mut page = Page::new(view, usize::from(width).saturating_sub(2).max(1));
        match cursor.detail {
            DetailView::Item => match super::inbox::selected(state, cursor) {
                Some(item) => item_page(&mut page, item),
                None => page.text(
                    Some(Meaning::Done),
                    "✓ Nothing waits on you. The build goes on.",
                ),
            },
            DetailView::Problem => match state.problems.get(cursor.problem) {
                Some(problem) => problem_page(&mut page, problem),
                None => page.text(Some(Meaning::Done), "✓ Nothing is wrong."),
            },
            DetailView::Epic => epic::page(&mut page, state, cursor),
            DetailView::Read => epic::read_page(&mut page, state, cursor),
            DetailView::Graph => graph::page(&mut page, state, cursor),
            DetailView::Why(ticket) => why::page(&mut page, state, ticket),
            DetailView::Answer => answer_page(&mut page, state, width, rows),
        }
        let lines = page.lines();
        let scroll = if cursor.detail == DetailView::Answer {
            0
        } else {
            usize::from(cursor.scroll).min(lines.len().saturating_sub(1))
        };
        lines
            .into_iter()
            .skip(scroll)
            .take(usize::from(rows))
            .map(|line| {
                let mut spans = vec![View::plain(" ")];
                spans.extend(line.spans);
                Line::from(spans)
            })
            .collect()
    }

    fn key(&self, key: Key, context: Context<'_>, cursor: &mut Cursor) -> Effect {
        match key {
            Key::Char('j') | Key::Down => {
                cursor.scroll = cursor.scroll.saturating_add(1);
                Effect::Done
            }
            Key::Char('k') | Key::Up => {
                cursor.scroll = cursor.scroll.saturating_sub(1);
                Effect::Done
            }
            Key::Char(' ') => {
                let page = context.rows.saturating_sub(2).max(1);
                cursor.scroll = cursor.scroll.saturating_add(page);
                Effect::Done
            }
            Key::Char('g') => {
                cursor.scroll = 0;
                Effect::Done
            }
            Key::Char('s') if cursor.detail == DetailView::Item => context
                .state
                .and_then(|state| super::inbox::selected(state, cursor))
                .and_then(super::inbox::signs)
                .map_or(Effect::Ignored, Effect::Sign),
            Key::Esc => {
                cursor.scroll = 0;
                let back = origin(cursor.detail);
                if back == PaneId::Epics {
                    cursor.detail = DetailView::Epic;
                }
                Effect::Focus(back)
            }
            Key::Enter => {
                let Some(state) = context.state else {
                    return Effect::Done;
                };
                match cursor.detail {
                    DetailView::Item => super::inbox::selected(state, cursor)
                        .map_or(Effect::Done, super::inbox::open),
                    DetailView::Problem => state
                        .problems
                        .get(cursor.problem)
                        .map_or(Effect::Done, |problem| {
                            super::problems::open(state, problem)
                        }),
                    _ => Effect::Ignored,
                }
            }
            _ => Effect::Ignored,
        }
    }

    fn hints(&self, view: &View<'_>) -> Vec<Hint> {
        match view.cursor.detail {
            DetailView::Epic | DetailView::Read => vec![
                hint("j/k", "scroll"),
                hint("Space", "page"),
                hint("s", "sign"),
                hint("d", "graph"),
                hint("w", "why"),
                hint("Esc", "back"),
                hint("q", "quit"),
            ],
            DetailView::Graph | DetailView::Why(_) => vec![
                hint("j/k", "scroll"),
                hint("d", "graph"),
                hint("w", "why"),
                hint("l", "live"),
                hint("Esc", "back"),
                hint("q", "quit"),
            ],
            _ => vec![
                hint("j/k", "scroll"),
                hint("Esc", "back"),
                hint("Tab", "next pane"),
                hint("q", "quit"),
            ],
        }
    }
}

#[cfg(test)]
mod tests;
