//! The measures of each fixture before the passes: the plan as written, its waits derived and
//! nothing rewritten (the reference's `baseline`). The `after` measures need the whole optimiser,
//! so the fixtures' own check holds them.

use zaf::domain::plan::measure::baseline;

use super::{Outcome, fixtures};

#[test]
fn before_measures() {
    let mut checked = 0;
    for (file, fixture) in fixtures() {
        let Outcome::Plan { before, .. } = fixture.expect.outcome() else {
            continue;
        };
        let got = baseline(&fixture.plan)
            .unwrap_or_else(|refusal| panic!("{file} is refused before its passes: {refusal}"));
        assert_eq!(&got, before, "{file}: the measures before");
        checked += 1;
    }
    assert_eq!(checked, 21, "every fixture that is no refusal is checked");
}

#[test]
fn fixture_20() {
    let (_, fixture) = fixtures()
        .into_iter()
        .find(|(file, _)| file.starts_with("20-"))
        .expect("fixture 20 is in the folder");
    let got = baseline(&fixture.plan).expect("fixture 20 measures");
    assert_eq!(got.critical, 4);
    let path: Vec<&str> = got.critical_path.iter().map(|id| id.as_str()).collect();
    assert_eq!(path, ["A", "C", "E"]);
}
