//! `zaf profile init`: generate `.zaf/profiles/<name>/`, a complete Claude home per profile.
//!
//! Each profile the manifest's roles name gets `settings.json`, `CLAUDE.md` and the
//! `projects/`, `memory/` and `plugins/` folders, ready for `CLAUDE_CONFIG_DIR`.
//! Generation is deterministic (sorted keys, fixed order, no timestamps), so a rerun
//! rewrites only the files whose bytes differ and says which.
//!
//! With a `[pack]`, every profile that carries plugins gets the pack under `plugins/`:
//! copied from `[pack] path`, or installed from the marketplace at `[pack] tag` by
//! `claude plugin install` run through the [`Shell`] port with `CLAUDE_CONFIG_DIR` set
//! to the profile. Its settings enable the pack.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::app::brief::INTERFACE_PAGE;
use crate::domain::Profile;
use crate::manifest::{CommandLine, Effort, Manifest, PackSection, RoleSection};
use crate::ports::{GateVerdict, Shell};

/// The folder under the state folder that holds the profiles.
pub const PROFILES_DIR: &str = "profiles";
/// The project's own rules, appended to the interface page when present, under the state folder.
pub const PROJECT_RULES: &str = "roles/project.md";
/// The folders every profile carries; `plugins` stays empty until the pack is vendored.
pub const PROFILE_DIRS: [&str; 3] = ["projects", "memory", "plugins"];

/// The pack's plugin name.
pub const PACK_PLUGIN: &str = "teams";
/// The marketplace the pack is published in, by its own name.
pub const PACK_MARKETPLACE: &str = "ways";
/// Where the marketplace is, as `claude plugin install --marketplace` takes it.
pub const MARKETPLACE_SOURCE: &str = "ProgrammingCheetah/ways";
/// The variable that points Claude Code at a profile instead of the host's home.
pub const CONFIG_DIR_VAR: &str = "CLAUDE_CONFIG_DIR";
/// The record, under a profile's `plugins/`, of the tag installed from the marketplace,
/// so a rerun with the same tag installs nothing.
pub const INSTALLED_TAG_FILE: &str = ".zaf-pack-tag";

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
    /// The file was in a vendored pack and is no longer in its source.
    Removed,
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
    /// The pack installs run from the marketplace, one line each.
    pub installed: Vec<String>,
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
                    Change::Removed => "removed",
                };
                format!("{verb} {}", file.path.display())
            })
            .collect();
        if self.unchanged() {
            lines.push("nothing changed".to_owned());
        }
        lines.extend(self.installed.iter().cloned());
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
    /// `[pack] path` names no folder.
    #[error("[pack] path {0} is not a folder")]
    NoPack(PathBuf),
    /// The marketplace install failed.
    #[error("`{line}` failed: {output}")]
    Install {
        /// The command line run.
        line: String,
        /// What it said.
        output: String,
    },
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> ProfileError {
    let path = path.to_owned();
    move |source| ProfileError::Io { path, source }
}

/// Generates every profile the manifest's roles name under `<state_dir>/profiles/`.
///
/// `root` is the project root, which `[pack] path` is relative to; `shell` runs the
/// marketplace install for `[pack] tag` and nothing else.
pub fn init(
    root: &Path,
    state_dir: &Path,
    manifest: &Manifest,
    shell: &dyn Shell,
) -> Result<Report, ProfileError> {
    let mut report = Report::default();
    let project = read_project_rules(state_dir)?;
    for profile in named(manifest) {
        let Some(template) = template_for(profile) else {
            report.skipped.push(profile.to_string());
            continue;
        };
        let home = home(state_dir, profile);
        for dir in PROFILE_DIRS {
            let dir = home.join(dir);
            fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        }
        let pack = manifest.pack.as_ref().filter(|_| carries_pack(profile));
        if let Some(pack) = pack {
            vendor(root, state_dir, &home, pack, shell, &mut report)?;
        }
        // After the pack: an install may touch the settings, and the generated bytes must win.
        let settings = settings_for(profile, template, manifest, pack.is_some())?;
        let claude_md = claude_md(project.as_deref());
        for (name, bytes) in [("settings.json", settings), ("CLAUDE.md", claude_md)] {
            record(state_dir, &home.join(name), &bytes, &mut report)?;
        }
        report.profiles.push(profile.to_string());
    }
    Ok(report)
}

/// The profiles the manifest's roles name, sorted, each once.
pub fn named(manifest: &Manifest) -> Vec<Profile> {
    let mut named: Vec<Profile> = manifest.roles.values().map(|role| role.profile).collect();
    named.sort();
    named.dedup();
    named
}

/// The profiles the manifest's roles name that `init` generates a home for.
pub fn generated(manifest: &Manifest) -> Vec<Profile> {
    named(manifest)
        .into_iter()
        .filter(|profile| template_for(*profile).is_some())
        .collect()
}

/// A profile's home: `<state_dir>/profiles/<name>`, what `CLAUDE_CONFIG_DIR` points at.
pub fn home(state_dir: &Path, profile: Profile) -> PathBuf {
    state_dir.join(PROFILES_DIR).join(profile.to_string())
}

/// Whether the profile carries the pack: every generated one but `lean`, which runs plugin-free.
fn carries_pack(profile: Profile) -> bool {
    !matches!(profile, Profile::Lean)
}

/// Writes one generated file when its bytes differ and records it.
fn record(
    state_dir: &Path,
    path: &Path,
    bytes: &[u8],
    report: &mut Report,
) -> Result<(), ProfileError> {
    if let Some(change) = write_if_different(path, bytes)? {
        report.written.push(Written {
            path: relative(state_dir, path),
            change,
        });
    }
    Ok(())
}

fn relative(state_dir: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(state_dir).unwrap_or(path).to_owned()
}

/// Puts the pack under `<home>/plugins/`: a copy of `[pack] path`, else a marketplace install.
fn vendor(
    root: &Path,
    state_dir: &Path,
    home: &Path,
    pack: &PackSection,
    shell: &dyn Shell,
    report: &mut Report,
) -> Result<(), ProfileError> {
    let plugins = home.join("plugins");
    if let Some(source) = pack.dir(root) {
        return mirror(&source, &plugins.join(PACK_PLUGIN), state_dir, report);
    }
    let Some(tag) = &pack.tag else {
        return Ok(());
    };
    let marker = plugins.join(INSTALLED_TAG_FILE);
    if fs::read_to_string(&marker).is_ok_and(|installed| installed.trim() == tag) {
        return Ok(());
    }
    let line = install_line(home, tag);
    let run = shell
        .run(&CommandLine::from(line.clone()))
        .map_err(|error| ProfileError::Install {
            line: line.clone(),
            output: error.to_string(),
        })?;
    if run.verdict != GateVerdict::Passed {
        return Err(ProfileError::Install {
            line,
            output: run.output.trim().to_owned(),
        });
    }
    report.installed.push(format!(
        "installed {PACK_PLUGIN}@{tag} into {}",
        relative(state_dir, home).display()
    ));
    record(state_dir, &marker, format!("{tag}\n").as_bytes(), report)
}

/// The marketplace install of the pack at `tag`, into the profile at `home`.
pub fn install_line(home: &Path, tag: &str) -> String {
    format!(
        "{CONFIG_DIR_VAR}={} claude plugin install {} --marketplace {}",
        single_quote(&home.to_string_lossy()),
        single_quote(&format!("{PACK_PLUGIN}@{tag}")),
        single_quote(MARKETPLACE_SOURCE),
    )
}

/// Wraps text in single quotes for a POSIX shell.
fn single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Makes `target` hold exactly the files of `source`, byte for byte: each file copied when
/// it differs, and each file no longer in the source removed.
fn mirror(
    source: &Path,
    target: &Path,
    state_dir: &Path,
    report: &mut Report,
) -> Result<(), ProfileError> {
    if !source.is_dir() {
        return Err(ProfileError::NoPack(source.to_owned()));
    }
    let wanted = files(source)?;
    for file in &wanted {
        let from = source.join(file);
        let to = target.join(file);
        let bytes = fs::read(&from).map_err(io_error(&from))?;
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(io_error(parent))?;
        }
        record(state_dir, &to, &bytes, report)?;
    }
    if target.is_dir() {
        for file in files(target)? {
            if !wanted.contains(&file) {
                let stale = target.join(&file);
                fs::remove_file(&stale).map_err(io_error(&stale))?;
                report.written.push(Written {
                    path: relative(state_dir, &stale),
                    change: Change::Removed,
                });
            }
        }
    }
    Ok(())
}

/// Every file under `dir`, relative to it, sorted.
fn files(dir: &Path) -> Result<Vec<PathBuf>, ProfileError> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let here = dir.join(&relative);
        for entry in fs::read_dir(&here).map_err(io_error(&here))? {
            let entry = entry.map_err(io_error(&here))?;
            let path = relative.join(entry.file_name());
            if entry.path().is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
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

/// The one tool rule the pack itself adds: the verbs every role runs.
const ZAF_ALLOW: &str = "Bash(zaf *)";

/// The template with the manifest's parts merged in: `zaf`, every `[requires]` tool and the
/// gate commands as allows (so the pack names no tool of its own), the
/// forbidden commands as denies, the model and effort of the roles using the profile, and
/// the pack enabled when the profile carries it. The template's `defaultMode` stays.
fn settings_for(
    profile: Profile,
    template: &str,
    manifest: &Manifest,
    pack: bool,
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
    allow.push(ZAF_ALLOW.to_owned());
    allow.extend(
        manifest
            .requires
            .keys()
            .map(|tool| format!("Bash({tool} *)")),
    );
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
    if pack {
        settings.insert(
            "enabledPlugins".to_owned(),
            json!({ format!("{PACK_PLUGIN}@{PACK_MARKETPLACE}"): true }),
        );
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
