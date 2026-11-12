use super::*;

fn hold(codes: &[i32], last_line: Option<&str>) -> Hold {
    Hold {
        task: TicketId::from(3),
        action: ActionName::from("fix"),
        codes: codes.to_vec(),
        last_line: last_line.map(str::to_owned),
        last_run: "1700".to_owned(),
    }
}

#[test]
fn the_question_names_the_phase_the_codes_and_the_last_line() {
    assert_eq!(
        question_text(&hold(&[1, 2, 3], Some("no such file"))),
        "fix failed 3 times in a row with exit codes 1, 2, 3; the last line of its log: \
         \"no such file\". It is not started again until this is ruled. Last run: 1700."
    );
}

#[test]
fn a_run_with_no_output_says_so() {
    assert!(question_text(&hold(&[1, 1, 1], None)).contains("none was recorded"));
}

#[test]
fn a_ruled_question_names_the_run_it_counted_to() {
    let text = question_text(&hold(&[1, 1, 1], Some("Last run: 9. nope")));
    assert_eq!(marked_run(&text, &ActionName::from("fix")), Some("1700"));
    assert_eq!(
        marked_run(&text, &ActionName::from("other")),
        None,
        "another phase's question is not this one's"
    );
    assert_eq!(
        marked_run("Which retry window?", &ActionName::from("fix")),
        None
    );
}

#[test]
fn the_limit_is_three() {
    assert_eq!(MODEL_FAILURE_LIMIT, 3);
}
