//! What the Engineer has to do after `zaf setup`: every failing check becomes a [`Fix`] that names
//! the problem, the command or page that fixes it, and why zaf needs it.
//!
//! [`fixes`] is pure: it reads [`Check`]s (what setup and doctor found) and nothing else. A check
//! that passes gives no fix. A forge or board token is only ever named by its variable, never read:
//! no check here carries a value, so no fix can print one.
//!
//! A missing or too-old tool is looked up in [`KNOWN_TOOLS`], the tools zaf knows why it needs
//! and where to install. A tool not in the table says that the manifest asks for it. There is
//! no package-manager command per system: a package missing from one distribution, a Mac with no
//! Homebrew.

use std::path::Path;

use crate::app::doctor::{
    BackendCheck, PackCheck, PathCheck, ProfileMcpCheck, ToolCheck, ToolStatus,
};
use crate::domain::screen::{Command, Fix, Text};

/// One tool zaf knows: why it needs it and the page that says how to install it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownTool {
    /// The tool's name, as `[requires]` writes it.
    pub tool: &'static str,
    /// What zaf does with it.
    pub why: &'static str,
    /// Where to install it.
    pub page: &'static str,
}

/// The tools zaf keeps a why and an install page for. A page is opened and checked when it is
/// written here; none is asserted from memory.
pub const KNOWN_TOOLS: [KnownTool; 6] = [
    KnownTool {
        tool: "git",
        why: "zaf reads and publishes your work through git",
        page: "https://git-scm.com/install/",
    },
    KnownTool {
        tool: "jj",
        why: "zaf keeps each task in its own jj workspace",
        page: "https://jj-vcs.github.io/jj/latest/install-and-setup/",
    },
    KnownTool {
        tool: "gh",
        why: "zaf opens pull requests through gh",
        page: "https://cli.github.com/",
    },
    KnownTool {
        tool: "claude",
        why: "zaf runs its models through Claude Code",
        page: "https://docs.claude.com/en/docs/claude-code/setup",
    },
    KnownTool {
        tool: "node",
        why: "zaf's hub page is built with node",
        page: "https://nodejs.org/en/download",
    },
    KnownTool {
        tool: "npm",
        why: "zaf installs the hub page's packages with npm",
        page: "https://docs.npmjs.com/downloading-and-installing-node-js-and-npm",
    },
];

/// The table's entry for `tool`.
fn known(tool: &str) -> Option<&'static KnownTool> {
    KNOWN_TOOLS.iter().find(|known| known.tool == tool)
}

/// The backend a board or a forge is reached through, and so what signs it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// GitHub, signed in with `gh`.
    Github,
    /// Forgejo, signed in by the token in the variable the manifest names.
    Forgejo {
        /// The variable's name, never its value.
        token_env: String,
    },
}

/// One result setup knows, of every kind that can turn into a [`Fix`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// A `[requires]` tool, checked.
    Tool(ToolCheck),
    /// What a board's or a forge's backend answered.
    Backend {
        /// How the board or the forge signs in.
        backend: Backend,
        /// What it answered.
        check: BackendCheck,
    },
    /// The pack is not at the version the manifest's `[pack]` asks: the profiles could not be
    /// generated. Holds what the refusal said.
    PackRefused(String),
    /// A generated profile's pack, checked.
    Pack(PackCheck),
    /// A generated profile's MCP server and settings, checked.
    ProfileMcp(ProfileMcpCheck),
    /// A folder the manifest names, checked.
    Folder(PathCheck),
    /// The design repo could not be cloned.
    CloneFailed {
        /// The repository, as `owner/name`.
        repo: String,
        /// Where it was to go.
        dir: String,
        /// What the clone said.
        said: String,
    },
    /// `.zaf/.gitignore` holds lines of the Engineer's own, so the allow-list is theirs to merge.
    IgnoreForeign {
        /// The lines to merge in.
        lines: Vec<String>,
        /// Where the file is, relative to the project.
        file: String,
    },
    /// The project's own `.gitignore` ignores the whole `.zaf/` folder.
    RootIgnoresState {
        /// The folder's name, as the ignore line writes it.
        folder: String,
    },
}

/// A fix for every failing check, in the order given; a check that passes gives none.
///
/// Interface: pub fn fixes(checks: &[<setup's check results>]) -> Vec<Fix>
/// (the results are [`Check`]).
pub fn fixes(checks: &[Check]) -> Vec<Fix> {
    checks.iter().filter_map(fix).collect()
}

fn fix(check: &Check) -> Option<Fix> {
    match check {
        Check::Tool(check) => tool(check),
        Check::Backend { backend, check } => backend_fix(backend, check),
        Check::PackRefused(said) => Some(Fix {
            problem: Text::from(format!(
                "the pack and the manifest's `[pack]` disagree, so the profiles cannot be generated (it said: {said})"
            )),
            command: None,
            why: Text::from(
                "make the pack and the manifest's `[pack]` agree: zaf runs its models in profiles built from the pack",
            ),
        }),
        Check::Pack(check) if !check.ok => Some(Fix {
            problem: Text::from(format!(
                "the pack in profile `{}` is not at the version the source declares",
                check.profile
            )),
            command: Some(Command::from("zaf profile init")),
            why: Text::from("zaf runs its models in profiles that carry the pack"),
        }),
        Check::Pack(_) => None,
        Check::ProfileMcp(check) if !check.ok => Some(Fix {
            problem: Text::from(format!(
                "profile `{}` lacks zaf's MCP server or its settings ({})",
                check.profile,
                check.problems.join("; ")
            )),
            command: Some(Command::from("zaf profile init")),
            why: Text::from("zaf runs its models' verbs through its MCP server in the profile"),
        }),
        Check::ProfileMcp(_) => None,
        Check::Folder(check) if !check.ok => Some(folder(check)),
        Check::Folder(_) => None,
        Check::CloneFailed { repo, dir, said } => Some(Fix {
            problem: Text::from(format!(
                "the design repo `{repo}` could not be cloned (it said: {said})"
            )),
            command: Some(Command::from(format!(
                "gh repo clone {repo} {}",
                quote(dir)
            ))),
            why: Text::from("zaf reads the design pages the keeper answers from"),
        }),
        Check::IgnoreForeign { lines, file } => Some(Fix {
            problem: Text::from(format!(
                "`{file}` holds your own lines, so zaf left it as it is"
            )),
            command: None,
            why: Text::from(format!(
                "merge these lines into it by hand, keeping yours, so git tracks actions and roles: {}",
                lines.join(", ")
            )),
        }),
        Check::RootIgnoresState { folder } => Some(Fix {
            problem: Text::from(format!(
                "the root `.gitignore` ignores the whole `{folder}/` folder"
            )),
            command: None,
            why: Text::from(format!(
                "change its `{folder}/` line to `{folder}/*`: git cannot track actions and roles under an ignored folder"
            )),
        }),
    }
}

/// The fix for a tool that is missing, too old or prints no version; none for one that is right.
fn tool(check: &ToolCheck) -> Option<Fix> {
    let name = &check.tool;
    let wanted = &check.wanted;
    let problem = match &check.status {
        ToolStatus::Ok { .. } | ToolStatus::NotNeeded { .. } => return None,
        ToolStatus::Missing => {
            format!("`{name}` is not installed (the manifest requires `{wanted}`)")
        }
        ToolStatus::TooOld { found, .. } => {
            format!("`{name}` is {found}, older than the manifest's `{wanted}`")
        }
        ToolStatus::Unknown => {
            format!("`{name} --version` prints no version to compare with `{wanted}`")
        }
    };
    Some(match known(name) {
        Some(known) => Fix {
            problem: Text::from(problem),
            command: Some(Command::from(known.page)),
            why: Text::from(known.why),
        },
        None => Fix {
            problem: Text::from(problem),
            command: None,
            why: Text::from(format!("install `{name}`: your `.zaf.toml` asks for it")),
        },
    })
}

/// The fix for a board or a forge that does not answer: the sign-in that backend uses.
fn backend_fix(backend: &Backend, check: &BackendCheck) -> Option<Fix> {
    if check.ok {
        return None;
    }
    let problem = Text::from(format!(
        "the {} did not answer: {}",
        check.what, check.detail
    ));
    Some(match backend {
        Backend::Github => Fix {
            problem,
            command: Some(Command::from("gh auth login")),
            why: Text::from("zaf reads your issues and opens pull requests as you, through gh"),
        },
        Backend::Forgejo { token_env } => Fix {
            problem,
            command: Some(Command::from(format!("export {token_env}=…"))),
            why: Text::from(format!(
                "zaf reaches your Forgejo with the token in `{token_env}`, which the manifest names"
            )),
        },
    })
}

/// The fix for a folder that is missing or cannot be written: make it.
fn folder(check: &PathCheck) -> Fix {
    let path = path_text(&check.path);
    Fix {
        problem: Text::from(format!(
            "the {} `{path}` is missing or cannot be written",
            check.what
        )),
        command: Some(Command::from(format!("mkdir -p {}", quote(&path)))),
        why: Text::from(match check.what {
            "workspaces dir" => "zaf opens each task's workspace there",
            _ => "zaf keeps what it makes for the project there",
        }),
    }
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

/// Single-quotes `text` for a POSIX shell when it holds anything but plain path characters.
fn quote(text: &str) -> String {
    let plain = text.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | '+' | '@' | ':')
    });
    if plain && !text.is_empty() {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests;
