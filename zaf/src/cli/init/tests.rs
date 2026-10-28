use super::*;

#[test]
fn from_teams_is_accepted_but_not_built() {
    assert_eq!(
        crate::cli::run(["zaf", "init", "--from-teams"]),
        NOT_IMPLEMENTED
    );
}
