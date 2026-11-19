use super::*;

fn tool_check(tool: &str, status: ToolStatus) -> Check {
    Check::Tool(ToolCheck {
        tool: tool.to_owned(),
        wanted: "*".to_owned(),
        status,
    })
}

fn failed(what: &str, detail: &str) -> BackendCheck {
    BackendCheck {
        what: what.to_owned(),
        ok: false,
        detail: detail.to_owned(),
    }
}

#[test]
fn a_missing_known_tool_gives_its_page_and_why() {
    let found = fixes(&[tool_check("gh", ToolStatus::Missing)]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].why.as_str(), "zaf opens pull requests through gh");
    assert_eq!(
        found[0].command.as_ref().map(Command::as_str),
        Some("https://cli.github.com/")
    );
    assert!(found[0].problem.as_str().contains("`gh`"));
}

#[test]
fn every_known_tool_has_a_why_and_an_https_page() {
    for known in KNOWN_TOOLS {
        let found = fixes(&[tool_check(known.tool, ToolStatus::Missing)]);
        assert_eq!(found[0].why.as_str(), known.why);
        assert!(known.page.starts_with("https://"), "{}", known.tool);
    }
}

#[test]
fn a_too_old_tool_gives_the_same_page() {
    let old = ToolStatus::TooOld {
        found: "2.1.0".to_owned(),
        wanted: ">=2.2".to_owned(),
    };
    let found = fixes(&[tool_check("git", old)]);
    assert!(found[0].problem.as_str().contains("2.1.0"));
    assert_eq!(
        found[0].command.as_ref().map(Command::as_str),
        Some("https://git-scm.com/install/")
    );
}

#[test]
fn an_unknown_tool_names_the_manifest() {
    let found = fixes(&[tool_check("cargo", ToolStatus::Missing)]);
    assert_eq!(
        found[0].why.as_str(),
        "install `cargo`: your `.zaf.toml` asks for it"
    );
    assert_eq!(found[0].command, None);
}

#[test]
fn github_sign_in_gives_gh_auth_login() {
    let found = fixes(&[Check::Backend {
        backend: Backend::Github,
        check: failed("token", "not signed in"),
    }]);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].command.as_ref().map(Command::as_str),
        Some("gh auth login")
    );
}

#[test]
fn forgejo_names_the_token_variable_never_its_value() {
    // The value is set, and still no check holds it: a fix can only name the variable.
    // SAFETY: the variable is this test's own and nothing else reads it.
    unsafe { std::env::set_var("ZAF_FIXES_TEST_TOKEN", "s3cr3t-value") };
    let found = fixes(&[Check::Backend {
        backend: Backend::Forgejo {
            token_env: "FORGEJO_TOKEN".to_owned(),
        },
        check: failed("token", "401 unauthorized"),
    }]);
    assert_eq!(
        found[0].command.as_ref().map(Command::as_str),
        Some("export FORGEJO_TOKEN=…")
    );
    assert!(found[0].why.as_str().contains("FORGEJO_TOKEN"));
    let every_word = format!("{found:?}");
    assert!(!every_word.contains("s3cr3t-value"), "{every_word}");

    let other = fixes(&[Check::Backend {
        backend: Backend::Forgejo {
            token_env: "WORK_TOKEN".to_owned(),
        },
        check: failed("instance", "no answer"),
    }]);
    assert_eq!(
        other[0].command.as_ref().map(Command::as_str),
        Some("export WORK_TOKEN=…")
    );
}

#[test]
fn a_stale_pack_gives_its_fix() {
    let found = fixes(&[Check::PackRefused(
        "pack teams is 2.0.3, manifest asks teams-v2.1.0".to_owned(),
    )]);
    assert_eq!(found.len(), 1);
    assert!(found[0].problem.as_str().contains("teams-v2.1.0"));
    assert!(found[0].why.as_str().contains("`[pack]` agree"));

    let profile = fixes(&[Check::Pack(PackCheck {
        profile: "build".to_owned(),
        pack: Some("teams@ways".to_owned()),
        version: Some("2.0.0".to_owned()),
        source_version: Some("2.1.0".to_owned()),
        ok: false,
    })]);
    assert_eq!(
        profile[0].command.as_ref().map(Command::as_str),
        Some("zaf profile init")
    );
}

#[test]
fn a_folder_that_cannot_be_made_gives_the_folder_and_the_command() {
    let found = fixes(&[Check::Folder(PathCheck {
        what: "workspaces dir",
        path: "/work spaces/z".into(),
        ok: false,
    })]);
    assert!(found[0].problem.as_str().contains("/work spaces/z"));
    assert_eq!(
        found[0].command.as_ref().map(Command::as_str),
        Some("mkdir -p '/work spaces/z'")
    );
}

#[test]
fn green_checks_give_no_fix() {
    let checks = [
        tool_check(
            "git",
            ToolStatus::Ok {
                found: "2.50.0".to_owned(),
            },
        ),
        tool_check(
            "gh",
            ToolStatus::NotNeeded {
                found: "missing".to_owned(),
            },
        ),
        Check::Backend {
            backend: Backend::Github,
            check: BackendCheck {
                what: "token".to_owned(),
                ok: true,
                detail: "fine".to_owned(),
            },
        },
        Check::Pack(PackCheck {
            profile: "build".to_owned(),
            pack: None,
            version: None,
            source_version: None,
            ok: true,
        }),
        Check::Folder(PathCheck {
            what: "workspaces dir",
            path: "/w".into(),
            ok: true,
        }),
    ];
    assert_eq!(fixes(&checks), Vec::<Fix>::new());
}
