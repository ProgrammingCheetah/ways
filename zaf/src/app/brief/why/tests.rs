use super::*;

fn conflicted() -> FailedRun {
    FailedRun {
        phase: ActionName::from("merge"),
        head: Some("3f2a1c0".to_owned()),
        exit: Some(1),
        meaning: Some("conflicted: the base conflicts with the slice".to_owned()),
        checks: Vec::new(),
        asked: Vec::new(),
        output: Some(
            "FAIL merge: exit 1, conflicted, log x\nthe base conflicts with the slice in: notes.txt\n"
                .to_owned(),
        ),
    }
}

#[test]
fn a_failed_merge_names_its_exit_its_meaning_and_what_it_printed() {
    let why = WhyThisRun {
        failed: Some(conflicted()),
        rulings: Vec::new(),
    };
    let text = why.render();
    assert!(text.starts_with("## Why this run\n\n"), "{text}");
    assert!(
        text.contains(
            "Phase `merge` failed at 3f2a1c0: exit 1, conflicted: the base conflicts with the slice.\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("```\nFAIL merge: exit 1, conflicted, log x\nthe base conflicts with the slice in: notes.txt\n```"),
        "{text}"
    );
    assert!(
        text.contains("### Rulings since the last model run\n\n— none\n"),
        "{text}"
    );
}

#[test]
fn a_red_check_and_a_ruled_question_are_named() {
    let why = WhyThisRun {
        failed: Some(FailedRun {
            phase: ActionName::from("review"),
            head: None,
            exit: None,
            meaning: None,
            checks: vec!["build".to_owned(), "lint".to_owned()],
            asked: vec![TicketId::from(45)],
            output: None,
        }),
        rulings: vec![Ruling {
            question: TicketId::from(45),
            text: "Should the export quote every field?".to_owned(),
            ruling: "Yes: quote every field, and test it.".to_owned(),
        }],
    };
    let text = why.render();
    assert!(
        text.contains("Phase `review` failed with no pull request head.\n"),
        "{text}"
    );
    assert!(text.contains("Failed checks: build, lint.\n"), "{text}");
    assert!(text.contains("Its run raised #45, ruled since"), "{text}");
    assert!(!text.contains("What its run printed last"), "{text}");
    assert!(
        text.contains(
            "#45 Should the export quote every field?\nruling: Yes: quote every field, and test it.\n"
        ),
        "{text}"
    );
}

#[test]
fn nothing_failed_still_carries_the_rulings_and_an_unread_one_says_why() {
    let why = WhyThisRun {
        failed: None,
        rulings: Vec::new(),
    };
    assert!(
        why.render().contains("No phase has failed"),
        "{}",
        why.render()
    );
    assert_eq!(
        WhyThisRun::unread("pull request #9: not found"),
        "## Why this run\n\nzaf could not read why this run was started: pull request #9: not found\n\n"
    );
}
