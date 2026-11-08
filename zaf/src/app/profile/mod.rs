//! `zaf profile init`: generate `.zaf/profiles/<name>/`, a complete Claude home per profile.
//!
//! Each profile the manifest's roles name gets `settings.json`, `CLAUDE.md` and the
//! `projects/`, `memory/` and `plugins/` folders, ready for `CLAUDE_CONFIG_DIR`.
//! Generation is deterministic (sorted keys, fixed order, no timestamps), so a rerun
//! rewrites only the files whose bytes differ and says which.
//!
//! With a `[pack]`, every profile that carries plugins gets the pack registered through
//! Claude's own plugin commands, run through the [`Shell`] port with `CLAUDE_CONFIG_DIR`
//! set to the profile (a raw copy under `plugins/` is invisible to Claude):
//! - `[pack] path`: the project root is added as a marketplace (its
//!   `.claude-plugin/marketplace.json` names the marketplace and lists the pack) and the
//!   pack is installed from it. What is already added or installed is read from
//!   `claude plugin list` and `claude plugin marketplace list`, and issues no command, unless
//!   the pack's `plugin.json` now declares another version than the listing shows: then
//!   `claude plugin marketplace update` and `claude plugin update` bring the profile to it.
//! - `[pack] tag`: the pack is installed from the marketplace on GitHub at that tag.
//!
//! The two share the install step. The profile's settings enable the pack.
//!
//! The login is shared, never copied: a profile's `.credentials.json` is a symlink to the one
//! file in the Claude home, so a rotated refresh token reaches every profile at once. The
//! Claude home is `$HOME/.claude` and never `CLAUDE_CONFIG_DIR`, which in a spawned instance
//! names a profile. A file already at the profile's path is left alone for doctor to report.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::app::actions::forbidden_action;
use crate::app::brief::INTERFACE_PAGE;
use crate::composition::HOME_VAR;
use crate::domain::Profile;
use crate::manifest::{CommandLine, Effort, Manifest, PackSection, RoleSection, ToolName};
use crate::ports::{GateVerdict, Shell};

/// The folder under the state folder that holds the profiles.
pub const PROFILES_DIR: &str = "profiles";
/// The project's own rules, appended to the interface page when present, under the state folder.
pub const PROJECT_RULES: &str = "roles/project.md";
/// The folders every profile carries; `plugins` is Claude's own, filled by the pack install.
pub const PROFILE_DIRS: [&str; 3] = ["projects", "memory", "plugins"];

/// The pack's plugin name when it is installed from a tag.
pub const PACK_PLUGIN: &str = "teams";
/// The marketplace the pack is published in, by its own name.
pub const PACK_MARKETPLACE: &str = "ways";
/// Where the marketplace is, as `claude plugin install --marketplace` takes it.
pub const MARKETPLACE_SOURCE: &str = "ProgrammingCheetah/ways";
/// The marketplace file at the project root, which names the marketplace and lists the pack.
pub const MARKETPLACE_FILE: &str = ".claude-plugin/marketplace.json";
/// The variable that points Claude Code at a profile instead of the host's home.
pub const CONFIG_DIR_VAR: &str = "CLAUDE_CONFIG_DIR";
/// The record, under a profile's `plugins/`, of the tag installed from the marketplace,
/// so a rerun with the same tag installs nothing.
pub const INSTALLED_TAG_FILE: &str = ".zaf-pack-tag";
/// A plugin's own manifest, under its source folder.
const PLUGIN_FILE: &str = ".claude-plugin/plugin.json";
/// What [`listed_version`] says of a listed plugin that shows no `Version:` line.
const UNKNOWN_VERSION: &str = "unknown";
/// The file a Claude home keeps its login in.
pub const CREDENTIALS_FILE: &str = ".credentials.json";
/// The Claude home's folder under the user's home.
const CLAUDE_HOME_DIR: &str = ".claude";

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
    /// The pack installs run, one line each.
    pub installed: Vec<String>,
}

impl Report {
    /// Whether nothing was written and nothing was installed.
    pub fn unchanged(&self) -> bool {
        self.written.is_empty() && self.installed.is_empty()
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
    /// The project's marketplace file cannot name the pack `[pack] path` points at.
    #[error("{path}: {reason}")]
    Marketplace {
        /// The marketplace file.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// A `claude plugin` command failed.
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
/// `root` is the project root, which `[pack] path` is relative to and which is added as the
/// marketplace; `shell` runs the `claude plugin` commands that register the pack and
/// nothing else. Each profile's login is linked to the Claude home's, see [`link_credentials`].
pub fn init(
    root: &Path,
    state_dir: &Path,
    manifest: &Manifest,
    shell: &dyn Shell,
) -> Result<Report, ProfileError> {
    init_with(root, state_dir, manifest, shell, claude_home().as_deref())
}

/// [`init`] against a given Claude home; `None`, when there is none, links nothing.
fn init_with(
    root: &Path,
    state_dir: &Path,
    manifest: &Manifest,
    shell: &dyn Shell,
    claude_home: Option<&Path>,
) -> Result<Report, ProfileError> {
    let mut report = Report::default();
    let project = read_project_rules(state_dir)?;
    let pack_ref = match &manifest.pack {
        Some(pack) => pack_ref(root, pack)?,
        None => None,
    };
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
        // Before the pack: `claude plugin` runs inside this profile, under its login.
        if let Some(claude_home) = claude_home {
            let link = home.join(CREDENTIALS_FILE);
            if link_credentials(&home, claude_home).map_err(io_error(&link))? {
                report.written.push(Written {
                    path: relative(state_dir, &link),
                    change: Change::Created,
                });
            }
        }
        let pack = pack_ref.as_ref().filter(|_| carries_pack(profile));
        if let (Some(pack_section), Some(pack)) = (&manifest.pack, pack) {
            register(
                root,
                state_dir,
                &home,
                pack_section,
                pack,
                shell,
                &mut report,
            )?;
        }
        // After the pack: an install may touch the settings, and the generated bytes must win.
        let settings = settings_for(profile, template, manifest, pack)?;
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

/// The host's Claude home, `$HOME/.claude`; `None` without a usable `HOME`.
pub fn claude_home() -> Option<PathBuf> {
    claude_home_under(env::var_os(HOME_VAR))
}

/// The Claude home under `home`, which must be absolute: a symlink holding a relative path
/// would resolve against the profile's folder and point somewhere else.
fn claude_home_under(home: Option<OsString>) -> Option<PathBuf> {
    let home = PathBuf::from(home?);
    home.is_absolute().then(|| home.join(CLAUDE_HOME_DIR))
}

/// Makes the `.credentials.json` of the profile at `home` a symlink to the one in
/// `claude_home`, and says whether it made the link.
///
/// Makes nothing when the Claude home holds no such file, and never replaces what is already
/// at the profile's path, be it a file or a link.
pub fn link_credentials(home: &Path, claude_home: &Path) -> io::Result<bool> {
    let source = claude_home.join(CREDENTIALS_FILE);
    if !source.is_file() {
        return Ok(false);
    }
    match symlink(&source, home.join(CREDENTIALS_FILE)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

/// Whether the symlink at `link` leads to the credentials file of `claude_home`.
///
/// By its written target, so a link survives the Claude home being logged out, or by where
/// both resolve, so a link through another spelling of the path still counts.
pub fn links_to_claude_home(link: &Path, claude_home: &Path) -> bool {
    let source = claude_home.join(CREDENTIALS_FILE);
    fs::read_link(link).is_ok_and(|target| target == source)
        || matches!(
            (fs::canonicalize(link), fs::canonicalize(&source)),
            (Ok(resolved), Ok(wanted)) if resolved == wanted
        )
}

/// Whether the profile carries the pack: every generated one but `lean`, which runs plugin-free.
pub fn carries_pack(profile: Profile) -> bool {
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

/// A pack as Claude names it: `<plugin>@<marketplace>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRef {
    /// The plugin's name.
    pub plugin: String,
    /// The marketplace's name.
    pub marketplace: String,
}

impl fmt::Display for PackRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}", self.plugin, self.marketplace)
    }
}

/// The pack a manifest's `[pack]` names, as Claude knows it; `None` when it names no source.
///
/// A `path` is looked up in the project root's marketplace file: the marketplace is that
/// file's `name` and the plugin is the entry whose `source` is the path. A `tag` is the
/// pack published as [`PACK_PLUGIN`] in [`PACK_MARKETPLACE`].
pub fn pack_ref(root: &Path, pack: &PackSection) -> Result<Option<PackRef>, ProfileError> {
    if let Some(path) = &pack.path {
        return marketplace_entry(root, path).map(Some);
    }
    Ok(pack.tag.as_ref().map(|_| PackRef {
        plugin: PACK_PLUGIN.to_owned(),
        marketplace: PACK_MARKETPLACE.to_owned(),
    }))
}

/// Reads the project root's marketplace file and finds the plugin whose source is `path`.
fn marketplace_entry(root: &Path, path: &Path) -> Result<PackRef, ProfileError> {
    let file = root.join(MARKETPLACE_FILE);
    let bad = |reason: String| ProfileError::Marketplace {
        path: file.clone(),
        reason,
    };
    let text = fs::read_to_string(&file).map_err(|error| bad(error.to_string()))?;
    let Ok(Value::Object(catalog)) = serde_json::from_str::<Value>(&text) else {
        return Err(bad("not a JSON object".to_owned()));
    };
    let Some(marketplace) = catalog.get("name").and_then(Value::as_str) else {
        return Err(bad("names no marketplace (no `name`)".to_owned()));
    };
    let spelled = path.to_string_lossy();
    let wanted = source_key(&spelled);
    let plugin = catalog
        .get("plugins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|entry| {
            entry
                .get("source")
                .and_then(Value::as_str)
                .is_some_and(|source| source_key(source) == wanted)
        })
        .and_then(|entry| entry.get("name").and_then(Value::as_str));
    match plugin {
        Some(plugin) => Ok(PackRef {
            plugin: plugin.to_owned(),
            marketplace: marketplace.to_owned(),
        }),
        None => Err(bad(format!(
            "lists no plugin with source {}",
            path.display()
        ))),
    }
}

/// A plugin source path without its leading `./` and trailing `/`, so spellings compare equal.
fn source_key(source: &str) -> &str {
    let source = source.trim();
    source
        .strip_prefix("./")
        .unwrap_or(source)
        .trim_end_matches('/')
}

/// Registers the pack in the profile at `home`, issuing no command for what is already there.
fn register(
    root: &Path,
    state_dir: &Path,
    home: &Path,
    pack: &PackSection,
    pack_ref: &PackRef,
    shell: &dyn Shell,
    report: &mut Report,
) -> Result<(), ProfileError> {
    if pack.path.is_some() {
        let id = pack_ref.to_string();
        if let Some(have) = listed_version(&run(shell, &plugin_line(home, "list"))?, &id) {
            let wanted = pack
                .path
                .as_deref()
                .and_then(|path| source_version(root, path));
            return match wanted {
                Some(wanted) if have != UNKNOWN_VERSION && have != wanted => {
                    update(shell, state_dir, home, pack_ref, &wanted, report)
                }
                _ => Ok(()),
            };
        }
        let marketplaces = run(shell, &plugin_line(home, "marketplace list"))?;
        if !lists_marketplace(&marketplaces, &pack_ref.marketplace) {
            let add = format!(
                "{} {}",
                plugin_line(home, "marketplace add"),
                single_quote(&root.to_string_lossy())
            );
            run(shell, &add)?;
        }
        let line = install_line(home, &id, None);
        return install(shell, state_dir, home, &id, &line, report);
    }
    let Some(tag) = &pack.tag else {
        return Ok(());
    };
    let marker = home.join("plugins").join(INSTALLED_TAG_FILE);
    if fs::read_to_string(&marker).is_ok_and(|installed| installed.trim() == tag) {
        return Ok(());
    }
    let spec = format!("{PACK_PLUGIN}@{tag}");
    let line = install_line(home, &spec, Some(MARKETPLACE_SOURCE));
    install(shell, state_dir, home, &spec, &line, report)?;
    record(state_dir, &marker, format!("{tag}\n").as_bytes(), report)
}

/// The install step both sources share: runs `line` and reports `spec` installed.
fn install(
    shell: &dyn Shell,
    state_dir: &Path,
    home: &Path,
    spec: &str,
    line: &str,
    report: &mut Report,
) -> Result<(), ProfileError> {
    run(shell, line)?;
    report.installed.push(format!(
        "installed {spec} into {}",
        relative(state_dir, home).display()
    ));
    Ok(())
}

/// The version the pack's own `.claude-plugin/plugin.json` declares, under `root` at `path`;
/// `None` when the file is missing, unreadable or declares none (the pack is not versioned).
fn source_version(root: &Path, path: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join(path).join(PLUGIN_FILE)).ok()?;
    let Value::Object(plugin) = serde_json::from_str::<Value>(&text).ok()? else {
        return None;
    };
    let version = plugin.get("version")?.as_str()?.trim();
    (!version.is_empty()).then(|| version.to_owned())
}

/// Brings the pack in the profile at `home` to the version the source holds: refreshes the
/// marketplace, then updates the plugin, so the profile holds `wanted`.
fn update(
    shell: &dyn Shell,
    state_dir: &Path,
    home: &Path,
    pack_ref: &PackRef,
    wanted: &str,
    report: &mut Report,
) -> Result<(), ProfileError> {
    let marketplace = format!(
        "{} {}",
        plugin_line(home, "marketplace update"),
        single_quote(&pack_ref.marketplace)
    );
    run(shell, &marketplace)?;
    let id = pack_ref.to_string();
    let line = format!("{} {}", plugin_line(home, "update"), single_quote(&id));
    run(shell, &line)?;
    report.installed.push(format!(
        "updated {id} to {wanted} in {}",
        relative(state_dir, home).display()
    ));
    Ok(())
}

/// Runs one line through the shell; its output when it passes, an error naming it when not.
fn run(shell: &dyn Shell, line: &str) -> Result<String, ProfileError> {
    let ran = shell
        .run(&CommandLine::from(line.to_owned()))
        .map_err(|error| ProfileError::Install {
            line: line.to_owned(),
            output: error.to_string(),
        })?;
    if ran.verdict != GateVerdict::Passed {
        return Err(ProfileError::Install {
            line: line.to_owned(),
            output: ran.output.trim().to_owned(),
        });
    }
    Ok(ran.output)
}

/// `claude plugin <subcommand>` run against the profile at `home`, without its arguments.
fn plugin_line(home: &Path, subcommand: &str) -> String {
    format!(
        "{CONFIG_DIR_VAR}={} claude plugin {subcommand}",
        single_quote(&home.to_string_lossy()),
    )
}

/// `claude plugin install <spec>` into the profile at `home`, from `marketplace` on GitHub
/// when one is given, else from a marketplace the profile already has.
fn install_line(home: &Path, spec: &str, marketplace: Option<&str>) -> String {
    let mut line = format!("{} {}", plugin_line(home, "install"), single_quote(spec));
    if let Some(marketplace) = marketplace {
        line.push_str(" --marketplace ");
        line.push_str(&single_quote(marketplace));
    }
    line
}

/// Wraps text in single quotes for a POSIX shell.
fn single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The version `claude plugin list` shows for `id` (`plugin@marketplace`); `None` when `id`
/// is not listed, `unknown` when it is listed without a `Version:` line.
///
/// An entry is its header line (the line with `id` as a word) and the lines after it up to
/// a blank line or the next entry's header.
pub fn listed_version(listing: &str, id: &str) -> Option<String> {
    let mut lines = listing.lines();
    lines.find(|line| line.split_whitespace().any(|word| bare(word) == id))?;
    for line in lines {
        if line.trim().is_empty() || is_header(line) {
            break;
        }
        let version = line
            .trim()
            .split_once(':')
            .filter(|(key, _)| key.trim().eq_ignore_ascii_case("version"))
            .and_then(|(_, value)| value.split_whitespace().next());
        if let Some(version) = version {
            return Some(bare(version).to_owned());
        }
    }
    Some(UNKNOWN_VERSION.to_owned())
}

/// Whether `claude plugin marketplace list` shows a marketplace called `name`.
fn lists_marketplace(listing: &str, name: &str) -> bool {
    listing
        .lines()
        .any(|line| line.split_whitespace().any(|word| bare(word) == name))
}

/// A header line names a plugin: its first word, past any bullet, holds an `@` and no `:`.
fn is_header(line: &str) -> bool {
    line.split_whitespace()
        .map(bare)
        .find(|word| word.chars().any(char::is_alphanumeric))
        .is_some_and(|word| word.contains('@') && !word.contains(':'))
}

/// A word without the quotes, commas and brackets a listing wraps it in.
fn bare(word: &str) -> &str {
    word.trim_matches(|ch: char| matches!(ch, '"' | '\'' | ',' | '(' | ')' | '[' | ']' | '`'))
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

/// The read-only shell basics every profile allows, one `Bash(<word> *)` rule each, so a
/// programmer never stops on a listing, a loop or a search. `sh -c` is not here: a shell
/// run from a string can write. Neither are `env`, `find`, `sed` and `awk`: `env <cmd>` runs
/// anything, `find -exec`/`-delete` and `sed -i` write, and `awk` can call `system()`.
pub const READ_ONLY: [&str; 22] = [
    "ls", "cat", "head", "tail", "wc", "diff", "rg", "grep", "fd", "pwd", "echo", "printf", "for",
    "while", "test", "sort", "uniq", "cut", "stat", "du", "which", "printenv",
];

/// The `git` read verbs a colocated repo (a manifest that requires `jj`) allows besides `jj`.
/// A writing verb (`push`, `commit`, `checkout`, `reset`, ...) is never in this list.
pub const GIT_READ_ONLY: [&str; 5] = ["log", "show", "diff", "status", "blame"];

/// The tool whose requirement makes the repo colocated, so `git` reads are allowed.
const COLOCATED_TOOL: &str = "jj";

/// The fixed read-only allow rules: the shell basics, and the `git` read verbs when the
/// manifest requires `jj`.
fn read_only_rules(manifest: &Manifest) -> Vec<String> {
    let mut rules: Vec<String> = READ_ONLY
        .iter()
        .map(|word| format!("Bash({word} *)"))
        .collect();
    if manifest
        .requires
        .contains_key(&ToolName::from(COLOCATED_TOOL))
    {
        rules.extend(
            GIT_READ_ONLY
                .iter()
                .map(|verb| format!("Bash(git {verb} *)")),
        );
    }
    rules
}

/// The template with the manifest's parts merged in: `zaf`, every `[requires]` tool and the
/// gate commands as allows (so the pack names no tool of its own), the fixed read-only
/// rules ([`READ_ONLY`], and [`GIT_READ_ONLY`] in a colocated repo), the
/// forbidden commands as denies, the model and effort of the roles using the profile, and
/// the pack enabled when the profile carries it. The template's `defaultMode` stays.
fn settings_for(
    profile: Profile,
    template: &str,
    manifest: &Manifest,
    pack: Option<&PackRef>,
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
        allow.extend(tool_rules(command));
    }
    allow.extend(read_only_rules(manifest));
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
    if let Some(pack) = pack {
        settings.insert(
            "enabledPlugins".to_owned(),
            json!({ pack.to_string(): true }),
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

/// Words that run no tool of their own: the directory and environment words and shell builtins.
const NO_TOOL: [&str; 24] = [
    "cd", "env", "export", "set", "unset", "source", ".", "exit", "true", "false", "test", "[",
    "[[", "echo", "printf", "pwd", "eval", "exec", "read", "return", "shift", "trap", "umask",
    "builtin",
];

/// Shells and wrapper tools that run whatever follows them. A bare `Bash(<one of these> *)`
/// rule would let a programmer run anything, so a segment starting with one gets no tool rule:
/// only [`allow_rule`]'s exact line covers it.
const SHELL_OR_WRAPPER: [&str; 12] = [
    "sh", "bash", "zsh", "dash", "env", "xargs", "nohup", "timeout", "time", "nice", "sudo", "doas",
];

/// Whether `tool`, by its file name (`/bin/sh` is `sh`), is a shell or wrapper tool.
fn is_shell_or_wrapper(tool: &str) -> bool {
    let name = tool.rsplit('/').next().unwrap_or(tool);
    SHELL_OR_WRAPPER.contains(&name)
}

/// One `Bash(<tool> *)` rule per tool the command invokes, in order of appearance: the first
/// word of each segment split on `&&`, `||`, `;` and `|` (`NAME=value` words aside), so a gate
/// tool is allowed from any directory, not only as the verbatim line from the root. A segment
/// that starts with a shell or wrapper tool ([`SHELL_OR_WRAPPER`]) yields no rule.
fn tool_rules(command: &CommandLine) -> Vec<String> {
    command
        .as_ref()
        .split(['&', '|', ';', '\n'])
        .filter_map(|segment| {
            let tool = segment
                .split_whitespace()
                .find(|word| !word.contains('='))?;
            let named = !tool.starts_with(['{', '$', '(', '"', '\'']);
            (named && !NO_TOOL.contains(&tool) && !is_shell_or_wrapper(tool)).then_some(tool)
        })
        .map(|tool| format!("Bash({tool} *)"))
        .collect()
}

/// Where flags may stand in a `zaf do <name>` line: before `do`, after it, both or neither.
/// Every other forbidden entry gets the same forms between its tool and its subcommand.
/// A permission pattern's `*` matches any text, spaces included, so `-*` is a flag and whatever
/// it takes (`--tier machine`, `--tier=machine`, `--json`).
const DO_FORMS: [&str; 4] = ["zaf do", "zaf -* do", "zaf do -*", "zaf -* do -*"];

/// Each tool's escape hatches: subcommands of a tool the profile allows with `Bash(<tool> *)`
/// that run any program or rewrite the tool's own config. Denied whenever the tool is allowed,
/// from `[requires]` or from a gate line; a tool the profile does not allow adds none.
const HATCHES: [(&str, &[&str]); 3] = [
    ("jj", &["util", "config set", "config edit"]),
    ("gh", &["alias", "extension"]),
    ("yarn", &["exec", "node", "dlx"]),
];

/// The tools whose config flag can define an alias, so the flag is denied anywhere on the line.
const CONFIG_FLAG_TOOLS: [&str; 1] = ["jj"];

/// The four deny rules of `<tool> <sub...>`: bare, with arguments, and each with flags between
/// the tool and the subcommand (`jj --no-pager git push`, `gh -R owner/repo pr merge 12`).
fn flag_forms(tool: &str, sub: &str) -> [String; 4] {
    [
        format!("Bash({tool} {sub})"),
        format!("Bash({tool} {sub} *)"),
        format!("Bash({tool} -* {sub})"),
        format!("Bash({tool} -* {sub} *)"),
    ]
}

/// The tools the profile allows with a `Bash(<tool> *)` rule: every `[requires]` tool and every
/// tool a gate line invokes.
fn allowed_tools(manifest: &Manifest) -> Vec<String> {
    let mut tools: Vec<String> = manifest.requires.keys().map(ToString::to_string).collect();
    for command in gate_commands(manifest) {
        for rule in tool_rules(command) {
            if let Some(tool) = rule
                .strip_prefix("Bash(")
                .and_then(|rest| rest.strip_suffix(" *)"))
            {
                tools.push(tool.to_owned());
            }
        }
    }
    tools
}

/// Each forbidden command, bare and with any arguments, and again with flags between its tool
/// and its subcommand, plus the escape hatches ([`HATCHES`]) of every allowed tool, sorted. A
/// forbidden `zaf do <name>` is denied again with flags before `do` or before the name
/// (`zaf --more do land 12`, `zaf do --json land 12`, `zaf do --tier=machine land 12`), so a
/// flag cannot walk round it.
fn deny_rules(manifest: &Manifest) -> Vec<String> {
    let mut rules: Vec<String> = Vec::new();
    for command in &manifest.forbidden.commands {
        let bare = allow_rule(command);
        let with_args = format!("{} *)", bare.trim_end_matches(')'));
        let line = bare
            .strip_prefix("Bash(")
            .and_then(|rest| rest.strip_suffix(')'))
            .unwrap_or_default()
            .to_owned();
        rules.push(bare);
        rules.push(with_args);
        if let Some((tool, sub)) = line.split_once(' ')
            && !sub.trim().is_empty()
        {
            rules.extend(flag_forms(tool, sub.trim()));
        }
        if let Some(name) = forbidden_action(command.as_ref()) {
            for form in DO_FORMS {
                rules.push(format!("Bash({form} {name})"));
                rules.push(format!("Bash({form} {name} *)"));
            }
        }
    }
    let allowed = allowed_tools(manifest);
    for (tool, subs) in HATCHES {
        if !allowed.iter().any(|allowed| allowed == tool) {
            continue;
        }
        for sub in subs {
            rules.extend(flag_forms(tool, sub));
        }
        if CONFIG_FLAG_TOOLS.contains(&tool) {
            rules.push(format!("Bash({tool} *--config*)"));
        }
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
