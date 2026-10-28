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
