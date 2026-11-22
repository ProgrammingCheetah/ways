use super::*;

use crate::domain::screen::Text;

const ESCAPE: char = '\x1b';

const EVERY_MEANING: [Meaning; 8] = [
    Meaning::Program,
    Meaning::Verb,
    Meaning::Noun,
    Meaning::Done,
    Meaning::Failed,
    Meaning::Waiting,
    Meaning::Dim,
    Meaning::Bold,
];

fn piece(meaning: Meaning, text: &str) -> (Meaning, Text) {
    (meaning, Text::from(text))
}

#[test]
fn a_plain_draw_is_the_pieces_joined() {
    let lines: Vec<Line> = vec![
        vec![piece(Meaning::Verb, "write"), piece(Meaning::Noun, " a.md")],
        vec![piece(Meaning::Dim, "why")],
    ];
    let drawn = Screen {
        colour: false,
        live: false,
        rows: 24,
        columns: 80,
    }
    .draw(&lines);
    assert_eq!(drawn, "write a.md\nwhy\n");
    assert!(!drawn.contains(ESCAPE));
}

#[test]
fn a_coloured_draw_wraps_each_piece_in_its_code() {
    let lines: Vec<Line> = vec![vec![piece(Meaning::Failed, "no"), piece(Meaning::Dim, "x")]];
    let screen = Screen {
        colour: true,
        live: true,
        rows: 24,
        columns: 80,
    };
    assert_eq!(
        screen.draw(&lines),
        format!(
            "\x1b[{}mno\x1b[0m\x1b[{}mx\x1b[0m\n",
            code(Meaning::Failed),
            code(Meaning::Dim)
        )
    );
}

#[test]
fn a_coloured_draw_stripped_is_the_plain_draw() {
    let lines: Vec<Line> = vec![
        EVERY_MEANING.iter().map(|m| piece(*m, "word ")).collect(),
        vec![piece(Meaning::Verb, "write"), piece(Meaning::Noun, " a/b")],
    ];
    let on = Screen {
        colour: true,
        live: true,
        rows: 24,
        columns: 80,
    };
    let off = Screen {
        colour: false,
        ..on
    };
    let drawn = on.draw(&lines);
    assert!(drawn.contains(ESCAPE));
    assert_eq!(strip_escapes(&drawn), off.draw(&lines));
}

#[test]
fn strip_escapes_leaves_a_string_with_no_escape_unchanged() {
    for text in ["", "plain\n", "a [31m b", "tab\tand ünïcode\n"] {
        assert_eq!(strip_escapes(text), text);
    }
}

#[test]
fn strip_escapes_keeps_what_is_not_an_sgr_sequence() {
    assert_eq!(strip_escapes("\x1b[2Kx\x1b[1;31my\x1b[0m"), "\x1b[2Kxy");
    assert_eq!(strip_escapes("a\x1b[31"), "a\x1b[31");
}

fn coloured() -> Screen {
    Screen {
        colour: true,
        live: true,
        rows: 24,
        columns: 80,
    }
}

fn plain() -> Screen {
    Screen {
        colour: false,
        live: false,
        ..coloured()
    }
}

fn detect(terminal: bool, no_color: Option<&str>, term: Option<&str>) -> Screen {
    Screen::from_environment(Environment {
        stdout_is_terminal: terminal,
        no_color,
        term,
    })
}

fn asked(screen: &Screen, default: DefaultAnswer, typed: &str) -> (bool, String) {
    let mut output = Vec::new();
    let answer = ask_yes(
        screen,
        "Continue?",
        default,
        &mut typed.as_bytes(),
        &mut output,
    );
    (answer, String::from_utf8(output).expect("utf8"))
}

const BOTH: [DefaultAnswer; 2] = [DefaultAnswer::Yes, DefaultAnswer::No];

#[test]
fn ask_yes_empty_takes_the_default() {
    assert!(asked(&plain(), DefaultAnswer::Yes, "\n").0);
    assert!(!asked(&plain(), DefaultAnswer::No, "\n").0);
}

#[test]
fn ask_yes_closed_input_is_no() {
    for default in BOTH {
        assert!(!asked(&plain(), default, "").0);
    }
}

#[test]
fn ask_yes_n_prefix_is_no() {
    for default in BOTH {
        for no in ["n", "N", "no", "NO", "nope", "nah"] {
            assert!(!asked(&plain(), default, &format!("{no}\n")).0, "{no}");
        }
    }
}

#[test]
fn ask_yes_yes_words_are_yes() {
    for default in BOTH {
        for yes in ["y", "Y", "yes", "Yes"] {
            assert!(asked(&plain(), default, &format!("{yes}\n")).0, "{yes}");
        }
    }
}

#[test]
fn ask_yes_other_answers_ask_again() {
    for default in BOTH {
        for other in ["cancel", "q", "maybe"] {
            let (yes, shown) = asked(&plain(), default, &format!("{other}\ny\n"));
            assert!(yes, "{other}");
            assert_eq!(shown.matches("Continue?").count(), 2, "{other}");
            let (no, _) = asked(&plain(), default, &format!("{other}\nno\n"));
            assert!(!no, "{other}");
            // Closed after an unrecognised answer is no, never the default.
            assert!(
                !asked(&plain(), default, &format!("{other}\n")).0,
                "{other}"
            );
        }
    }
}

#[test]
fn plain_prompt_has_no_escape_codes() {
    let (_, yes) = asked(&plain(), DefaultAnswer::Yes, "\n");
    assert_eq!(yes, "Continue? [Y/n] ");
    let (_, no) = asked(&plain(), DefaultAnswer::No, "\n");
    assert_eq!(no, "Continue? [y/N] ");
    assert!(!yes.contains(ESCAPE));
}

#[test]
fn coloured_prompt_paints_the_choices_in_done() {
    let (_, shown) = asked(&coloured(), DefaultAnswer::Yes, "\n");
    assert_eq!(shown, "Continue? \x1b[32m[Y/n]\x1b[0m ");
}

#[test]
fn each_meaning_paints_its_code() {
    let screen = coloured();
    let table = [
        (Meaning::Program, "1;38;5;201"),
        (Meaning::Verb, "1;36"),
        (Meaning::Noun, "33"),
        (Meaning::Done, "32"),
        (Meaning::Failed, "31"),
        (Meaning::Waiting, "1;38;5;141"),
        (Meaning::Dim, "2"),
        (Meaning::Bold, "1"),
    ];
    for (meaning, code) in table {
        assert_eq!(
            screen.paint(meaning, "x"),
            format!("\x1b[{code}mx\x1b[0m"),
            "{meaning:?}"
        );
    }
}

#[test]
fn colour_off_paints_plain_text() {
    let screen = plain();
    assert_eq!(screen.paint(Meaning::Failed, "no"), "no");
    let line = "run zaf setup";
    assert_eq!(screen.program(line), line);
    assert!(!screen.program(line).contains(ESCAPE));
}

#[test]
fn no_color_set_turns_colour_off() {
    assert!(!detect(true, Some("1"), Some("xterm")).colour);
    assert!(detect(true, Some("1"), Some("xterm")).live);
}

#[test]
fn no_color_empty_keeps_colour() {
    assert!(detect(true, Some(""), Some("xterm-256color")).colour);
    assert!(detect(true, None, Some("xterm-256color")).colour);
    assert!(detect(true, None, None).colour);
}

#[test]
fn dumb_term_turns_colour_and_redraw_off() {
    let screen = detect(true, None, Some("dumb"));
    assert!(!screen.colour);
    assert!(!screen.live);
}

#[test]
fn a_pipe_turns_colour_and_redraw_off() {
    let screen = detect(false, None, Some("xterm"));
    assert!(!screen.colour);
    assert!(!screen.live);
}

#[test]
fn program_colours_whole_word_zaf_only() {
    let painted = coloured().paint(Meaning::Program, "zaf");
    assert_eq!(
        coloured().program("run zaf setup in ~/.zaf or .zaf.toml"),
        format!("run {painted} setup in ~/.zaf or .zaf.toml")
    );
    assert_eq!(coloured().program("zafx"), "zafx");
    assert_eq!(
        coloured().program("zaf/src and zaf-v1"),
        "zaf/src and zaf-v1"
    );
    assert_eq!(
        coloured().program("zaf, then zaf."),
        format!("{painted}, then {painted}.")
    );
}

/// A pseudo-terminal: the master end the "Engineer" types on and the slave end the process reads.
struct Pty {
    master: std::fs::File,
    slave: std::fs::File,
}

fn pty() -> Pty {
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (0, 0);
    // SAFETY: `openpty` writes two descriptors through the pointers, which are to live locals; the
    // null pointers ask for no name, terminal settings or window size.
    let made = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(made, 0, "openpty");
    // SAFETY: both descriptors were just opened and nothing else owns them.
    unsafe {
        Pty {
            master: std::fs::File::from_raw_fd(master),
            slave: std::fs::File::from_raw_fd(slave),
        }
    }
}

/// An output that types `line` on a file the moment the prompt is flushed: the Engineer answering
/// after the question is shown.
struct TypesAfterPrompt {
    typing: std::fs::File,
    line: &'static str,
}

impl Write for TypesAfterPrompt {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.typing.write_all(self.line.as_bytes())
    }
}

#[test]
fn typed_ahead_input_is_discarded() {
    use std::os::fd::AsRawFd;
    let Pty { mut master, slave } = pty();
    // The Enter pressed during a long step, before any question.
    master.write_all(b"\n").expect("type ahead");
    let typed = slave.as_raw_fd();
    let mut input = std::io::BufReader::new(slave);
    let mut output = TypesAfterPrompt {
        typing: master,
        line: "n\n",
    };
    // Enter would take the default, yes; only the line typed after the prompt answers.
    let answer = ask_yes_typed_on(
        &plain(),
        "Continue?",
        DefaultAnswer::Yes,
        &mut input,
        &mut output,
        typed,
    );
    assert!(!answer);
}

#[test]
fn input_on_a_pipe_is_not_flushed() {
    use std::os::fd::{AsRawFd, FromRawFd};
    let mut ends = [0; 2];
    // SAFETY: `pipe` writes two descriptors into the array.
    assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
    // SAFETY: both descriptors were just opened and nothing else owns them.
    let (read, mut write) = unsafe {
        (
            std::fs::File::from_raw_fd(ends[0]),
            std::fs::File::from_raw_fd(ends[1]),
        )
    };
    write.write_all(b"y\n").expect("write");
    let typed = read.as_raw_fd();
    let mut input = std::io::BufReader::new(read);
    let mut output = Vec::new();
    let answer = ask_yes_typed_on(
        &plain(),
        "Continue?",
        DefaultAnswer::No,
        &mut input,
        &mut output,
        typed,
    );
    assert!(answer);
}

const ERROR_LINE: &str = "zaf board claim: #7 is held by another instance";

#[test]
fn stderr_is_detected_on_its_own() {
    // Stdout a terminal and stderr a pipe: the error line is plain.
    let stdout = detect(true, None, Some("xterm"));
    let stderr = detect(false, None, Some("xterm"));
    assert!(stdout.colour);
    assert_eq!(stderr.error_line(ERROR_LINE), ERROR_LINE);
    assert!(!stderr.error_line(ERROR_LINE).contains(ESCAPE));
    // The reverse: stdout a pipe and stderr a terminal paints stderr only.
    let stdout = detect(false, None, Some("xterm"));
    let stderr = detect(true, None, Some("xterm"));
    assert_eq!(stdout.paint(Meaning::Program, "zaf"), "zaf");
    assert!(stderr.error_line(ERROR_LINE).contains(ESCAPE));
}

#[test]
fn an_error_starts_with_a_magenta_zaf() {
    let line = coloured().error_line(ERROR_LINE);
    let program = format!("\x1b[{}mzaf\x1b[0m", code(Meaning::Program));
    assert!(line.starts_with(&program), "{line:?}");
    assert_eq!(strip_escapes(&line), ERROR_LINE);
    // Only the leading `zaf` is painted: the rest keeps its text.
    let inside = "zaf begin: cannot start `zaf spawn --watch`: no";
    let painted = coloured().error_line(inside);
    assert_eq!(painted.matches(ESCAPE).count(), 2);
    assert_eq!(strip_escapes(&painted), inside);
    // A line that does not start with the program's name is as it was.
    assert_eq!(coloured().error_line("zafx: no"), "zafx: no");
    assert_eq!(
        coloured().error_line("pull request 3 is open"),
        "pull request 3 is open"
    );
}

#[test]
fn no_color_keeps_stderr_plain() {
    for (no_color, term) in [
        (Some("1"), Some("xterm")),
        (Some("1"), None),
        (None, Some("dumb")),
    ] {
        let stderr = detect(true, no_color, term);
        assert_eq!(stderr.error_line(ERROR_LINE), ERROR_LINE);
        assert!(!stderr.error_line(ERROR_LINE).contains(ESCAPE));
    }
    // An empty `NO_COLOR` is no setting.
    assert!(
        detect(true, Some(""), Some("xterm"))
            .error_line(ERROR_LINE)
            .contains(ESCAPE)
    );
}
