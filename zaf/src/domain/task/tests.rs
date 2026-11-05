use strum::VariantArray;

use super::*;

/// The template of the design page, filled with neutral values, in canonical form.
const FILLED_TEMPLATE: &str = "\
Goal: Members can export their reading list as a CSV file.
Owned paths: src/export/**, tests/export.rs
Interfaces: ExportSink (defined here), consumed by the export verb
Design pages: 1001, 1002
Architecture: export, ExportSink
Acceptance:
  - an empty list exports a header row only
  - a list of three books exports four rows
Tests: export_empty_list_writes_header, export_three_books_writes_four_rows
Security trigger: no
Model: sonnet
Size: S
Blocked by: #3, #5
";

fn filled() -> TaskTemplate {
    FILLED_TEMPLATE
        .parse()
        .unwrap_or_else(|error| panic!("the filled template parses: {error}"))
}

#[test]
fn template_round_trips_parse_render_parse() {
    let first = filled();
    let rendered = first.to_string();
    let second: TaskTemplate = rendered
        .parse()
        .unwrap_or_else(|error| panic!("the rendering parses: {error}"));
    assert_eq!(second, first);
    assert_eq!(rendered, FILLED_TEMPLATE);
}

#[test]
fn template_fields_are_read_into_their_types() {
    let template = filled();
    let expected_blockers = vec![IssueNumber::from(3), IssueNumber::from(5)];
    assert_eq!(
        template.goal,
        "Members can export their reading list as a CSV file."
    );
    assert_eq!(template.acceptance.len(), 2);
    assert_eq!(template.security_trigger, SecurityTrigger::No);
    assert_eq!(template.model, TaskModel::Sonnet);
    assert_eq!(template.size, TaskSize::S);
    assert_eq!(template.blocked_by, expected_blockers);
    assert_eq!(template.style, None);
}

#[test]
fn optional_style_and_a_security_trigger_round_trip() {
    let varied = FILLED_TEMPLATE
        .replace("Security trigger: no", "Security trigger: yes auth tokens")
        .replace(
            "Blocked by: #3, #5",
            "Blocked by: —\nStyle: the house style page",
        );
    let first: TaskTemplate = varied
        .parse()
        .unwrap_or_else(|error| panic!("the varied template parses: {error}"));
    let expected_trigger = SecurityTrigger::Yes("auth tokens".to_owned());
    assert_eq!(first.design_pages, "1001, 1002");
    assert_eq!(first.security_trigger, expected_trigger);
    assert!(first.blocked_by.is_empty());
    assert_eq!(first.style.as_deref(), Some("the house style page"));
    let second: TaskTemplate = first
        .to_string()
        .parse()
        .unwrap_or_else(|error| panic!("re-parses: {error}"));
    assert_eq!(second, first);
}

#[test]
fn missing_field_is_named() {
    let without_size = FILLED_TEMPLATE.replace("Size: S\n", "");
    let missing_size_error = TaskTemplateError::MissingField(TaskField::Size);
    assert_eq!(
        without_size.parse::<TaskTemplate>(),
        Err(missing_size_error)
    );
}

#[test]
fn unknown_field_is_named_with_its_line() {
    let with_extra = format!("{FILLED_TEMPLATE}Colour: blue\n");
    let unknown_field_error = TaskTemplateError::UnknownField {
        line: 14,
        label: "Colour".to_owned(),
    };
    assert_eq!(with_extra.parse::<TaskTemplate>(), Err(unknown_field_error));
}

#[test]
fn duplicate_field_is_rejected() {
    let doubled = format!("{FILLED_TEMPLATE}Design pages: 1003\n");
    let duplicate_error = TaskTemplateError::DuplicateField(TaskField::DesignPages);
    assert_eq!(doubled.parse::<TaskTemplate>(), Err(duplicate_error));
}

#[test]
fn values_outside_their_vocabulary_are_rejected() {
    let bad_model = FILLED_TEMPLATE.replace("Model: sonnet", "Model: large");
    let bad_model_error = TaskTemplateError::InvalidValue {
        field: TaskField::Model,
        value: "large".to_owned(),
    };
    assert_eq!(bad_model.parse::<TaskTemplate>(), Err(bad_model_error));
    let bad_trigger = FILLED_TEMPLATE.replace("Security trigger: no", "Security trigger: yes");
    assert!(bad_trigger.parse::<TaskTemplate>().is_err());
    let bad_blocker = FILLED_TEMPLATE.replace("#3, #5", "#3, soon");
    assert!(bad_blocker.parse::<TaskTemplate>().is_err());
}

#[test]
fn acceptance_needs_items_and_items_need_acceptance() {
    let no_items = FILLED_TEMPLATE
        .replace("  - an empty list exports a header row only\n", "")
        .replace("  - a list of three books exports four rows\n", "");
    let empty_acceptance_error = TaskTemplateError::EmptyField(TaskField::Acceptance);
    assert_eq!(
        no_items.parse::<TaskTemplate>(),
        Err(empty_acceptance_error)
    );
    let stray = format!("{FILLED_TEMPLATE}  - a stray statement\n");
    let stray_item_error = TaskTemplateError::StrayListItem { line: 14 };
    assert_eq!(stray.parse::<TaskTemplate>(), Err(stray_item_error));
}

/// The body of #38 exactly as posted on GitHub.
const TICKET_38_BODY: &str = r##"Goal: the archives of tag `zaf-v0.1.1` are named `zaf-v0.1.1-<target>.tar.gz` (the 0.1.0 release shipped `zaf-zaf-v0.1.0-<target>.tar.gz`: the packaging step prefixes the tag, which already starts with `zaf-`, with `zaf-` again), and `zaf/Cargo.toml` says 0.1.1 so the tag's version check passes.
Owned paths: .github/workflows/zaf-release.yml; zaf/Cargo.toml; zaf/Cargo.lock
Interfaces:
  1. In the step "Package archive and checksum", the line `name="zaf-${GITHUB_REF_NAME}-${{ matrix.target }}"` becomes `name="${GITHUB_REF_NAME}-${{ matrix.target }}"`. Nothing else in the workflow changes.
  2. `version = "0.1.1"` in zaf/Cargo.toml; the `zaf` entry of zaf/Cargo.lock follows (the gate's `cargo build` rewrites it; commit the rewritten lock).
Design pages: none; the three owned files, read fully.
Architecture: n/a (workflow and crate metadata only)
Acceptance:
  - the text `zaf-${GITHUB_REF_NAME}` appears nowhere in .github/workflows/zaf-release.yml, and `"${GITHUB_REF_NAME}-${{ matrix.target }}"` appears exactly once
  - `zaf/Cargo.toml` has `version = "0.1.1"` and the `name = "zaf"` entry in zaf/Cargo.lock has `version = "0.1.1"`
  - `zaf check <task>` is green (the four cargo commands)
  - the lead runs actionlint on the workflow and, after the merge into main, tags `zaf-v0.1.1`
Tests: none new (no code changes)
Security trigger: no
Model: sonnet
Size: S
Blocked by: none"##;

#[test]
fn indented_lines_continue_the_field_above() {
    let template: TaskTemplate = TICKET_38_BODY
        .parse()
        .unwrap_or_else(|error| panic!("the body of #38 parses: {error}"));
    let items: Vec<&str> = template.interfaces.lines().collect();
    assert_eq!(items.len(), 2);
    assert!(items[0].starts_with("1. In the step \"Package archive and checksum\""));
    assert!(items[1].starts_with("2. `version = \"0.1.1\"` in zaf/Cargo.toml"));
    assert_eq!(template.acceptance.len(), 4);
    assert_eq!(template.size, TaskSize::S);
}

#[test]
fn a_multi_line_field_round_trips_through_display() {
    let first: TaskTemplate = TICKET_38_BODY
        .parse()
        .unwrap_or_else(|error| panic!("the body of #38 parses: {error}"));
    let rendered = first.to_string();
    assert!(rendered.contains("Interfaces: 1. In the step"));
    assert!(rendered.contains("\n  2. `version"));
    let second: TaskTemplate = rendered
        .parse()
        .unwrap_or_else(|error| panic!("the rendering parses: {error}"));
    assert_eq!(second, first);
}

#[test]
fn an_indented_line_before_any_field_is_unrecognized() {
    let indented_first = format!("  1. an item with no field\n{FILLED_TEMPLATE}");
    let unrecognized_error = TaskTemplateError::UnrecognizedLine {
        line: 1,
        text: "1. an item with no field".to_owned(),
    };
    assert_eq!(
        indented_first.parse::<TaskTemplate>(),
        Err(unrecognized_error)
    );
}

#[test]
fn a_tab_indented_line_continues_and_a_stray_item_stays_refused() {
    let tabbed = FILLED_TEMPLATE.replace(
        "Owned paths: src/export/**, tests/export.rs",
        "Owned paths: src/export/**\n\ttests/export.rs",
    );
    let template: TaskTemplate = tabbed
        .parse()
        .unwrap_or_else(|error| panic!("the tabbed body parses: {error}"));
    assert_eq!(template.owned_paths, "src/export/**\ntests/export.rs");
    let stray = FILLED_TEMPLATE.replace(
        "Interfaces: ExportSink (defined here), consumed by the export verb",
        "Interfaces:\n  - not acceptance",
    );
    assert_eq!(
        stray.parse::<TaskTemplate>(),
        Err(TaskTemplateError::StrayListItem { line: 4 })
    );
}

/// The filled template written with `Scope` and `Out of scope` in place of `Owned paths`.
fn scoped_text() -> String {
    FILLED_TEMPLATE.replace(
        "Owned paths: src/export/**, tests/export.rs\n",
        "Scope: the CSV export of a reading list; start in src/export\n\
         Out of scope: the import path stays with #7\n",
    )
}

#[test]
fn scope_field_parses() {
    let template: TaskTemplate = scoped_text()
        .parse()
        .unwrap_or_else(|error| panic!("a scoped ticket parses: {error}"));
    assert!(template.scoped);
    assert_eq!(
        template.owned_paths,
        "the CSV export of a reading list; start in src/export"
    );
    assert_eq!(
        template.out_of_scope.as_deref(),
        Some("the import path stays with #7")
    );
    let rendered = template.to_string();
    assert!(rendered.contains("Scope: the CSV export"));
    assert!(rendered.contains("Out of scope: the import path"));
    assert!(!rendered.contains("Owned paths"));
    let second: TaskTemplate = rendered
        .parse()
        .unwrap_or_else(|error| panic!("the rendering parses: {error}"));
    assert_eq!(second, template);
}

#[test]
fn owned_paths_still_parses() {
    let template = filled();
    assert!(!template.scoped);
    assert_eq!(template.owned_paths, "src/export/**, tests/export.rs");
    assert_eq!(template.out_of_scope, None);
    assert!(template.to_string().contains("Owned paths: src/export/**"));
}

#[test]
fn ticket_without_scope_or_owned_paths_is_an_error() {
    let neither = FILLED_TEMPLATE.replace("Owned paths: src/export/**, tests/export.rs\n", "");
    let error = neither
        .parse::<TaskTemplate>()
        .expect_err("a ticket with neither field is refused");
    assert_eq!(error, TaskTemplateError::MissingScope);
    let message = error.to_string();
    assert!(message.contains("Scope"));
    assert!(message.contains("Owned paths"));
}

#[test]
fn scope_without_out_of_scope_is_an_error() {
    let unbounded = scoped_text().replace("Out of scope: the import path stays with #7\n", "");
    assert_eq!(
        unbounded.parse::<TaskTemplate>(),
        Err(TaskTemplateError::MissingField(TaskField::OutOfScope))
    );
    let message = TaskTemplateError::MissingField(TaskField::OutOfScope).to_string();
    assert!(message.contains("Out of scope"));
}

#[test]
fn scope_wins_over_owned_paths() {
    let both = scoped_text().replace("Interfaces:", "Owned paths: src/export/**\nInterfaces:");
    let template: TaskTemplate = both
        .parse()
        .unwrap_or_else(|error| panic!("a ticket with both parses: {error}"));
    assert!(template.scoped);
    assert_eq!(
        template.owned_paths,
        "the CSV export of a reading list; start in src/export"
    );
}

#[test]
fn architecture_is_optional() {
    let without = FILLED_TEMPLATE.replace("Architecture: export, ExportSink\n", "");
    let template: TaskTemplate = without
        .parse()
        .unwrap_or_else(|error| panic!("a ticket without Architecture parses: {error}"));
    assert_eq!(template.architecture, "");
    assert!(!template.to_string().contains("Architecture"));
    let second: TaskTemplate = template
        .to_string()
        .parse()
        .unwrap_or_else(|error| panic!("the rendering parses: {error}"));
    assert_eq!(second, template);
}

#[test]
fn field_labels_are_pinned() {
    let labels: Vec<String> = TaskField::VARIANTS
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        labels,
        [
            "Goal",
            "Owned paths",
            "Scope",
            "Out of scope",
            "Interfaces",
            "Design pages",
            "Architecture",
            "Acceptance",
            "Tests",
            "Security trigger",
            "Model",
            "Size",
            "Blocked by",
            "Style",
        ]
    );
}
