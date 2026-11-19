use super::*;

fn plain() -> Screen {
    Screen {
        colour: false,
        live: false,
        rows: 24,
        columns: 80,
    }
}

const FILE: &str = ".zaf/releases/0.1.0/objectives.md";

#[test]
fn begin_draws_four_stages_at_objectives() {
    let text = stages_text(&plain(), "0.1.0", 0);
    let expected = "
Release 0.1.0 goes through 4 stages. You are at the first:

  ▸ 1. Objectives  you say what 0.1.0 is for; Claude asks, writes it down, you sign
    2. Design      Claude drafts how to build it; you answer its questions, you sign
    3. Epics       the design becomes epics of tickets; you sign each epic
    4. Build       zaf builds the signed tickets; you watch, and answer when asked
";
    assert_eq!(text, expected);
    assert!(!text.contains('\x1b'));
}

#[test]
fn begin_marks_objectives_done_at_design() {
    let text = stages_text(&plain(), "0.1.0", 1);
    assert!(text.contains("You are at the second:"), "{text}");
    assert!(text.contains("  ✓ 1. Objectives "), "{text}");
    assert!(text.contains("  ▸ 2. Design "), "{text}");
    assert!(text.contains("    3. Epics "), "{text}");
}

#[test]
fn begin_stage_index_follows_the_stage_and_a_named_phase() {
    assert_eq!(stage_index(true, Some(Stage::Design), Phase::Design), 1);
    assert_eq!(
        stage_index(true, Some(Stage::Objectives), Phase::Objectives),
        0
    );
    // A phase named by hand opens whatever stage the board reports.
    assert_eq!(stage_index(false, Some(Stage::Build), Phase::Objectives), 0);
    assert_eq!(stage_index(true, None, Phase::Design), 1);
}

#[test]
fn begin_coloured_screen_paints_the_stage_marks() {
    let screen = Screen {
        colour: true,
        ..plain()
    };
    let text = stages_text(&screen, "0.1.0", 1);
    assert!(text.contains("\x1b[32m✓\x1b[0m"), "{text:?}");
    assert!(text.contains("\x1b[1;36m▸\x1b[0m"), "{text:?}");
}

#[test]
fn begin_preface_names_the_session_and_how_to_leave_it() {
    let text = preface_text(&plain(), Phase::Objectives);
    assert!(
        text.contains("zaf now opens Claude Code for the objectives interview."),
        "{text}"
    );
    assert!(
        text.contains(
            "Answer in your own words. Leave Claude (/exit) when the file says what you mean."
        ),
        "{text}"
    );
}

#[test]
fn begin_open_no_changes_nothing() {
    let mut out = Vec::new();
    let opened = open_it(&plain(), "0.1.0", &mut "n\n".as_bytes(), &mut out).expect("write");
    assert!(!opened);
    let out = String::from_utf8(out).expect("utf8");
    assert_eq!(
        out,
        "Open it? [Y/n] \nNothing changed. Run zaf begin --version 0.1.0 when you are ready.\n"
    );
}

#[test]
fn begin_open_goes_on_at_enter() {
    let mut out = Vec::new();
    assert!(open_it(&plain(), "0.1.0", &mut "\n".as_bytes(), &mut out).expect("write"));
}

#[test]
fn begin_prints_written_then_asks_sign() {
    let mut out = Vec::new();
    let yes = sign_it(&plain(), FILE, &mut "y\n".as_bytes(), &mut out).expect("write");
    assert!(yes);
    let out = String::from_utf8(out).expect("utf8");
    assert_eq!(
        out,
        format!(
            "\n  ✓ {FILE} written\nSign it? It says what you mean, and an edit later unsigns it. [Y/n] "
        )
    );
    assert_eq!(signed_text(&plain(), FILE), format!("  ✓ {FILE} signed"));
}

#[test]
fn sign_question_is_yes_by_default_on_screen() {
    for yes in ["\n", "y\n", "yes\n"] {
        let mut out = Vec::new();
        assert!(
            sign_it(&plain(), FILE, &mut yes.as_bytes(), &mut out).expect("write"),
            "{yes:?}"
        );
        assert!(
            String::from_utf8(out).expect("utf8").ends_with("[Y/n] "),
            "{yes:?}"
        );
    }
    for no in ["n\n", "nope\n", ""] {
        let mut out = Vec::new();
        assert!(
            !sign_it(&plain(), FILE, &mut no.as_bytes(), &mut out).expect("write"),
            "{no:?}"
        );
    }
}

#[test]
fn sign_question_other_answers_ask_again_on_screen() {
    let mut out = Vec::new();
    assert!(sign_it(&plain(), FILE, &mut "q\ny\n".as_bytes(), &mut out).expect("write"));
    let out = String::from_utf8(out).expect("utf8");
    assert_eq!(out.matches("[Y/n] ").count(), 2, "{out}");
}

#[test]
fn begin_sign_no_says_not_signed() {
    let mut out = Vec::new();
    assert!(!sign_it(&plain(), FILE, &mut "n\n".as_bytes(), &mut out).expect("write"));
    assert_eq!(
        not_signed_text(&plain(), "0.1.0"),
        "\nNot signed. Run zaf begin --version 0.1.0 to open it again.\n"
    );
}

fn to_sign(number: u64, title: &str, total: usize, unsigned: bool) -> EpicToSign {
    EpicToSign {
        epic: crate::app::epic::EpicLine {
            number: number.into(),
            title: title.into(),
            stage: if unsigned {
                crate::domain::EpicStage::Unsigned
            } else {
                crate::domain::EpicStage::Signed
            },
            done: 0,
            total,
            signature_void: false,
        },
        unsigned,
    }
}

fn two_epics() -> Vec<EpicToSign> {
    vec![
        to_sign(12, "Accounts: sign up and sign in", 6, false),
        to_sign(13, "Profile page", 0, true),
    ]
}

#[test]
fn begin_epics_stage_cleans_titles() {
    let epics = [to_sign(12, "A\x1b[2J\x1b]52;c;aGk=\x07B\u{9b}C", 6, true)];
    let text = epics_stage_text(&plain(), "0.1.0", 2, &epics);
    assert!(text.contains("A [2J ]52;c;aGk= B C"), "{text:?}");
    assert!(
        !text.chars().any(|c| c.is_control() && c != '\n'),
        "{text:?}"
    );
}

#[test]
fn begin_epics_stage_lists_read_and_sign() {
    let text = epics_stage_text(&plain(), "0.1.0", 2, &two_epics());
    assert!(text.contains("  ✓ 1. Objectives "), "{text}");
    assert!(text.contains("  ▸ 3. Epics "), "{text}");
    assert!(
        text.contains("The design became 2 epics. Read each, then sign it to let zaf build it:"),
        "{text}"
    );
    assert!(
        text.contains("  12   Accounts: sign up and sign in"),
        "{text}"
    );
    assert!(text.contains("6 tickets"), "{text}");
    assert!(
        text.contains("       read zaf epic show 12   signed\n"),
        "{text}"
    );
    assert!(
        text.contains("       read zaf epic show 13   sign zaf epic sign 13\n"),
        "{text}"
    );
    assert!(!text.contains('\x1b'));
}

#[test]
fn begin_epics_stage_notes_unsigned_epic() {
    let text = epics_stage_text(&plain(), "0.1.0", 2, &two_epics());
    assert!(text.contains("  13   Profile page"), "{text}");
    assert!(
        text.contains("zaf plans its tickets once you sign it"),
        "{text}"
    );
    assert_eq!(text.matches("plans its tickets").count(), 1, "{text}");
    assert!(!text.contains("sign zaf epic sign 12"), "{text}");
}

#[test]
fn begin_epics_stage_ends_with_spawn_line() {
    let text = epics_stage_text(&plain(), "0.1.0", 2, &two_epics());
    assert!(
        text.ends_with(
            "\nOnce they are signed, start the build:\n\n  zaf spawn --watch --version 0.1.0   leave it running; zaf hub shows it live\n\n"
        ),
        "{text}"
    );
}

#[test]
fn begin_epics_stage_json_unchanged() {
    let value = nothing_due_json("0.1.0", &two_epics());
    assert_eq!(value["version"], "0.1.0");
    assert_eq!(value["due"], "nothing");
    let first = value["epics"][0].as_object().expect("an epic object");
    let mut keys: Vec<&str> = first.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "done",
            "number",
            "signature_void",
            "stage",
            "title",
            "total",
            "unsigned"
        ]
    );
}

#[test]
fn begin_built_stage_names_pr_and_next() {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = built_stage(
        &plain(),
        "0.5.2",
        "0.6.0",
        || {
            Ok(ReleasePullRequest {
                number: "77".to_owned(),
                link: Some("https://forge.example/pulls/77".to_owned()),
            })
        },
        &mut out,
        &mut err,
    );
    assert_eq!(code, OK);
    assert!(err.is_empty());
    let out = String::from_utf8(out).expect("utf8");
    for mark in [
        "  ✓ 1. Objectives",
        "  ✓ 2. Design",
        "  ✓ 3. Epics",
        "  ✓ 4. Build",
    ] {
        assert!(out.contains(mark), "{out}");
    }
    assert!(!out.contains('▸'), "{out}");
    assert!(
        out.contains("Release pull request #77 https://forge.example/pulls/77"),
        "{out}"
    );
    assert!(out.contains("zaf begin --version 0.6.0"), "{out}");
    assert!(!out.contains('\x1b'));
}

#[test]
fn begin_built_stage_refused_release_open() {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = built_stage(
        &plain(),
        "0.5.2",
        "0.6.0",
        || {
            Err(BuildError::Refused(
                "feature/0.5.2 is not on the remote origin".to_owned(),
            ))
        },
        &mut out,
        &mut err,
    );
    assert_eq!(code, REFUSED);
    let err = String::from_utf8(err).expect("utf8");
    assert_eq!(
        err,
        "zaf begin: feature/0.5.2 is not on the remote origin\n"
    );
    let out = String::from_utf8(out).expect("utf8");
    assert!(!out.contains("zaf begin --version"), "{out}");
}

#[test]
fn begin_built_stage_backend_failure_exits_2() {
    let mut err = Vec::new();
    let code = built_stage(
        &plain(),
        "0.5.2",
        "0.6.0",
        || Err(BuildError::Backend("FAIL release-open".to_owned())),
        &mut Vec::new(),
        &mut err,
    );
    assert_eq!(code, BACKEND);
    assert_eq!(
        String::from_utf8(err).expect("utf8"),
        "zaf begin: FAIL release-open\n"
    );
}
