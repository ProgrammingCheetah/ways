use strum::VariantArray;

use super::*;
use crate::fakes::FakeGate;

#[test]
fn declared_levels_run_and_undeclared_ones_are_unavailable() {
    let gate = FakeGate::default();
    let failed_run = GateRun {
        verdict: GateVerdict::Failed,
        output: "1 test failed".to_owned(),
    };
    gate.declare(GateLevel::Full, failed_run.clone());
    let port: &dyn Gate = &gate;
    assert_eq!(port.run(GateLevel::Full), Ok(failed_run));
    assert!(matches!(
        port.run(GateLevel::Light),
        Err(PortError::Unavailable(_))
    ));
    assert_eq!(gate.runs(), [GateLevel::Full, GateLevel::Light]);
}

#[test]
fn level_tokens_are_pinned() {
    let tokens: Vec<String> = GateLevel::VARIANTS
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(tokens, ["full", "light"]);
}
