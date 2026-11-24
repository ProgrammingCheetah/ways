use std::collections::BTreeSet;
use std::path::Path;

use super::*;
use crate::domain::plan::PlanInput;

/// Fixture 25's vocabulary, written as the block a design holds.
const BLOCK_25: &str = r#"
[areas]
"prompts" = "the standing briefs"
"docs" = "the docs"
"adapters/mcp" = "the MCP server"

[names."mcp.catalog"]
signature = "fn catalog() -> Vec<Tool>"
epic = 1157

[names."prompt.fix"]
no_signature = true
epic = 1157

[names."cli"]
no_signature = true
existing = "zaf/src/cli.rs"
"#;

fn set<T: Ord + From<&'static str>>(items: &[&'static str]) -> BTreeSet<T> {
    items.iter().map(|item| T::from(*item)).collect()
}

#[test]
fn vocabulary_parses_fixture_25() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.teams/releases/0.6.0/plan-graph/fixtures/25-vocabulary-gives-the-lists.json");
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture reads"))
            .expect("the fixture is JSON");
    let plan: PlanInput = serde_json::from_value(fixture["plan"].clone()).expect("a plan");
    let held = plan.vocabulary.expect("fixture 25 has a vocabulary");

    let parsed = parse(BLOCK_25).expect("the block parses");
    // The fixture holds the four lists; the epics the block gives its names to are kept besides.
    assert_eq!(
        parsed,
        Vocabulary {
            epic: parsed.epic.clone(),
            ..held
        }
    );
    assert_eq!(
        parsed.epic,
        [("mcp.catalog", 1157), ("prompt.fix", 1157)]
            .into_iter()
            .map(|(name, epic)| (Name::from(name), epic.into()))
            .collect()
    );
    assert_eq!(parsed.areas, set(&["prompts", "docs", "adapters/mcp"]));
    assert_eq!(parsed.signature, set(&["mcp.catalog"]));
    assert_eq!(parsed.no_signature, set(&["prompt.fix"]));
    assert_eq!(parsed.existing, set(&["cli"]));
}

#[test]
fn vocabulary_refuses_both_signature_kinds() {
    let block = "[names.\"x.y\"]\nsignature = \"s\"\nno_signature = true\n";
    let error = parse(block).unwrap_err();
    assert_eq!(error, VocabularyError::BothSignatureKinds("x.y".into()));
    assert!(error.to_string().contains("\"x.y\""));
}

#[test]
fn vocabulary_refuses_an_unknown_key() {
    let error = parse("[names.\"x\"]\nno_signature = true\nshape = 1\n").unwrap_err();
    assert!(error.to_string().contains("`shape`"), "{error}");
    assert!(error.to_string().contains("\"x\""), "{error}");
    let error = parse("[extra]\na = 1\n").unwrap_err();
    assert!(error.to_string().contains("`extra`"), "{error}");
}

#[test]
fn vocabulary_refuses_neither_signature_kind() {
    let error = parse("[names.\"x\"]\nepic = 3\n").unwrap_err();
    assert_eq!(error, VocabularyError::NoSignatureKind("x".into()));
}

#[test]
fn vocabulary_refuses_a_bad_name_or_area() {
    let error = parse("[names.\"Bad Name\"]\nno_signature = true\n").unwrap_err();
    assert_eq!(error, VocabularyError::BadName("Bad Name".into()));
    let error = parse("[names.\"sig:x\"]\nno_signature = true\n").unwrap_err();
    assert_eq!(error, VocabularyError::BadName("sig:x".into()));
    let error = parse("[areas]\n\"Docs Area\" = \"n\"\n").unwrap_err();
    assert_eq!(error, VocabularyError::BadArea("Docs Area".into()));
}

#[test]
fn vocabulary_refuses_text_that_is_not_toml() {
    let error = parse("[areas\n").unwrap_err();
    assert!(matches!(error, VocabularyError::NotToml(_)));
    assert!(!error.to_string().contains('\n'));
}

#[test]
fn vocabulary_refuses_an_entry_with_epic_and_existing() {
    let block = "[names.\"x\"]\nno_signature = true\nepic = 2\nexisting = \"a/b\"\n";
    assert_eq!(
        parse(block).unwrap_err(),
        VocabularyError::BothProviders("x".into())
    );
}

#[test]
fn vocabulary_refuses_a_wrong_typed_value() {
    for block in [
        "[names.\"x\"]\nsignature = 3\n",
        "[names.\"x\"]\nno_signature = false\n",
        "[names.\"x\"]\nno_signature = true\nepic = \"two\"\n",
        "[names.\"x\"]\nno_signature = true\nexisting = \"\"\n",
        "[areas]\ndocs = 4\n",
    ] {
        assert!(
            matches!(parse(block), Err(VocabularyError::BadValue { .. })),
            "{block}"
        );
    }
}

#[test]
fn an_empty_block_is_an_empty_vocabulary() {
    assert_eq!(parse("").unwrap(), Vocabulary::default());
}
