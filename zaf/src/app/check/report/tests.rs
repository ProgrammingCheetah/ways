use super::*;

fn source(name: &str) -> FindingSource {
    FindingSource::from(name)
}

fn at(path: &str, line: u32) -> Option<Location> {
    Some(Location {
        path: path.to_owned(),
        line: Some(line),
    })
}

fn finding(from: &str, severity: Severity, message: &str, location: Option<Location>) -> Finding {
    Finding {
        source: source(from),
        severity,
        message: message.to_owned(),
        location,
    }
}

#[test]
fn must_and_note_lines_become_findings_and_prose_is_ignored() {
    let report = "Reviewed the diff.\n\
        MUST: src/export.rs:12 the empty list is not exported\n\
          NOTE: src/export.rs:40 name the constant\n\
        musts: none else\n";
    let findings = parse_report(&source("runner:local:critique"), report);
    let expected = [
        finding(
            "runner:local:critique",
            Severity::MustFix,
            "the empty list is not exported",
            at("src/export.rs", 12),
        ),
        finding(
            "runner:local:critique",
            Severity::Advisory,
            "name the constant",
            at("src/export.rs", 40),
        ),
    ];
    assert_eq!(findings, expected);
}

#[test]
fn a_line_without_a_location_keeps_its_whole_text() {
    let findings = parse_report(&source("ci"), "MUST: the build is red\nNOTE:\n");
    let expected = [finding("ci", Severity::MustFix, "the build is red", None)];
    assert_eq!(findings, expected);
}

#[test]
fn duplicates_within_the_window_merge_and_keep_the_gravest_severity() {
    let findings = vec![
        finding(
            "first",
            Severity::Advisory,
            "Unchecked input!",
            at("a.rs", 10),
        ),
        finding(
            "second",
            Severity::MustFix,
            "unchecked  input",
            at("a.rs", 13),
        ),
        finding(
            "second",
            Severity::MustFix,
            "unchecked input",
            at("a.rs", 14),
        ),
        finding(
            "third",
            Severity::MustFix,
            "unchecked input",
            at("b.rs", 10),
        ),
        finding("third", Severity::MustFix, "another title", at("a.rs", 10)),
    ];
    let checked = dedupe(findings);
    assert_eq!(checked.len(), 4);
    assert_eq!(checked[0].finding.source, source("first"));
    assert_eq!(checked[0].finding.severity, Severity::MustFix);
    assert_eq!(checked[0].also_found_by, [source("second")]);
    assert_eq!(checked[1].finding.location, at("a.rs", 14));
    assert!(checked[2].also_found_by.is_empty());
}
