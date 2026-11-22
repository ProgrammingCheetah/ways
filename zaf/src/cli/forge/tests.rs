use super::*;

use crate::cli::screen::{code, strip_escapes};
use crate::domain::screen::Meaning;
use crate::ports::{CheckBucket, CheckLink, CheckName, CheckRun};

const COLOUR: Screen = Screen {
    colour: true,
    live: true,
    rows: 24,
    columns: 80,
};
const PLAIN: Screen = Screen {
    colour: false,
    live: false,
    rows: 24,
    columns: 80,
};

fn run(bucket: CheckBucket, name: &str, link: Option<&str>) -> CheckRun {
    CheckRun {
        bucket,
        name: CheckName::from(name),
        link: link.map(CheckLink::from),
    }
}

fn runs() -> Vec<CheckRun> {
    vec![
        run(CheckBucket::Pass, "ci / test", Some("http://forge.test/1")),
        run(CheckBucket::Fail, "ci / lint", None),
        run(CheckBucket::Pending, "external", None),
    ]
}

fn drawn(screen: &Screen, runs: &[CheckRun]) -> String {
    let lines: Vec<_> = runs.iter().map(forge::check_line).collect();
    screen.draw(&lines)
}

fn painted(meaning: Meaning, text: &str) -> String {
    format!("\x1b[{}m{}\x1b[0m", code(meaning), text)
}

#[test]
fn the_coloured_checks_stripped_are_the_plain_checks() {
    let runs = runs();
    let plain = drawn(&PLAIN, &runs);
    assert!(!plain.contains('\x1b'));
    assert_eq!(
        plain,
        "pass\tci / test\thttp://forge.test/1\nfail\tci / lint\t\npending\texternal\t\n"
    );
    let coloured = drawn(&COLOUR, &runs);
    assert!(coloured.contains('\x1b'));
    assert_eq!(strip_escapes(&coloured), plain);
}

#[test]
fn a_run_takes_its_meaning() {
    let coloured = drawn(&COLOUR, &runs());
    let mut lines = coloured.lines();
    assert!(
        lines
            .next()
            .unwrap()
            .starts_with(&painted(Meaning::Done, "pass"))
    );
    assert!(
        lines
            .next()
            .unwrap()
            .starts_with(&painted(Meaning::Failed, "fail"))
    );
    assert!(
        lines
            .next()
            .unwrap()
            .starts_with(&painted(Meaning::Waiting, "pending"))
    );
    assert!(coloured.contains(&painted(Meaning::Noun, "ci / test")));
}

#[test]
fn a_run_name_cannot_move_the_terminal() {
    let runs = [run(CheckBucket::Pass, "a\x1b[2Jb", None)];
    let coloured = drawn(&COLOUR, &runs);
    assert_eq!(strip_escapes(&coloured), "pass\ta [2Jb\t\n");
    assert!(!drawn(&PLAIN, &runs).contains('\x1b'));
}
