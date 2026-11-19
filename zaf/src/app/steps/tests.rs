use super::*;

#[test]
fn recorder_keeps_the_order_of_calls() {
    let mut recorder = Recorder::default();
    recorder.started(0);
    recorder.ended(0, &StepEnd::Done("zaf.toml".into()));
    recorder.started(1);
    recorder.ended(1, &StepEnd::Failed("no tool".into()));
    assert_eq!(
        recorder.calls,
        vec![
            Call::Started(0),
            Call::Ended(0, StepEnd::Done("zaf.toml".into())),
            Call::Started(1),
            Call::Ended(1, StepEnd::Failed("no tool".into())),
        ]
    );
}
