use super::*;

use crate::cli::screen::{Environment, code, strip_escapes};

fn screen(colour: bool) -> Screen {
    Screen::from_environment(Environment {
        stdout_is_terminal: colour,
        no_color: None,
        term: Some("xterm"),
    })
}

fn two_bad_fields() -> Vec<TaskProblem> {
    vec![
        TaskProblem::Bad {
            field: crate::domain::TaskField::Tests,
            value: "x\u{1b}]52;c;AAAA\u{7}y".to_owned(),
            accepts: "names".to_owned(),
        },
        TaskProblem::Missing(crate::domain::TaskField::Goal),
    ]
}

#[test]
fn the_coloured_lint_stripped_is_the_plain_lint() {
    let problems = two_bad_fields();
    let coloured = screen(true).draw(&lint_lines(&problems));
    let plain = screen(false).draw(&lint_lines(&problems));
    assert!(coloured.contains('\x1b'));
    assert_eq!(strip_escapes(&coloured), plain);
    assert!(!plain.contains('\x1b'));
    assert_eq!(plain.lines().count(), 2);
}

#[test]
fn a_lint_problem_is_red() {
    let problems = two_bad_fields();
    let coloured = screen(true).draw(&lint_lines(&problems));
    let red = format!("\x1b[{}m", code(Meaning::Failed));
    let yellow = format!("\x1b[{}m", code(Meaning::Noun));
    for line in coloured.lines() {
        assert!(line.starts_with(&yellow), "{line}");
        assert!(line.contains(&red), "{line}");
    }
    assert_eq!(coloured.matches(&red).count(), 2);
}

#[test]
fn a_body_that_parses_prints_nothing_in_either_screen() {
    let none: Vec<TaskProblem> = Vec::new();
    assert_eq!(screen(true).draw(&lint_lines(&none)), "");
    assert_eq!(screen(false).draw(&lint_lines(&none)), "");
}

#[test]
fn the_plain_lint_is_the_problems_text_one_to_a_line() {
    let problems = two_bad_fields();
    let plain = screen(false).draw(&lint_lines(&problems));
    assert_eq!(
        plain,
        "Tests: x ]52;c;AAAA y (accepts: names)\nGoal: missing\n"
    );
}
