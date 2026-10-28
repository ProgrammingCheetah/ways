//! `zaf profile init`: generate `.zaf/profiles/<name>/`, a complete Claude home per profile.
//!
//! Each profile the manifest's roles name gets `settings.json`, `CLAUDE.md` and the
//! empty `projects/`, `memory/` and `plugins/` folders, ready for `CLAUDE_CONFIG_DIR`.
//! Generation is deterministic (sorted keys, fixed order, no timestamps), so a rerun
//! rewrites only the files whose bytes differ and says which. It touches the file
//! system only: no port is needed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::app::brief::INTERFACE_PAGE;
use crate::domain::Profile;
use crate::manifest::{CommandLine, Effort, Manifest, RoleSection};

/// The folder under the state folder that holds the profiles.
pub const PROFILES_DIR: &str = "profiles";
/// The project's own rules, appended to the interface page when present, under the state folder.
pub const PROJECT_RULES: &str = "roles/project.md";
/// The folders every profile carries; `plugins` stays empty until the pack is vendored.
pub const PROFILE_DIRS: [&str; 3] = ["projects", "memory", "plugins"];

const LEAN_SETTINGS: &str = include_str!("../../../assets/profiles/lean/settings.json");
const BUILD_SETTINGS: &str = include_str!("../../../assets/profiles/build/settings.json");

/// What happened to one generated file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    /// The file was not there.
    Created,
    /// The file was there with other bytes.
    Rewritten,
}

/// One file that was written.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Written {
    /// The file, relative to the state folder.
    pub path: PathBuf,
    /// Created or rewritten.
    pub change: Change,
}

/// What `profile init` did.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Report {
    /// The profiles generated, by name.
    pub profiles: Vec<String>,
    /// The files written; empty when every profile was already in place.
    pub written: Vec<Written>,
    /// Profiles roles name that have no generated home (design and review run on their own settings).
    pub skipped: Vec<String>,
}

impl Report {
    /// Whether nothing was written.
    pub fn unchanged(&self) -> bool {
        self.written.is_empty()
    }

    /// One line per written file, or one line saying nothing changed.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .written
            .iter()
            .map(|file| {
                let verb = match file.change {
                    Change::Created => "created",
                    Change::Rewritten => "rewritten",
                };
                format!("{verb} {}", file.path.display())
            })
            .collect();
        if self.unchanged() {
            lines.push("nothing changed".to_owned());
        }
        lines.extend(
            self.skipped
                .iter()
                .map(|name| format!("skipped {name}: no generated home")),
        );
        lines
    }
}

/// Why profiles could not be generated.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// A file or folder could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The failure.
        #[source]
        source: io::Error,
    },
    /// A profile's settings template is not a JSON object (a defect of the pack).
    #[error("the {0} settings template is not a JSON object")]
    Template(Profile),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> ProfileError {
    let path = path.to_owned();
    move |source| ProfileError::Io { path, source }
}

/// Generates every profile the manifest's roles name under `<state_dir>/profiles/`.
pub fn init(state_dir: &Path, manifest: &Manifest) -> Result<Report, ProfileError> {
    let mut report = Report::default();
    let project = read_project_rules(state_dir)?;
    let mut named: Vec<Profile> = manifest.roles.values().map(|role| role.profile).collect();
    named.sort();
    named.dedup();
    for profile in named {
        let Some(template) = template_for(profile) else {
            report.skipped.push(profile.to_string());
            continue;
        };
        let home = state_dir.join(PROFILES_DIR).join(profile.to_string());
        for dir in PROFILE_DIRS {
            let dir = home.join(dir);
            fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        }
        let settings = settings_for(profile, template, manifest)?;
        let claude_md = claude_md(project.as_deref());
        for (name, bytes) in [("settings.json", settings), ("CLAUDE.md", claude_md)] {
            let path = home.join(name);
            if let Some(change) = write_if_different(&path, &bytes)? {
                let relative = path.strip_prefix(state_dir).unwrap_or(&path);
                report.written.push(Written {
                    path: relative.to_owned(),
                    change,
                });
            }
        }
        report.profiles.push(profile.to_string());
    }
    Ok(report)
}

fn template_for(profile: Profile) -> Option<&'static str> {
    match profile {
        Profile::Lean => Some(LEAN_SETTINGS),
        Profile::Build => Some(BUILD_SETTINGS),
        Profile::Design | Profile::Review => None,
    }
}

fn read_project_rules(state_dir: &Path) -> Result<Option<String>, ProfileError> {
    let path = state_dir.join(PROJECT_RULES);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(&path)(error)),
    }
}

/// The interface page, then the project's rules after one blank line.
fn claude_md(project: Option<&str>) -> Vec<u8> {
    let mut text = INTERFACE_PAGE.trim_end().to_owned();
    text.push('\n');
    if let Some(project) = project.map(str::trim).filter(|rules| !rules.is_empty()) {
        text.push('\n');
        text.push_str(project);
        text.push('\n');
    }
    text.into_bytes()
}

/// The template with the manifest's parts merged in: the gate commands as allows, the
/// forbidden commands as denies, and the model and effort of the roles using the profile.
fn settings_for(
    profile: Profile,
    template: &str,
    manifest: &Manifest,
) -> Result<Vec<u8>, ProfileError> {
    let Ok(Value::Object(mut settings)) = serde_json::from_str::<Value>(template) else {
        return Err(ProfileError::Template(profile));
    };
    let deny = deny_rules(manifest);
    let permissions = settings
        .entry("permissions")
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(permissions) = permissions else {
        return Err(ProfileError::Template(profile));
    };
    let mut allow: Vec<String> = match permissions.get("allow") {
        Some(Value::Array(rules)) => rules
            .iter()
            .filter_map(|rule| rule.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    for command in gate_commands(manifest) {
        allow.push(allow_rule(command));
    }
    let mut seen = Vec::new();
    allow.retain(|rule| {
        let fresh = !seen.contains(rule) && !deny.contains(rule);
        seen.push(rule.clone());
        fresh
    });
    permissions.insert("allow".to_owned(), json!(allow));
    permissions.insert("deny".to_owned(), json!(deny));
    if let Some(role) = lead_role(profile, manifest) {
        settings.insert("model".to_owned(), json!(role.model.as_ref() as &str));
        settings.insert("effortLevel".to_owned(), json!(effort_level(role.effort)));
    }
    let mut text = serde_json::to_string_pretty(&Value::Object(settings))
        .map_err(|_| ProfileError::Template(profile))?;
    text.push('\n');
    Ok(text.into_bytes())
}

/// The role whose model and effort the profile carries: the first by name among the
/// roles that use it (the spawner still passes each role's own model and effort).
fn lead_role(profile: Profile, manifest: &Manifest) -> Option<&RoleSection> {
    manifest.roles.values().find(|role| role.profile == profile)
}

/// `effortLevel` takes no `max`: the highest value it holds is `xhigh`.
fn effort_level(effort: Effort) -> String {
    match effort {
        Effort::Max => Effort::Xhigh.to_string(),
        other => other.to_string(),
    }
}

fn gate_commands(manifest: &Manifest) -> Vec<&CommandLine> {
    manifest
        .gate
        .iter()
        .flat_map(|gate| [Some(&gate.full), gate.light.as_ref(), gate.tests.as_ref()])
        .flatten()
        .collect()
}

/// `{placeholders}` match anything.
fn allow_rule(command: &CommandLine) -> String {
    let mut pattern = String::new();
    let mut in_placeholder = false;
    for character in command.as_ref().trim().chars() {
        match character {
            '{' if !in_placeholder => {
                in_placeholder = true;
                pattern.push('*');
            }
            '}' if in_placeholder => in_placeholder = false,
            _ if in_placeholder => {}
            other => pattern.push(other),
        }
    }
    format!("Bash({pattern})")
}

/// Each forbidden command, bare and with any arguments, sorted.
fn deny_rules(manifest: &Manifest) -> Vec<String> {
    let mut rules: Vec<String> = Vec::new();
    for command in &manifest.forbidden.commands {
        let bare = allow_rule(command);
        let with_args = format!("{} *)", bare.trim_end_matches(')'));
        rules.push(bare);
        rules.push(with_args);
    }
    rules.sort();
    rules.dedup();
    rules
}

/// Writes `bytes` when the file is missing or differs; says which.
fn write_if_different(path: &Path, bytes: &[u8]) -> Result<Option<Change>, ProfileError> {
    let change = match fs::read(path) {
        Ok(existing) if existing == bytes => return Ok(None),
        Ok(_) => Change::Rewritten,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Change::Created,
        Err(error) => return Err(io_error(path)(error)),
    };
    fs::write(path, bytes).map_err(io_error(path))?;
    Ok(Some(change))
}

#[cfg(test)]
mod tests;
