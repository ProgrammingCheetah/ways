use clap::Parser;

use super::*;
use crate::domain::{Instance, Ticket, TicketId, TicketState};
use crate::fakes::FakeBoard;
use crate::ports::{ClaimOutcome, ClaimRecord, NewQuestion, PortError};

const MANIFEST: &str =
    "[project]\nname = \"demo\"\n[board]\nkind = \"github-issues\"\nrepo = \"owner/demo\"\n";

fn project() -> (tempfile::TempDir, Composition) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("ZAF.toml"), MANIFEST).expect("manifest");
    let composition = Composition::load_with_home(dir.path(), None).expect("loads");
    (dir, composition)
}

fn parsed(args: &[&str]) -> (Cli, ReportArgs) {
    let cli =
        Cli::try_parse_from(std::iter::once("zaf").chain(args.iter().copied())).expect("parses");
    let crate::cli::Verb::Report(report_args) = &cli.verb else {
        panic!("not a report");
    };
    let report_args = ReportArgs {
        all: report_args.all,
        hours: report_args.hours,
        matching: report_args.matching.clone(),
        top: report_args.top,
        epics: report_args.epics,
    };
    (cli, report_args)
}

#[test]
fn the_flags_default_to_a_week_and_twenty_five_rows() {
    let (_, args) = parsed(&["report"]);
    assert_eq!((args.all, args.hours, args.top), (false, 168.0, 25));
    assert_eq!(args.matching, None);
    let (_, args) = parsed(&[
        "report", "--all", "--hours", "24", "--match", "export", "--top", "3",
    ]);
    assert_eq!((args.all, args.hours, args.top), (true, 24.0, 3));
    assert_eq!(args.matching.as_deref(), Some("export"));
}

#[test]
fn a_project_without_transcripts_reports_no_runs_and_exits_ok() {
    let (_dir, composition) = project();
    let (cli, args) = parsed(&["report"]);
    assert_eq!(print_report(&cli, &composition, &args, None), OK);
}

#[test]
fn a_malformed_price_table_exits_backend() {
    let (dir, composition) = project();
    std::fs::create_dir_all(dir.path().join(".zaf")).expect("state dir");
    std::fs::write(
        dir.path().join(".zaf/local.toml"),
        "[report.prices]\nx = [1]\n",
    )
    .expect("local.toml");
    let (cli, args) = parsed(&["report"]);
    assert_eq!(print_report(&cli, &composition, &args, None), BACKEND);
}

#[test]
fn report_without_epics_is_unchanged() {
    let (_dir, composition) = project();
    let (cli, args) = parsed(&["report"]);
    assert!(!args.epics, "--epics is opt-in");
    assert_eq!(print_report(&cli, &composition, &args, None), OK);
    let (_, args) = parsed(&["report", "--epics"]);
    assert!(args.epics);
}

fn down() -> PortError {
    PortError::Backend("gh is down".to_owned())
}

/// A board whose every call fails as a backend does.
struct Down;

impl Board for Down {
    fn ready(&self) -> Result<Vec<Ticket>, PortError> {
        Err(down())
    }
    fn task(&self, _: TicketId) -> Result<Ticket, PortError> {
        Err(down())
    }
    fn claim(&self, _: TicketId, _: &Instance) -> Result<ClaimOutcome, PortError> {
        Err(down())
    }
    fn heartbeat(&self, _: TicketId, _: &Instance) -> Result<(), PortError> {
        Err(down())
    }
    fn claims(&self) -> Result<Vec<ClaimRecord>, PortError> {
        Err(down())
    }
    fn release(&self, _: TicketId, _: &Instance) -> Result<(), PortError> {
        Err(down())
    }
    fn set_state(&self, _: TicketId, _: TicketState) -> Result<(), PortError> {
        Err(down())
    }
    fn comment(&self, _: TicketId, _: &str) -> Result<(), PortError> {
        Err(down())
    }
    fn report(&self, _: TicketId, _: &Instance, _: &str) -> Result<(), PortError> {
        Err(down())
    }
    fn questions(&self) -> Result<Vec<Ticket>, PortError> {
        Err(down())
    }
    fn open_question(&self, _: &NewQuestion) -> Result<TicketId, PortError> {
        Err(down())
    }
    fn record_ruling(&self, _: TicketId, _: &str) -> Result<Option<TicketId>, PortError> {
        Err(down())
    }
}

fn ledger_row(tasks: Vec<u64>) -> LedgerRow {
    LedgerRow {
        run_id: "s".to_owned(),
        parent_run: None,
        project: "demo".to_owned(),
        role: Some("team".to_owned()),
        epic: Some(103),
        tasks,
        model: "claude-opus-5-5".to_owned(),
        first_time: None,
        last_time: None,
        turns: 3,
        input: 1,
        cache_write: 1,
        cache_read: 1,
        output: 1,
        context_first: 1,
        context_last: 1,
        first_prompt: String::new(),
        path: "/gone/s.jsonl".to_owned(),
        size: 1,
        modified_ns: 1,
    }
}

#[test]
fn epics_board_failure_exits_2() {
    let (cli, _) = parsed(&["report", "--epics"]);
    let rows = [ledger_row(vec![96])];
    assert_eq!(
        print_epics(&cli, &rows, &Down, &PriceTable::default()),
        BACKEND
    );
}

#[test]
fn epics_prints_and_exits_ok_when_the_board_answers() {
    let board = FakeBoard::default();
    let (cli, _) = parsed(&["report", "--epics"]);
    let rows = [ledger_row(vec![])];
    assert_eq!(print_epics(&cli, &rows, &board, &PriceTable::default()), OK);
}

#[test]
fn encoded_root_counts_as_this_project() {
    let root = Path::new("/home/zuri/code/ai/ways");
    let with_project = |project: &str| {
        let mut row = ledger_row(vec![]);
        row.project = project.to_owned();
        row
    };
    let own = with_project("-home-zuri-code-ai-ways");
    let named = with_project("demo");
    let rows = vec![
        own.clone(),
        named.clone(),
        with_project("-home-zuri-code-other"),
        with_project("-home-zuri-code-ai-ways-extra"),
    ];

    let kept = project_rows(rows.clone(), false, "demo", root);
    assert_eq!(kept, [own, named], "the encoded root and the name only");
    assert_eq!(
        project_rows(rows, true, "demo", root).len(),
        4,
        "--all keeps all"
    );
    assert_eq!(
        ledger::encoded_root(Path::new("/home/zuri/my code/ways.rs")),
        "-home-zuri-my-code-ways-rs"
    );
}

fn screen(colour: bool) -> crate::cli::screen::Screen {
    crate::cli::screen::Screen {
        colour,
        live: false,
        rows: 24,
        columns: 80,
    }
}

/// A view of two epics, the second with nothing done, and one run outside any epic.
fn two_epics() -> epics::EpicView {
    let row = |epic: u64, tasks_done: usize| epics::EpicRow {
        epic,
        tasks_done,
        est_usd: 12.5,
        usd_per_task: (tasks_done > 0).then_some(4.0),
        tokens: epics::Tokens {
            input: 1_000_000,
            cache_write: 2_000_000,
            cache_read: 3_000_000,
            output: 4_000_000,
        },
        turns: 40,
        turns_per_task: None,
        context_first: Some(30_000.0),
        context_last: Some(90_000.0),
        roles: vec![epics::RoleShare {
            role: "team".to_owned(),
            est_usd: 12.5,
            share_percent: 100.0,
        }],
        median_usd: epics::SizeMedians::default(),
        done_by_size: epics::SizeCounts::default(),
        versus_last: epics::SizeVerdicts::default(),
    };
    epics::EpicView {
        rows: vec![row(96, 3), row(103, 0)],
        outside: Some(epics::Outside {
            runs: 1,
            turns: 2,
            est_usd: 1.5,
            tokens: epics::Tokens::default(),
        }),
    }
}

#[test]
fn the_coloured_report_stripped_is_the_plain_report() {
    use crate::cli::screen::strip_escapes;
    let view = two_epics();
    let coloured = screen(true).draw(&view.lines());
    assert!(coloured.contains('\x1b'), "the colour draw paints");
    assert_eq!(strip_escapes(&coloured), screen(false).draw(&view.lines()));
    assert_eq!(strip_escapes(&coloured), view.render_text());
    assert!(!view.render_text().contains('\x1b'));
}

#[test]
fn a_heading_is_bold() {
    use crate::cli::screen::code;
    use crate::domain::screen::Meaning;
    let view = two_epics();
    let drawn = screen(true).draw(&view.lines());
    let bold = code(Meaning::Bold);
    let heading = drawn
        .lines()
        .find(|line| line.contains("S vs last"))
        .expect("the heading line");
    assert!(
        heading.starts_with(&format!("\x1b[{bold}mepic")),
        "{heading:?}"
    );
    assert!(drawn.contains(&format!("\x1b[{}m#96\x1b[0m", code(Meaning::Noun))));
    assert!(drawn.contains(&format!("\x1b[{}m3\x1b[0m", code(Meaning::Done))));

    let report = report::report(&Query {
        roots: vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app/report/fixtures/projects")],
        hours: 100_000.0,
        matching: String::new(),
        top: 25,
        prices: PriceTable::default(),
        now: SystemTime::now(),
    });
    let drawn = screen(true).draw(&report.lines());
    assert_eq!(
        drawn.matches(&format!("\x1b[{bold}m")).count(),
        2,
        "two headings"
    );
}
