use strum::VariantArray;

use super::*;

#[test]
fn task_label_tokens_are_pinned() {
    let tokens: Vec<String> = TaskLabel::VARIANTS
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(tokens, ["ready", "claimed", "blocked", "question", "done"]);
    for label in TaskLabel::VARIANTS {
        assert_eq!(label.to_string().parse::<TaskLabel>(), Ok(*label));
    }
}

#[test]
fn page_status_tokens_are_pinned() {
    let tokens: Vec<String> = PageStatus::VARIANTS
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(tokens, ["decided", "current", "proposed", "superseded"]);
    for status in PageStatus::VARIANTS {
        assert_eq!(status.to_string().parse::<PageStatus>(), Ok(*status));
    }
}

#[test]
fn only_decided_and_current_pages_are_citable() {
    let citable: Vec<PageStatus> = PageStatus::VARIANTS
        .iter()
        .copied()
        .filter(|status| status.is_citable())
        .collect();
    assert_eq!(citable, [PageStatus::Decided, PageStatus::Current]);
}
