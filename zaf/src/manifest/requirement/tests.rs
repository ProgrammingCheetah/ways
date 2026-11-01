use super::*;

fn version(text: &str) -> semver::Version {
    semver::Version::parse(text).expect("a valid version")
}

#[test]
fn version_requirements_parse() {
    for text in ["*", ">=2.1", "2.0.0", "=1.4.2"] {
        let requirement = text.parse::<Requirement>();
        assert!(
            matches!(requirement, Ok(Requirement::Version(_))),
            "{text} parses as a version requirement"
        );
    }
    let at_least = ">=2.1".parse::<Requirement>().expect("parses");
    assert!(at_least.is_met_by(&version("2.3.0")));
    assert!(!at_least.is_met_by(&version("2.0.9")));
}

#[test]
fn release_tags_parse_and_pin_exactly() {
    let tag = "toolkit-v2.0.0"
        .parse::<Requirement>()
        .expect("a tag parses");
    let expected_tag = Requirement::Tag {
        name: "toolkit".to_owned(),
        version: version("2.0.0"),
    };
    assert_eq!(tag, expected_tag);
    assert_eq!(tag.to_string(), "toolkit-v2.0.0");
    assert!(tag.is_met_by(&version("2.0.0")));
    assert!(!tag.is_met_by(&version("2.0.1")));
}

#[test]
fn other_text_is_rejected_with_the_value() {
    for text in ["latest", "-v1.0.0", "toolkit-vnext", ""] {
        let rejected_error = RequirementError {
            value: text.to_owned(),
        };
        assert_eq!(text.parse::<Requirement>(), Err(rejected_error));
    }
}
