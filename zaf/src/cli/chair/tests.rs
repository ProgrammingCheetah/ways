use std::path::PathBuf;

use super::{Bare, bare, nowhere};
use crate::cli::screen::{Environment, Screen};

#[test]
fn bare_zaf_follows_d1_in_each_of_its_four_cases() {
    let project = PathBuf::from("/srv/ways");

    assert_eq!(
        bare(false, Some(project.clone()), 3),
        Bare::Usage,
        "no terminal"
    );
    assert_eq!(
        bare(true, Some(project.clone()), 0),
        Bare::Open(Some(project)),
        "in a project"
    );
    assert_eq!(
        bare(true, None, 2),
        Bare::Open(None),
        "outside one, with projects"
    );
    assert_eq!(bare(true, None, 0), Bare::Nowhere, "outside one, with none");
}

#[test]
fn nowhere_says_what_makes_a_project_and_what_lists_the_verbs() {
    let plain = Screen::from_environment(Environment {
        stdout_is_terminal: false,
        no_color: None,
        term: None,
    });

    assert_eq!(
        nowhere(&plain),
        "This folder has no zaf project.\nzaf setup makes one.\nzaf help lists the verbs.\n"
    );
}
