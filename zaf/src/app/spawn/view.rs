//! The live view of one version's build: the plain words for where an epic or a ticket stands,
//! and the frame that lays them out to fit the window.
//!
//! Pure: it reads the hub's [`ProjectState`] and hands back words with their [`Meaning`]. It
//! draws nothing and colours nothing; the CLI paints each meaning and redraws the frame.

use std::time::SystemTime;

use semver::Version;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::hub::LineState;
use crate::app::hub::state::{EpicState, LineTask, ProjectState};
use crate::domain::EpicStage;
use crate::domain::screen::{Line, Meaning, Text, clean};

/// The spinner's ten braille frames, picked by `spin % 10`.
pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// What the view says about stopping it.
const STOP_LINE: &str =
    "Ctrl-C stops it; work in progress is kept, and the same command picks it up.";
/// The columns an epic's title takes before its bar.
const EPIC_TITLE_WIDTH: usize = 34;
/// The columns a ticket's title takes before its stage.
const TICKET_TITLE_WIDTH: usize = 16;
/// The columns a phase's name takes before its sentence.
const PHASE_WIDTH: usize = 9;

/// The lines of one frame of the view of `version`'s build, cut to `size` (columns, rows).
///
/// The header, the stop line and a blank line come first, then each epic whose release is the
/// version: its title, its bar and count, and a line for each ticket. The other open epics the
/// tick also runs follow, every piece dim. Every line is cut to the width. When the frame is
/// taller than the window, an epic whose tickets are all merged takes one line, and what still
/// does not fit ends in "and n more", `n` counting the lines left out.
pub fn frame(
    state: &ProjectState,
    version: &Version,
    now: SystemTime,
    spin: u8,
    size: (u16, u16),
) -> Vec<Line> {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let spinner = SPINNER[usize::from(spin) % SPINNER.len()];
    let ours: Vec<&EpicState> = state
        .epics
        .iter()
        .filter(|epic| is_release(epic, version))
        .collect();
    let others: Vec<&EpicState> = state
        .epics
        .iter()
        .filter(|epic| !is_release(epic, version))
        .filter(|epic| !matches!(epic.stage, EpicStage::Unsigned | EpicStage::Done))
        .collect();

    let build = |collapse: bool| -> Vec<Line> {
        let mut lines = vec![
            header(state, &ours, version, now, spinner),
            vec![(Meaning::Dim, STOP_LINE.into())],
            Vec::new(),
        ];
        for epic in &ours {
            lines.extend(epic_lines(epic, spinner, collapse));
        }
        for epic in &others {
            lines.extend(epic_lines(epic, spinner, collapse).into_iter().map(dimmed));
        }
        lines
    };

    let mut lines = build(false);
    if lines.len() > height {
        lines = build(true);
    }
    if lines.len() > height {
        let shown = height.saturating_sub(1);
        let more = lines.len() - shown;
        lines.truncate(shown);
        if height > 0 {
            lines.push(vec![(Meaning::Dim, format!("and {more} more").into())]);
        }
    }
    lines.into_iter().map(|line| cut(line, width)).collect()
}

fn is_release(epic: &EpicState, version: &Version) -> bool {
    epic.release.as_deref().is_some_and(|release| {
        release.trim() == version.to_string()
            || Version::parse(release.trim()).is_ok_and(|parsed| parsed == *version)
    })
}

fn piece(meaning: Meaning, text: impl Into<String>) -> (Meaning, Text) {
    (meaning, Text::from(text.into()))
}

/// `text` padded with spaces to `width` display columns; longer text is left as it is.
fn padded(text: &str, width: usize) -> String {
    let missing = width.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(missing))
}

fn header(
    state: &ProjectState,
    ours: &[&EpicState],
    version: &Version,
    now: SystemTime,
    spinner: &str,
) -> Line {
    let tickets = ours.iter().flat_map(|epic| epic.tasks.iter());
    let total = tickets.clone().count();
    let merged = tickets.filter(|task| task.state == LineState::Done).count();
    let minutes = state
        .watch
        .as_ref()
        .and_then(|watch| watch.since.as_deref())
        .and_then(|since| humantime::parse_rfc3339_weak(since).ok())
        .and_then(|since| now.duration_since(since).ok())
        .map_or(0, |elapsed| elapsed.as_secs() / 60);
    vec![
        piece(Meaning::Program, "zaf"),
        piece(Meaning::Bold, " is building "),
        piece(Meaning::Noun, version.to_string()),
        piece(Meaning::Verb, format!("  {spinner}  ")),
        piece(Meaning::Bold, format!("{merged} of {total}")),
        piece(Meaning::Dim, " tickets merged  "),
        piece(Meaning::Dim, format!("{minutes} min")),
    ]
}

/// An epic's own line, then a line for each of its tickets unless it is collapsed and all merged.
fn epic_lines(epic: &EpicState, spinner: &str, collapse: bool) -> Vec<Line> {
    let total = epic.tasks.len();
    let merged = epic
        .tasks
        .iter()
        .filter(|task| task.state == LineState::Done)
        .count();
    let mut head = Vec::new();
    if total == 0 {
        head.push(piece(Meaning::Bold, format!("  {}", clean(&epic.title))));
        let mut lines = vec![head];
        if let Some(words) = epic_words(epic) {
            lines.push(vec![piece(Meaning::Verb, format!("    {words}"))]);
        }
        return lines;
    }
    head.push(piece(
        Meaning::Bold,
        format!("  {} ", padded(&clean(&epic.title), EPIC_TITLE_WIDTH)),
    ));
    head.push(piece(Meaning::Done, "■".repeat(merged)));
    head.push(piece(Meaning::Dim, "□".repeat(total - merged)));
    head.push(piece(Meaning::Dim, format!(" {merged}/{total}")));
    let mut lines = vec![head];
    if !(collapse && merged == total) {
        lines.extend(epic.tasks.iter().map(|task| ticket_line(task, spinner)));
    }
    lines
}

/// One ticket's line: its number and title, then its stage in words.
fn ticket_line(task: &LineTask, spinner: &str) -> Line {
    let words = stage_words(task);
    let name = format!(
        "    #{} {}",
        task.number,
        padded(&clean(&task.title), TICKET_TITLE_WIDTH)
    );
    match words.meaning {
        Meaning::Dim => vec![piece(Meaning::Dim, format!("{name} · {}", words.text))],
        Meaning::Verb => {
            let phase = task.phase.as_deref().unwrap_or_default();
            let mut line = vec![piece(Meaning::Noun, format!("{name} "))];
            if words.text.as_str() == phase {
                line.push(piece(Meaning::Verb, format!("{spinner} {phase}")));
            } else {
                line.push(piece(
                    Meaning::Verb,
                    format!("{spinner} {}", padded(phase, PHASE_WIDTH)),
                ));
                line.push(piece(Meaning::Dim, format!(" {}", words.text)));
            }
            line
        }
        meaning => vec![
            piece(Meaning::Noun, format!("{name} ")),
            (meaning, words.text),
        ],
    }
}

fn dimmed(line: Line) -> Line {
    line.into_iter()
        .map(|(_, text)| (Meaning::Dim, text))
        .collect()
}

/// `line` cut to `width` display columns, never inside a character: a character that
/// would cross the edge is left out.
fn cut(line: Line, width: usize) -> Line {
    let mut left = width;
    let mut cut = Vec::new();
    for (meaning, text) in line {
        if left == 0 {
            break;
        }
        let length = text.as_str().width();
        if length <= left {
            left -= length;
            cut.push((meaning, text));
        } else {
            let mut kept = String::new();
            for character in text.as_str().chars() {
                let columns = character.width().unwrap_or(0);
                if columns > left {
                    break;
                }
                left -= columns;
                kept.push(character);
            }
            cut.push(piece(meaning, kept));
            break;
        }
    }
    cut
}

/// What a ticket reads, with what it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketWords {
    /// The words.
    pub text: Text,
    /// What they mean: done, waiting on the Engineer, a dim wait, or a phase at work (`Verb`).
    pub meaning: Meaning,
}

impl TicketWords {
    fn new(text: impl Into<Text>, meaning: Meaning) -> Self {
        Self {
            text: text.into(),
            meaning,
        }
    }

    fn question() -> Self {
        Self::new("? question for you", Meaning::Waiting)
    }
}

/// The words of a ticket, as its line shows them.
///
/// A merged ticket reads "✓ merged" and one with a question open "? question for you", whatever
/// its phase. A ticket not started reads "waits on #n" for each open blocker in order, or
/// "queued" with none. Else its phase reads in plain words: a default phase by its sentence, a
/// project's own by its name.
pub fn stage_words(task: &LineTask) -> TicketWords {
    if task.state == LineState::Done {
        return TicketWords::new("✓ merged", Meaning::Done);
    }
    if task.state == LineState::Question || task.question.is_some() {
        return TicketWords::question();
    }
    match (task.state, task.phase.as_deref()) {
        (LineState::Working | LineState::WaitingTick, Some(phase)) => {
            TicketWords::new(phase_words(phase), Meaning::Verb)
        }
        _ => queue_words(task),
    }
}

/// The words of an epic before it has tickets, by its stage: signed, the planner's turn; planned,
/// the challenger's. None for any other stage.
pub fn epic_words(epic: &EpicState) -> Option<Text> {
    match epic.stage {
        EpicStage::Signed => Some("Claude plans the tickets".into()),
        EpicStage::Planned => Some("Claude checks the plan".into()),
        EpicStage::Unsigned | EpicStage::Challenged | EpicStage::Done => None,
    }
}

fn queue_words(task: &LineTask) -> TicketWords {
    let open: Vec<String> = task
        .blocked_by
        .iter()
        .filter(|blocker| blocker.state != LineState::Done)
        .map(|blocker| format!("#{}", blocker.number))
        .collect();
    if open.is_empty() {
        TicketWords::new("queued", Meaning::Dim)
    } else {
        TicketWords::new(format!("waits on {}", open.join(", ")), Meaning::Dim)
    }
}

fn phase_words(phase: &str) -> &str {
    match phase {
        "implement" => "Claude writes the code",
        "ci" => "your check command runs on its pull request",
        "review" => "Claude reviews the change",
        "merge" => "merging",
        own => own,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::hub::state::{LineBlocker, LineQuestion, WatchState};

    fn task(state: LineState, phase: Option<&str>) -> LineTask {
        LineTask {
            number: 30,
            title: "A ticket".into(),
            size: None,
            state,
            phase: phase.map(str::to_owned),
            phases_done: 0,
            phases_total: 4,
            blocked_by: Vec::new(),
            question: None,
            issue_url: None,
            waits: None,
            cost: None,
        }
    }

    fn blocker(number: u64, state: LineState) -> LineBlocker {
        LineBlocker {
            number,
            title: None,
            state,
        }
    }

    fn epic(stage: EpicStage) -> EpicState {
        EpicState {
            number: 7,
            title: "An epic".into(),
            stage,
            half_filed: false,
            release: None,
            url: None,
            done: 0,
            total: 0,
            dollars: 0.0,
            groups: Vec::new(),
            line_state: LineState::NotStarted,
            line_question: None,
            tasks: Vec::new(),
        }
    }

    #[test]
    fn default_phases_have_their_words() {
        for (phase, words) in [
            ("implement", "Claude writes the code"),
            ("ci", "your check command runs on its pull request"),
            ("review", "Claude reviews the change"),
            ("merge", "merging"),
        ] {
            let got = stage_words(&task(LineState::Working, Some(phase)));
            assert_eq!(got.text.as_str(), words);
            assert_eq!(got.meaning, Meaning::Verb);
        }
        let merged = stage_words(&task(LineState::Done, None));
        assert_eq!(merged.text.as_str(), "✓ merged");
        assert_eq!(merged.meaning, Meaning::Done);
    }

    #[test]
    fn own_phase_shows_its_name() {
        let got = stage_words(&task(LineState::Working, Some("deploy-staging")));
        assert_eq!(got.text.as_str(), "deploy-staging");
    }

    #[test]
    fn waits_lists_open_blockers() {
        let mut ticket = task(LineState::Blocked, Some("implement"));
        ticket.blocked_by = vec![
            blocker(21, LineState::Working),
            blocker(22, LineState::Done),
            blocker(23, LineState::NotStarted),
        ];
        let got = stage_words(&ticket);
        assert_eq!(got.text.as_str(), "waits on #21, #23");
        assert_eq!(got.meaning, Meaning::Dim);
    }

    #[test]
    fn not_started_without_blockers_is_queued() {
        let mut ticket = task(LineState::NotStarted, Some("implement"));
        assert_eq!(stage_words(&ticket).text.as_str(), "queued");
        ticket.blocked_by = vec![blocker(21, LineState::Done)];
        let got = stage_words(&ticket);
        assert_eq!(got.text.as_str(), "queued");
        assert_eq!(got.meaning, Meaning::Dim);
    }

    #[test]
    fn question_wins_over_phase() {
        for state in [
            LineState::Question,
            LineState::Working,
            LineState::WaitingTick,
            LineState::Blocked,
            LineState::NotStarted,
        ] {
            let mut ticket = task(state, Some("review"));
            ticket.question = Some(LineQuestion {
                number: 99,
                text: "Which one?".into(),
            });
            let got = stage_words(&ticket);
            assert_eq!(got.text.as_str(), "? question for you");
            assert_eq!(got.meaning, Meaning::Waiting);
        }
    }

    fn ticket(number: u64, title: &str, state: LineState, phase: Option<&str>) -> LineTask {
        LineTask {
            number,
            title: title.into(),
            ..task(state, phase)
        }
    }

    fn waiting_on(mut ticket: LineTask, blocker_number: u64, state: LineState) -> LineTask {
        ticket.blocked_by = vec![blocker(blocker_number, state)];
        ticket
    }

    fn epic_of(number: u64, title: &str, release: Option<&str>, tasks: Vec<LineTask>) -> EpicState {
        EpicState {
            number,
            title: title.into(),
            release: release.map(str::to_owned),
            done: tasks.iter().filter(|t| t.state == LineState::Done).count() as u32,
            total: tasks.len() as u32,
            tasks,
            ..epic(EpicStage::Challenged)
        }
    }

    fn project(epics: Vec<EpicState>, since: Option<&str>) -> ProjectState {
        ProjectState {
            project: "demo".into(),
            root: "/srv/demo".into(),
            missing: false,
            gathered_at: "2026-11-19T10:10:00Z".into(),
            reason: None,
            epics,
            queue: Vec::new(),
            questions: Vec::new(),
            problems: Vec::new(),
            instances: Vec::new(),
            watch: Some(WatchState {
                running: true,
                pid: None,
                since: since.map(str::to_owned),
                zaf: None,
                version: None,
                last_tick: None,
            }),
            last_tick: None,
            orphaned: Vec::new(),
            releases_due: Vec::new(),
            status: crate::app::hub::state::StatusState::default(),
        }
    }

    fn now() -> SystemTime {
        humantime::parse_rfc3339("2026-11-19T10:10:00Z").unwrap()
    }

    fn version() -> Version {
        Version::parse("0.1.0").unwrap()
    }

    fn plain(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|(_, text)| text.as_str()).collect())
            .collect()
    }

    /// The mock's two epics at step 10, ten minutes in: nine tickets, four merged.
    fn mock_state() -> ProjectState {
        use LineState::{Done, NotStarted, Working};
        let accounts = vec![
            ticket(21, "Sign-up form", Done, None),
            ticket(22, "Password rules", Done, None),
            ticket(23, "Sign-in", Done, None),
            waiting_on(ticket(24, "Sessions", Working, Some("implement")), 21, Done),
            ticket(26, "Reset password", Working, Some("implement")),
            waiting_on(
                ticket(25, "Sign-out", NotStarted, Some("implement")),
                24,
                Working,
            ),
        ];
        let profile = vec![
            ticket(27, "Profile model", Done, None),
            ticket(28, "Profile page", Working, Some("implement")),
            waiting_on(
                ticket(29, "Avatar upload", NotStarted, Some("implement")),
                28,
                Working,
            ),
        ];
        project(
            vec![
                epic_of(1, "Accounts: sign up and sign in", Some("0.1.0"), accounts),
                epic_of(2, "Profile page", Some("0.1.0"), profile),
            ],
            Some("2026-11-19T10:00:00Z"),
        )
    }

    #[test]
    fn frame_reads_as_the_mock() {
        let got = plain(&frame(&mock_state(), &version(), now(), 3, (100, 40)));
        let want = [
            "zaf is building 0.1.0  ⠸  4 of 9 tickets merged  10 min",
            "Ctrl-C stops it; work in progress is kept, and the same command picks it up.",
            "",
            "  Accounts: sign up and sign in      ■■■□□□ 3/6",
            "    #21 Sign-up form     ✓ merged",
            "    #22 Password rules   ✓ merged",
            "    #23 Sign-in          ✓ merged",
            "    #24 Sessions         ⠸ implement Claude writes the code",
            "    #26 Reset password   ⠸ implement Claude writes the code",
            "    #25 Sign-out         · waits on #24",
            "  Profile page                       ■□□ 1/3",
            "    #27 Profile model    ✓ merged",
            "    #28 Profile page     ⠸ implement Claude writes the code",
            "    #29 Avatar upload    · waits on #28",
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn frame_cuts_lines_to_width() {
        let lines = frame(&mock_state(), &version(), now(), 0, (30, 40));
        assert!(!lines.is_empty());
        for line in plain(&lines) {
            assert!(line.width() <= 30, "too long: {line:?}");
        }
        assert_eq!(plain(&lines)[3], "  Accounts: sign up and sign i");
    }

    #[test]
    fn an_ascii_frame_is_unchanged() {
        let got = plain(&frame(&mock_state(), &version(), now(), 3, (100, 40)));
        assert_eq!(got[3], "  Accounts: sign up and sign in      ■■■□□□ 3/6");
        assert_eq!(got[4], "    #21 Sign-up form     ✓ merged");
        let narrow = plain(&frame(&mock_state(), &version(), now(), 0, (30, 40)));
        assert_eq!(narrow[3], "  Accounts: sign up and sign i");
    }

    #[test]
    fn a_wide_title_is_cut_at_the_window_edge() {
        let mut state = mock_state();
        state.epics[0].title = "日本語のタイトルがとても長い🎉🎉🎉🎉🎉🎉🎉🎉🎉".into();
        state.epics[0].tasks[0].title = "漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字漢字".into();
        for width in 7..=41 {
            for line in plain(&frame(&state, &version(), now(), 0, (width, 40))) {
                assert!(
                    line.width() <= usize::from(width),
                    "{width}: too wide: {line:?}"
                );
            }
        }
        // The character that would cross the edge is dropped, not split.
        let lines = plain(&frame(&state, &version(), now(), 0, (7, 40)));
        assert_eq!(lines[3], "  日本");
    }

    #[test]
    fn frame_cuts_to_height_with_and_more() {
        use LineState::Done;
        let mut state = mock_state();
        state.epics = vec![
            epic_of(
                1,
                "Shipped",
                Some("0.1.0"),
                vec![ticket(1, "A", Done, None), ticket(2, "B", Done, None)],
            ),
            epic_of(
                2,
                "Also shipped",
                Some("0.1.0"),
                vec![ticket(3, "C", Done, None)],
            ),
            state.epics.remove(0),
            state.epics.remove(0),
        ];
        // Full frame: 3 + 3 + 2 + 7 + 4 = 19 lines; collapsed: 3 + 1 + 1 + 7 + 4 = 16.
        let got = plain(&frame(&state, &version(), now(), 0, (100, 10)));
        assert_eq!(got.len(), 10);
        assert_eq!(got[3], "  Shipped                            ■■ 2/2");
        assert_eq!(got[4], "  Also shipped                       ■ 1/1");
        assert_eq!(got[9], "and 7 more");
    }

    #[test]
    fn frame_cleans_control_characters_from_titles() {
        use LineState::NotStarted;
        let hostile = "A\x1b[2J\x1b]52;c;aGk=\x07B\u{9b}C";
        let mut state = mock_state();
        state.epics[0] = epic_of(
            1,
            hostile,
            Some("0.1.0"),
            vec![ticket(21, hostile, NotStarted, None)],
        );
        state.epics[1] = epic_of(2, hostile, Some("0.1.0"), Vec::new());
        let text = plain(&frame(&state, &version(), now(), 0, (100, 40))).join("\n");
        for bad in ['\x1b', '\x07', '\u{9b}'] {
            assert!(!text.contains(bad), "{text:?}");
        }
        assert!(text.contains("A [2J ]52;c;aGk= B C"), "{text:?}");
        assert_eq!(text.matches("A [2J ]52;c;aGk= B C").count(), 3, "{text:?}");
    }

    #[test]
    fn merged_epic_collapses_first() {
        use LineState::Done;
        let mut state = mock_state();
        state.epics[1] = epic_of(
            2,
            "Profile page",
            Some("0.1.0"),
            vec![ticket(27, "Profile model", Done, None)],
        );
        // 3 + 7 + 2 = 12 lines in full, 11 with the merged epic on one line.
        let tall = plain(&frame(&state, &version(), now(), 0, (100, 12)));
        assert_eq!(tall.len(), 12);
        assert_eq!(tall[11], "    #27 Profile model    ✓ merged");
        let short = plain(&frame(&state, &version(), now(), 0, (100, 11)));
        assert_eq!(short.len(), 11);
        assert_eq!(short[10], "  Profile page                       ■ 1/1");
        assert!(short.iter().all(|line| !line.starts_with("and ")));
    }

    #[test]
    fn other_epics_come_after_dimmed() {
        use LineState::NotStarted;
        let mut state = mock_state();
        let elsewhere = epic_of(
            8,
            "Other release",
            Some("0.2.0"),
            vec![ticket(80, "Later", NotStarted, Some("implement"))],
        );
        let closed = EpicState {
            stage: EpicStage::Done,
            ..epic_of(9, "Closed one", Some("0.0.9"), Vec::new())
        };
        state.epics.insert(0, elsewhere);
        state.epics.push(closed);
        let lines = frame(&state, &version(), now(), 0, (100, 40));
        let text = plain(&lines);
        assert_eq!(text.iter().filter(|l| l.contains("Closed one")).count(), 0);
        let at = text
            .iter()
            .position(|l| l.contains("Other release"))
            .unwrap();
        assert_eq!(at, 14, "after the version's epics");
        for line in &lines[at..] {
            assert!(line.iter().all(|(meaning, _)| *meaning == Meaning::Dim));
        }
        assert_eq!(text[at + 1], "    #80 Later            · queued");
        // The header still counts the version's tickets alone.
        assert!(text[0].contains("4 of 9 tickets merged"));
    }

    #[test]
    fn header_counts_version_tickets() {
        use LineState::Done;
        let mut state = mock_state();
        state.epics.push(epic_of(
            8,
            "Other release",
            Some("0.2.0"),
            vec![
                ticket(80, "One", Done, None),
                ticket(81, "Two", Done, None),
                ticket(82, "Three", Done, None),
            ],
        ));
        state.epics.push(epic_of(
            9,
            "No release",
            None,
            vec![ticket(90, "Four", Done, None)],
        ));
        let got = plain(&frame(&state, &version(), now(), 7, (100, 40)));
        assert_eq!(
            got[0],
            "zaf is building 0.1.0  ⠧  4 of 9 tickets merged  10 min"
        );
    }

    #[test]
    fn epic_without_tickets_reads_its_stage() {
        let words = |stage| epic_words(&epic(stage)).map(|text| text.as_str().to_owned());
        assert_eq!(
            words(EpicStage::Signed).as_deref(),
            Some("Claude plans the tickets")
        );
        assert_eq!(
            words(EpicStage::Planned).as_deref(),
            Some("Claude checks the plan")
        );
        assert_eq!(words(EpicStage::Challenged), None);
    }
}
