use std::collections::BTreeSet;

use super::{PlannedTicket, Section, Signature};
use crate::domain::plan::{Name, NodeId, NodeKind};
use crate::domain::{SecurityTrigger, TaskModel, TaskSize, TaskTemplate, TaskTemplateError};

fn section(size: TaskSize, model: TaskModel) -> Section {
    Section {
        key: None,
        template: TaskTemplate {
            goal: "a goal".to_owned(),
            owned_paths: "a path".to_owned(),
            scoped: false,
            out_of_scope: None,
            interfaces: "none".to_owned(),
            design_pages: "none".to_owned(),
            architecture: String::new(),
            acceptance: vec!["a line".to_owned()],
            tests: "a test".to_owned(),
            security_trigger: SecurityTrigger::No,
            model,
            size,
            blocked_by: Vec::new(),
            style: None,
        },
        writes: BTreeSet::new(),
        provides: BTreeSet::new(),
        implements: BTreeSet::new(),
        needs: BTreeSet::new(),
    }
}

fn ticket(sections: Vec<Section>) -> PlannedTicket {
    PlannedTicket {
        node: NodeId::from("A"),
        kind: NodeKind::Body,
        signatures: Vec::new(),
        turns: Vec::new(),
        waits: Vec::new(),
        sections,
    }
}

fn interface(count: usize) -> PlannedTicket {
    let signatures = (0..count)
        .map(|n| Signature {
            name: Name::from(format!("name-{n}").as_str()),
            shape: "fn shape()".to_owned(),
        })
        .collect();
    PlannedTicket {
        node: NodeId::from("I@1004.1"),
        kind: NodeKind::Interface,
        signatures,
        turns: Vec::new(),
        waits: Vec::new(),
        sections: vec![section(TaskSize::S, TaskModel::Sonnet)],
    }
}

#[test]
fn planned_size_one_xs_is_xs() {
    let one = ticket(vec![section(TaskSize::XS, TaskModel::Sonnet)]);
    assert_eq!(one.size(), Ok(TaskSize::XS));
}

#[test]
fn planned_size_one_s_is_s() {
    let one = ticket(vec![section(TaskSize::S, TaskModel::Sonnet)]);
    assert_eq!(one.size(), Ok(TaskSize::S));
}

#[test]
fn planned_size_two_behaviours_is_m() {
    let two_s = ticket(vec![
        section(TaskSize::S, TaskModel::Sonnet),
        section(TaskSize::S, TaskModel::Sonnet),
    ]);
    assert_eq!(two_s.size(), Ok(TaskSize::M));
    let xs_and_s = ticket(vec![
        section(TaskSize::XS, TaskModel::Sonnet),
        section(TaskSize::S, TaskModel::Sonnet),
    ]);
    assert_eq!(xs_and_s.size(), Ok(TaskSize::M));
    let one_m = ticket(vec![section(TaskSize::M, TaskModel::Sonnet)]);
    assert_eq!(one_m.size(), Ok(TaskSize::M));
}

#[test]
fn planned_size_past_two_is_refused() {
    let three = ticket(vec![
        section(TaskSize::S, TaskModel::Sonnet),
        section(TaskSize::S, TaskModel::Sonnet),
        section(TaskSize::S, TaskModel::Sonnet),
    ]);
    assert_eq!(
        three.size(),
        Err(TaskTemplateError::TooLarge {
            weight: 3,
            limit: 2
        })
    );
    let large = ticket(vec![section(TaskSize::L, TaskModel::Sonnet)]);
    assert!(matches!(
        large.size(),
        Err(TaskTemplateError::TooLarge { limit: 2, .. })
    ));
}

#[test]
fn interface_size_from_signatures() {
    assert_eq!(interface(1).size(), Ok(TaskSize::S));
    assert_eq!(interface(2).size(), Ok(TaskSize::S));
    assert_eq!(interface(3).size(), Ok(TaskSize::M));
    assert_eq!(interface(7).size(), Ok(TaskSize::M));
}

#[test]
fn planned_model_opus_if_any_section() {
    let merged = ticket(vec![
        section(TaskSize::S, TaskModel::Sonnet),
        section(TaskSize::S, TaskModel::Opus),
    ]);
    assert_eq!(merged.model(), TaskModel::Opus);
    let sonnet = ticket(vec![
        section(TaskSize::S, TaskModel::Sonnet),
        section(TaskSize::S, TaskModel::Sonnet),
    ]);
    assert_eq!(sonnet.model(), TaskModel::Sonnet);
}
