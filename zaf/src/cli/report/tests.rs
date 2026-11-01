use clap::Parser;

use super::*;

const MANIFEST: &str =
    "[project]\nname = \"demo\"\n[board]\nkind = \"github-issues\"\nrepo = \"owner/demo\"\n";

fn project() -> (tempfile::TempDir, Composition) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("ZAF.toml"), MANIFEST).expect("manifest");
    let composition = Composition::load(dir.path()).expect("loads");
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
