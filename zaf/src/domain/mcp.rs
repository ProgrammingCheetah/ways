//! The pure types of the MCP server: a tool, a call to it, what the call returns, a progress
//! tick and the flag that stops a call. No protocol type crosses into the domain.

use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::RoleName;

/// Who calls the MCP server, as the server read it once at start (D7).
///
/// A caller it cannot tell is no `McpCaller`: the probe refuses instead, so a call is never
/// treated as the Engineer's by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpCaller {
    /// Neither `ZAF_INSTANCE` nor a running launch above: the Engineer's own session.
    Outside,
    /// An instance of this role, named by `ZAF_INSTANCE`.
    Instance(RoleName),
}

/// One tool the server lists: a verb of the binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    /// The tool's name, the verb it runs.
    pub name: String,
    /// What the tool does, in a line.
    pub description: String,
    /// The JSON schema of the tool's input: `args`, `stdin`, `json` and `more`, none required.
    pub input_schema: serde_json::Value,
}

impl ToolSpec {
    /// The command line of `call`: `zaf [--json] [--more] <verb> <args…>`, one element per
    /// word, for a process to run directly: no shell, no quoting.
    pub fn command_line(&self, call: &ToolCall) -> Vec<OsString> {
        let mut line = vec![OsString::from("zaf")];
        if call.json {
            line.push("--json".into());
        }
        if call.more {
            line.push("--more".into());
        }
        line.push(self.name.as_str().into());
        line.extend(call.args.iter().map(OsString::from));
        line
    }
}

/// A request to run one tool.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolCall {
    /// The tool's name, the verb to run.
    pub tool: String,
    /// The verb's arguments, in order.
    pub args: Vec<String>,
    /// What the verb reads on its standard input, if anything.
    pub stdin: Option<String>,
    /// Whether the verb runs with `--json`.
    pub json: bool,
    /// Whether the verb runs with `--more`.
    pub more: bool,
}

/// The `_meta` key that carries a result's exit code.
///
/// The MCP spec's `_meta` key rules (modelcontextprotocol.io, latest spec, "General fields"):
/// a key is an optional prefix and a name. A prefix is dot-separated labels followed by a
/// slash; each label starts with a letter, ends with a letter or digit and holds letters,
/// digits and hyphens inside. A prefix whose second label is `modelcontextprotocol` or `mcp`
/// is reserved for MCP, and reverse DNS is only a SHOULD. A name begins and ends with an
/// alphanumeric and may hold hyphens, underscores and dots inside. `zaf/exit` follows all of
/// that: the one-label prefix `zaf` has no second label to reserve, and `exit` is a valid
/// name, so the key stays as the ticket names it.
pub const META_EXIT_KEY: &str = "zaf/exit";

/// What a tool call returns, shaped for the client: one block of text, whether it is an
/// error, and the exit code for `_meta`. No structured content is ever produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// The verb's standard output, then its standard error, in one block; when the exit is
    /// not 0, a last line `exit <n>` follows.
    pub text: String,
    /// Whether the exit is not 0.
    pub is_error: bool,
    /// The verb's exit code, always carried, for `_meta` under [`META_EXIT_KEY`]. Exit codes
    /// keep their meanings: 0, 1, 2, 64, and for `do` 125 and the action's own.
    pub exit: i32,
}

/// A sign of life from a running call: how long it has run and the last line it wrote to
/// standard error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    /// Time since the call started.
    pub elapsed: Duration,
    /// The last line the call wrote to standard error, if it has written one.
    pub last_stderr: Option<String>,
}

/// A shared flag that asks a running call to stop; the adapter polls it.
///
/// Clones share the flag: cancelling one cancels all.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// A flag not yet raised.
    pub fn new() -> Self {
        Self::default()
    }

    /// Raises the flag; raising it again changes nothing.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether the flag is raised.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The parsed command a call runs, as policy reads it: the words of its path and the flags that
/// matter, never its text. The words are `Verb::path()` split on spaces, then the action's name
/// when the verb is `do` (`["forge", "merge"]`, `["do", "land"]`, `["board", "claim"]`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandPath {
    /// The path's words.
    pub words: Vec<String>,
    /// Whether `spawn` was given `--watch`.
    pub watch: bool,
    /// Whether `hub` serves (`--port` was given).
    pub serves: bool,
}

impl CommandPath {
    /// The path of `words`, with no flag.
    pub fn new<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            words: words.into_iter().map(Into::into).collect(),
            watch: false,
            serves: false,
        }
    }
}

/// The `[forbidden]` lines that start with `zaf`, each read by its words with flags dropped.
///
/// A line is read as `zaf` then words: a word starting with `-` is a flag and is dropped
/// (`--tier` takes a value, which is dropped too), so `zaf do --tier machine land` reads as
/// `do land` and `zaf --more forge merge` as `forge merge`. A line that does not start with
/// `zaf` (`gh pr merge`) is no zaf path and never matches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForbiddenSection {
    lines: Vec<Vec<String>>,
}

impl ForbiddenSection {
    /// The section of the manifest's `forbidden.commands`.
    pub fn new<'a>(commands: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            lines: commands.into_iter().filter_map(Self::words).collect(),
        }
    }

    /// The words of a `zaf` line with flags dropped; `None` for any other line.
    fn words(command: &str) -> Option<Vec<String>> {
        let mut raw = command.split_whitespace();
        if raw.next()? != "zaf" {
            return None;
        }
        let mut words = Vec::new();
        while let Some(word) = raw.next() {
            if word == "--tier" {
                raw.next();
            } else if !word.starts_with('-') {
                words.push(word.to_owned());
            }
        }
        Some(words)
    }

    /// `forbids(&CommandPath) -> bool`: whether some line names `path`.
    ///
    /// The path's words begin with the line's words, so `zaf forge merge` names `forge merge`,
    /// and a line naming a whole verb names its subcommands. A bare `do` path (no action) is
    /// not `do land`; a line of `zaf` alone names nothing.
    pub fn forbids(&self, path: &CommandPath) -> bool {
        self.lines
            .iter()
            .any(|line| !line.is_empty() && path.words.starts_with(line))
    }
}

/// What the policy decides for one call: run it, or refuse it before a child starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The call may start.
    Run,
    /// The call is refused: one line of what was refused and one of the way on.
    Refuse {
        /// What was refused, as the command line (`zaf epic sign`).
        what: String,
        /// The way on, from the policy table.
        way_on: &'static str,
    },
}

/// Who a policy row refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refused {
    /// Every caller, the Engineer's own session included.
    Everyone,
    /// An instance; an outside caller runs it.
    Instance,
}

/// How a policy row names a command.
#[derive(Debug, Clone, Copy)]
enum Names {
    /// Every path whose words begin with these.
    Path(&'static [&'static str]),
    /// Every path whose words begin with `path` but not with `except`.
    PathBut {
        path: &'static [&'static str],
        except: &'static [&'static str],
    },
    /// `hub` serving (`--port`).
    Serving,
    /// `spawn --watch`.
    Watching,
    /// Each `zaf …` line of the manifest's `[forbidden]`.
    Forbidden,
}

impl Names {
    fn hits(self, path: &CommandPath, forbidden: &ForbiddenSection) -> bool {
        let starts = |prefix: &[&str]| {
            path.words.len() >= prefix.len()
                && path
                    .words
                    .iter()
                    .zip(prefix)
                    .all(|(word, prefix)| word == prefix)
        };
        match self {
            Self::Path(prefix) => starts(prefix),
            Self::PathBut {
                path: prefix,
                except,
            } => starts(prefix) && !starts(except),
            Self::Serving => path.serves,
            Self::Watching => path.watch,
            Self::Forbidden => forbidden.forbids(path),
        }
    }
}

/// One row of D9's table.
struct Row {
    names: &'static [Names],
    refused: Refused,
    way_on: &'static str,
}

/// D9's table, in the order a call is judged: the rows refused to every caller first, so a
/// command both of them name gets the way on that holds for the Engineer too. `do release` and
/// `do ship` have no row (Q5); they stay under the action modes and `[forbidden]`.
const ROWS: [Row; 7] = [
    Row {
        names: &[Names::Path(&["epic", "sign"])],
        refused: Refused::Everyone,
        way_on: "Sign in zaf's chair.",
    },
    Row {
        names: &[Names::PathBut {
            path: &["begin"],
            except: &["begin", "ready"],
        }],
        refused: Refused::Everyone,
        way_on: "Objectives and design are the Engineer's interviews: open zaf's chair in a terminal (`zaf`).",
    },
    Row {
        names: &[
            Names::Serving,
            Names::Path(&["hub", "report"]),
            Names::Watching,
        ],
        refused: Refused::Everyone,
        way_on: "they never end, or are the hook's own; `spawn start --version <v>` starts the scheduler",
    },
    Row {
        names: &[Names::Forbidden],
        refused: Refused::Instance,
        way_on: "Forbidden by the manifest; only `slice-merge` and `release` merge.",
    },
    Row {
        names: &[Names::Path(&["board", "ruling"])],
        refused: Refused::Instance,
        way_on: "Rulings are the Engineer's: raise a question.",
    },
    Row {
        names: &[
            Names::Path(&["update"]),
            Names::Path(&["setup"]),
            Names::Path(&["profile"]),
            Names::Path(&["hub", "link"]),
            Names::Path(&["hub", "service"]),
        ],
        refused: Refused::Instance,
        way_on: "It changes the machine; the Engineer runs it.",
    },
    Row {
        names: &[
            Names::PathBut {
                path: &["spawn"],
                except: &["spawn", "trace"],
            },
            Names::Path(&["board", "claim"]),
            Names::Path(&["board", "done"]),
            Names::Path(&["slice"]),
        ],
        refused: Refused::Instance,
        way_on: "zaf moves tickets; nobody moves themselves.",
    },
];

/// The MCP's policy: who may run which command (D9). Pure: it reads the parsed command, never
/// its text, and does nothing but answer.
#[derive(Debug, Clone, Copy)]
pub struct Policy;

impl Policy {
    /// Judges one call by its caller and its parsed command path, against the manifest's
    /// `[forbidden]`.
    pub fn judge(caller: &McpCaller, path: &CommandPath, forbidden: &ForbiddenSection) -> Verdict {
        for row in &ROWS {
            let applies = match (row.refused, caller) {
                (Refused::Everyone, _) | (Refused::Instance, McpCaller::Instance(_)) => true,
                (Refused::Instance, McpCaller::Outside) => false,
            };
            if applies && row.names.iter().any(|names| names.hits(path, forbidden)) {
                return Verdict::Refuse {
                    what: Self::what(path),
                    way_on: row.way_on,
                };
            }
        }
        Verdict::Run
    }

    /// The command line that was refused.
    fn what(path: &CommandPath) -> String {
        let mut line = String::from("zaf");
        for word in &path.words {
            line.push(' ');
            line.push_str(word);
        }
        if path.watch {
            line.push_str(" --watch");
        }
        if path.serves {
            line.push_str(" --port");
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::RoleName;

    /// ways' own `[forbidden]` list (`.zaf.toml`).
    const WAYS: [&str; 9] = [
        "gh pr merge",
        "jj git push",
        "zaf forge merge",
        "zaf do pr-merge",
        "zaf do land",
        "zaf do ship",
        "zaf do --tier machine pr-merge",
        "zaf do --tier machine land",
        "zaf do --tier machine ship",
    ];

    #[test]
    fn forbidden_section_matches_forge_merge() {
        let section = ForbiddenSection::new(WAYS);
        assert!(section.forbids(&CommandPath::new(["forge", "merge"])));
        assert!(section.forbids(&CommandPath::new(["do", "land"])));
        assert!(section.forbids(&CommandPath::new(["do", "pr-merge"])));
    }

    #[test]
    fn forbidden_section_drops_flags_and_tier_value() {
        let section =
            ForbiddenSection::new(["zaf do --tier machine land", "zaf --more forge merge"]);
        assert!(section.forbids(&CommandPath::new(["do", "land"])));
        assert!(section.forbids(&CommandPath::new(["forge", "merge"])));
        // The tier's value is no word: `machine` names nothing.
        assert!(!section.forbids(&CommandPath::new(["do", "machine"])));
        assert!(!section.forbids(&CommandPath::new(["machine"])));
    }

    #[test]
    fn forbidden_section_ignores_non_zaf_lines() {
        let section = ForbiddenSection::new(WAYS);
        assert!(!section.forbids(&CommandPath::new(["gh", "pr", "merge"])));
        assert!(!section.forbids(&CommandPath::new(["pr", "merge"])));
        assert!(!section.forbids(&CommandPath::new(["git", "push"])));
        let section = ForbiddenSection::new(["gh pr merge", "zafx forge merge"]);
        assert!(!section.forbids(&CommandPath::new(["forge", "merge"])));
    }

    #[test]
    fn forbidden_section_leaves_other_paths() {
        let section = ForbiddenSection::new(WAYS);
        assert!(!section.forbids(&CommandPath::new(["forge", "show"])));
        assert!(!section.forbids(&CommandPath::new(["do", "slice-pr"])));
        assert!(!section.forbids(&CommandPath::new(["board", "ready"])));
        // A bare `do` and a bare `forge` are not the forbidden command.
        assert!(!section.forbids(&CommandPath::new(["do"])));
        assert!(!section.forbids(&CommandPath::new(["forge"])));
        assert!(!section.forbids(&CommandPath::default()));
        assert!(!ForbiddenSection::default().forbids(&CommandPath::new(["do", "land"])));
    }

    const SIGN: &str = "Sign in zaf's chair.";
    const BEGIN: &str = "Objectives and design are the Engineer's interviews: open zaf's chair in a terminal (`zaf`).";
    const NEVER_END: &str =
        "they never end, or are the hook's own; `spawn start --version <v>` starts the scheduler";
    const FORBIDDEN: &str = "Forbidden by the manifest; only `slice-merge` and `release` merge.";
    const RULING: &str = "Rulings are the Engineer's: raise a question.";
    const MACHINE: &str = "It changes the machine; the Engineer runs it.";
    const LANE: &str = "zaf moves tickets; nobody moves themselves.";

    fn instance() -> McpCaller {
        McpCaller::Instance(RoleName::from("programmer"))
    }

    fn refuse(what: &str, way_on: &'static str) -> Verdict {
        Verdict::Refuse {
            what: what.to_owned(),
            way_on,
        }
    }

    fn judge(caller: &McpCaller, path: &CommandPath) -> Verdict {
        Policy::judge(caller, path, &ForbiddenSection::new(WAYS))
    }

    fn watching(words: &[&str]) -> CommandPath {
        CommandPath {
            watch: true,
            ..CommandPath::new(words.iter().copied())
        }
    }

    fn serving(words: &[&str]) -> CommandPath {
        CommandPath {
            serves: true,
            ..CommandPath::new(words.iter().copied())
        }
    }

    #[test]
    fn policy_table_every_row_both_callers() {
        // (path, what, way on, refused to Outside too)
        let rows: Vec<(CommandPath, &str, &'static str, bool)> = vec![
            (
                CommandPath::new(["epic", "sign"]),
                "zaf epic sign",
                SIGN,
                true,
            ),
            (CommandPath::new(["begin"]), "zaf begin", BEGIN, true),
            (serving(&["hub"]), "zaf hub --port", NEVER_END, true),
            (
                CommandPath::new(["hub", "report"]),
                "zaf hub report",
                NEVER_END,
                true,
            ),
            (watching(&["spawn"]), "zaf spawn --watch", NEVER_END, true),
            (
                CommandPath::new(["forge", "merge"]),
                "zaf forge merge",
                FORBIDDEN,
                false,
            ),
            (
                CommandPath::new(["do", "land"]),
                "zaf do land",
                FORBIDDEN,
                false,
            ),
            (
                CommandPath::new(["do", "pr-merge"]),
                "zaf do pr-merge",
                FORBIDDEN,
                false,
            ),
            (
                CommandPath::new(["board", "ruling"]),
                "zaf board ruling",
                RULING,
                false,
            ),
            (CommandPath::new(["update"]), "zaf update", MACHINE, false),
            (CommandPath::new(["setup"]), "zaf setup", MACHINE, false),
            (
                CommandPath::new(["profile", "init"]),
                "zaf profile init",
                MACHINE,
                false,
            ),
            (
                CommandPath::new(["hub", "link"]),
                "zaf hub link",
                MACHINE,
                false,
            ),
            (
                CommandPath::new(["hub", "service", "install"]),
                "zaf hub service install",
                MACHINE,
                false,
            ),
            (CommandPath::new(["spawn"]), "zaf spawn", LANE, false),
            (
                CommandPath::new(["spawn", "start"]),
                "zaf spawn start",
                LANE,
                false,
            ),
            (
                CommandPath::new(["spawn", "stop"]),
                "zaf spawn stop",
                LANE,
                false,
            ),
            (
                CommandPath::new(["board", "claim"]),
                "zaf board claim",
                LANE,
                false,
            ),
            (
                CommandPath::new(["board", "done"]),
                "zaf board done",
                LANE,
                false,
            ),
            (
                CommandPath::new(["slice", "open"]),
                "zaf slice open",
                LANE,
                false,
            ),
        ];
        for (path, what, way_on, outside_too) in rows {
            assert_eq!(judge(&instance(), &path), refuse(what, way_on), "{what}");
            let outside = if outside_too {
                refuse(what, way_on)
            } else {
                Verdict::Run
            };
            assert_eq!(judge(&McpCaller::Outside, &path), outside, "{what}");
        }
    }

    #[test]
    fn policy_forbidden_refused_to_instance_runs_outside() {
        for words in [["forge", "merge"], ["do", "land"]] {
            let path = CommandPath::new(words);
            assert!(matches!(
                judge(&instance(), &path),
                Verdict::Refuse {
                    way_on: FORBIDDEN,
                    ..
                }
            ));
            assert_eq!(judge(&McpCaller::Outside, &path), Verdict::Run);
        }
        // With no `[forbidden]` line, the same command runs for an instance.
        let none = ForbiddenSection::default();
        let path = CommandPath::new(["forge", "merge"]);
        assert_eq!(Policy::judge(&instance(), &path, &none), Verdict::Run);
    }

    #[test]
    fn policy_open_paths_run() {
        // `do ship` is on ways' `[forbidden]` list but has no row of its own (Q5), so it is
        // judged against a manifest that does not list it.
        let section = ForbiddenSection::new(["zaf forge merge"]);
        let open = [
            CommandPath::new(["begin", "ready"]),
            CommandPath::new(["spawn", "trace"]),
            CommandPath::new(["do", "release"]),
            CommandPath::new(["do", "ship"]),
            CommandPath::new(["board", "ready"]),
            CommandPath::new(["hub"]),
        ];
        for path in &open {
            for caller in [McpCaller::Outside, instance()] {
                assert_eq!(Policy::judge(&caller, path, &section), Verdict::Run);
            }
        }
        // `do release` and `do ship` also run under ways' own `[forbidden]` for Outside.
        for action in ["release", "ship"] {
            let path = CommandPath::new(["do", action]);
            assert_eq!(judge(&McpCaller::Outside, &path), Verdict::Run);
        }
        // `spawn` with no flag is the tick: it runs for Outside.
        assert_eq!(
            judge(&McpCaller::Outside, &CommandPath::new(["spawn"])),
            Verdict::Run
        );
    }

    #[test]
    fn policy_sign_refused_to_every_caller() {
        let path = CommandPath::new(["epic", "sign"]);
        let want = refuse("zaf epic sign", SIGN);
        assert_eq!(judge(&McpCaller::Outside, &path), want);
        assert_eq!(judge(&instance(), &path), want);
        // Other `epic` verbs are not the row.
        assert_eq!(
            judge(&instance(), &CommandPath::new(["epic", "show"])),
            Verdict::Run
        );
    }
}
