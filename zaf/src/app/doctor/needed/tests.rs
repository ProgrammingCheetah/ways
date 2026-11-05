use super::*;

fn names(line: &str) -> Vec<(String, bool)> {
    calls_in(line)
        .into_iter()
        .map(|call| (call.name, call.machine))
        .collect()
}

fn call(name: &str, machine: bool) -> (String, bool) {
    (name.to_owned(), machine)
}

#[test]
fn a_plain_call_names_its_action() {
    assert_eq!(names("zaf do gate-light"), [call("gate-light", false)]);
    assert_eq!(
        names("cd zaf && zaf do test {name}"),
        [call("test", false)],
        "a placeholder after the name is an argument"
    );
}

#[test]
fn a_tier_flag_is_skipped_and_marks_the_call() {
    assert_eq!(
        names("zaf do --tier machine push origin"),
        [call("push", true)]
    );
    assert_eq!(names("zaf do --tier=machine push"), [call("push", true)]);
    assert_eq!(names("zaf do --show land"), [call("land", false)]);
}

#[test]
fn several_calls_on_a_line_are_all_found() {
    assert_eq!(
        names("zaf do a; zaf do b && zaf do c | cat"),
        [call("a", false), call("b", false), call("c", false)]
    );
    assert_eq!(names("(zaf do a)"), [call("a", false)]);
    assert_eq!(names("/usr/local/bin/zaf do a"), [call("a", false)]);
    assert_eq!(names("zaf do 'quoted'"), [call("quoted", false)]);
}

#[test]
fn a_name_that_is_not_literal_is_not_found() {
    assert!(names("zaf do \"$name\"").is_empty());
    assert!(names("zaf do $(pick)").is_empty());
    assert!(names("zaf do `pick`").is_empty());
    assert!(names("zaf do ${name}").is_empty());
    assert!(names("zaf do").is_empty());
    assert!(names("zaf --version; echo do x").is_empty());
    assert!(names("lazaf do x").is_empty());
}
