//! The ways repository's own manifest keeps every instance from merging outside
//! `zaf do slice-merge` and `zaf do release`: its `[forbidden]` list holds the merge paths,
//! and the profiles generated from it deny each one, bare and with arguments, for every role.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use zaf::app::profile;
use zaf::manifest::{CommandLine, Manifest};
use zaf::ports::{GateRun, PortError, Shell};

/// The merge paths no instance may take.
const MERGE_PATHS: [&str; 7] = [
    "gh pr merge",
    "zaf do pr-merge",
    "zaf do land",
    "zaf do ship",
    "zaf do --tier machine pr-merge",
    "zaf do --tier machine land",
    "zaf do --tier machine ship",
];

/// The two actions that are allowed to merge, so the list must never name them.
const ALLOWED_MERGES: [&str; 2] = ["zaf do slice-merge", "zaf do release"];

/// The ways repository's root, one folder above the crate.
fn ways_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .to_owned()
}

/// The ways repository's own manifest, as zaf reads it.
fn ways_manifest() -> Manifest {
    Manifest::load_layers(&ways_root(), None).expect("the ways manifest loads")
}

/// A shell that must stay unused: with no `[pack]`, profile generation runs no command.
struct NoShell;

impl Shell for NoShell {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        panic!("profile generation ran a command: {line}");
    }
}

/// The deny rules of `profile`'s settings, as generated from `manifest` into `state`.
fn denies(state: &Path, profile: &str) -> Vec<String> {
    let text = fs::read_to_string(state.join("profiles").join(profile).join("settings.json"))
        .expect("the generated settings");
    let settings: Value = serde_json::from_str(&text).expect("settings parse");
    settings["permissions"]["deny"]
        .as_array()
        .expect("a deny list")
        .iter()
        .map(|rule| rule.as_str().expect("a rule").to_owned())
        .collect()
}

/// Generates the profiles of the ways manifest into a temp state folder, and returns each
/// profile that a role uses with the deny rules it got.
fn generated_denies() -> Vec<(String, Vec<String>)> {
    let mut manifest = ways_manifest();
    // The pack install needs `claude`; the denies do not depend on it.
    manifest.pack = None;
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join(".zaf");
    profile::init(dir.path(), &state, &manifest, &NoShell).expect("profiles generate");
    let used: Vec<String> = profile::generated(&manifest)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(!used.is_empty(), "the ways manifest names a role profile");
    used.into_iter()
        .map(|name| {
            let rules = denies(&state, &name);
            (name, rules)
        })
        .collect()
}

#[test]
fn forbidden_denies_the_merge_paths() {
    let manifest = ways_manifest();
    let listed: Vec<String> = manifest
        .forbidden
        .commands
        .iter()
        .map(ToString::to_string)
        .collect();
    for path in MERGE_PATHS {
        assert!(
            listed.iter().any(|command| command == path),
            "[forbidden] commands lacks `{path}`: {listed:?}"
        );
    }

    // Every instance role runs on a generated profile, so each one must carry every deny.
    let roles: Vec<String> = manifest
        .roles
        .values()
        .map(|role| role.profile.to_string())
        .collect();
    assert!(!roles.is_empty(), "the ways manifest declares roles");
    let generated = generated_denies();
    for role_profile in &roles {
        let (_, rules) = generated
            .iter()
            .find(|(name, _)| name == role_profile)
            .unwrap_or_else(|| panic!("no generated profile {role_profile}"));
        for path in MERGE_PATHS {
            for rule in [format!("Bash({path})"), format!("Bash({path} *)")] {
                assert!(
                    rules.contains(&rule),
                    "profile {role_profile} does not deny {rule}: {rules:?}"
                );
            }
        }
    }
}

#[test]
fn forbidden_leaves_slice_merge_and_release() {
    let manifest = ways_manifest();
    for command in &manifest.forbidden.commands {
        let command = command.to_string();
        for allowed in ALLOWED_MERGES {
            assert!(
                !command.starts_with(allowed) && !allowed.starts_with(&command),
                "[forbidden] names `{command}`, which covers `{allowed}`"
            );
        }
    }

    // No generated deny may cover either action, whether named or by a wider pattern.
    for (profile, rules) in generated_denies() {
        for rule in rules {
            let pattern = rule
                .strip_prefix("Bash(")
                .and_then(|rest| rest.strip_suffix(')'))
                .expect("a Bash rule");
            for allowed in ALLOWED_MERGES {
                let covers = match pattern.strip_suffix('*') {
                    Some(prefix) => allowed.starts_with(prefix),
                    None => allowed == pattern,
                };
                assert!(
                    !covers,
                    "profile {profile} denies `{rule}`, covering `{allowed}`"
                );
            }
        }
    }
}
