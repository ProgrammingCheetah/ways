//! `zaf do`: list the actions the caller may read, run one, or print the file a name resolves to.
//!
//! Parses and prints; [`crate::app::actions`] spawns, logs and times out. Exit 0 prints nothing
//! beyond what `--more` or `--json` asks for. `FAIL` and `NOT_RUN` go to stderr, the way every
//! verb reports what went wrong, and what was asked for goes to stdout.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::Cli;
use super::exit::{BACKEND, OK};
use super::slice::VERSION_VAR;
use crate::app::actions::{
    Caller, MINUTE, Nesting, Refused, RunOutcome, RunRequest, RunResult, SliceContext, Tier,
    last_lines, list_actions, read_action, refuse_forbidden, run_action,
};
use crate::composition::{Composition, CompositionError, zaf_home};
use crate::manifest::{ActionsSection, HOME_FILE};

/// How many lines of the log a `FAIL` shows.
const TAIL_LINES: usize = 10;

/// What `zaf do` takes. Flags come before the name: everything after it goes to the body as
/// given, `--json` and `--help` included.
#[derive(Debug, clap::Args)]
pub struct DoArgs {
    /// Print the file the name resolves to instead of running it.
    #[arg(long)]
    pub show: bool,
    /// Skip the project tier: run the machine's action of that name, as when the project's file
    /// shadows it. Only `machine` is a tier to name; the mode and phase checks apply as ever.
    #[arg(long, value_enum, value_name = "TIER")]
    pub tier: Option<TierFlag>,
    /// The action's name and its arguments; no name lists every action the caller may read.
    #[arg(
        value_name = "NAME [ARGS]",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub words: Vec<OsString>,
}

/// The tiers `--tier` names: the machine's alone, since the project's is what a name resolves to
/// first anyway. A clap usage error, exit 2, for any other value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum TierFlag {
    /// The machine's `~/.zaf/actions/`.
    Machine,
}

impl From<TierFlag> for Tier {
    fn from(flag: TierFlag) -> Self {
        match flag {
            TierFlag::Machine => Self::Machine,
        }
    }
}

/// Runs the verb.
pub fn run(cli: &Cli, args: &DoArgs) -> u8 {
    let path = cli.verb.path();
    let only = args.tier.map(Tier::from);
    match (args.words.split_first(), args.show) {
        (None, _) if only.is_some() => {
            eprintln!("zaf {path}: --tier takes an action name");
            BACKEND
        }
        (None, false) => list(cli, &path),
        (Some((name, [])), true) => show(cli, &name.to_string_lossy(), only),
        (_, true) => {
            eprintln!("zaf {path}: --show takes one name");
            BACKEND
        }
        (Some((name, rest)), false) => {
            let name = name.to_string_lossy();
            let outcome = run_named(&name, rest, only);
            print_outcome(cli, &outcome)
        }
    }
}

/// Where a run happens: the project when there is one, and the home.
struct Place {
    root: Option<PathBuf>,
    /// The project, for the board and branch patterns a `target: task` run names its slice from.
    project: Option<Composition>,
    home: PathBuf,
    timeout_minutes: u64,
}

impl Place {
    /// Finds the place from the current directory. Outside a project the bound comes from the
    /// home's `config.toml`, the only manifest there is. A broken manifest is an error, never
    /// outside a project: the project's actions must not silently turn into the machine's.
    fn here() -> Result<Self, String> {
        let home = zaf_home().ok_or("no zaf home: set ZAF_HOME or HOME")?;
        let home = std::path::absolute(&home)
            .map_err(|error| format!("cannot read the zaf home {}: {error}", home.display()))?;
        match Composition::discover_here() {
            Ok(composition) => Ok(Self {
                root: Some(composition.root().to_owned()),
                timeout_minutes: composition.manifest().actions.timeout_minutes,
                project: Some(composition),
                home,
            }),
            Err(CompositionError::NoProject { .. }) => Ok(Self {
                root: None,
                project: None,
                timeout_minutes: machine_timeout(&home)?,
                home,
            }),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// `[actions] timeout_minutes` of the home's `config.toml`, the default when it has none.
fn machine_timeout(home: &Path) -> Result<u64, String> {
    let file = home.join(HOME_FILE);
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("cannot read {}: {error}", file.display())),
    };
    let invalid = |error: &dyn std::fmt::Display| format!("{}: {error}", file.display());
    let table: toml::Table = text.parse().map_err(|error| invalid(&error))?;
    let section: ActionsSection = table
        .get("actions")
        .cloned()
        .map(toml::Value::try_into)
        .transpose()
        .map_err(|error| invalid(&error))?
        .unwrap_or_default();
    if section.timeout_minutes == 0 {
        return Err(invalid(&"[actions] timeout_minutes must be at least 1"));
    }
    Ok(section.timeout_minutes)
}

/// The run of one name, with everything that stops it before the body as `NOT_RUN`.
pub(super) fn run_named(name: &str, args: &[OsString], only: Option<Tier>) -> RunOutcome {
    let not_run = |reason| RunOutcome::not_run(name, None, reason);
    let place = match Place::here() {
        Ok(place) => place,
        Err(reason) => return not_run(reason),
    };
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => return not_run(format!("no current directory: {error}")),
    };
    let nesting = match Nesting::from_env() {
        Ok(nesting) => nesting,
        Err(reason) => return not_run(reason),
    };
    let caller = match Caller::from_env() {
        Ok(caller) => caller,
        Err(reason) => return not_run(reason),
    };
    // Before any file is read: a name `[forbidden]` holds back is refused whatever it resolves
    // to, unless this call runs inside another action's body.
    let forbidden = place
        .project
        .as_ref()
        .map(|project| &project.manifest().forbidden.commands[..])
        .unwrap_or_default();
    if let Some(reason) =
        refuse_forbidden(name, forbidden.iter().map(AsRef::as_ref), &caller, &nesting)
    {
        return not_run(reason);
    }
    // Built here and not asked of the board: a run without a target never reads it, and a board
    // that cannot be built stops only the runs that need it.
    let board = place
        .project
        .as_ref()
        .map(|project| project.board().map_err(|error| error.to_string()));
    // The same for the forge and the version control, which only an action with `phases` reads.
    let forge = place
        .project
        .as_ref()
        .map(|project| project.forge().map_err(|error| error.to_string()));
    let vcs = place
        .project
        .as_ref()
        .map(|project| project.vcs().map_err(|error| error.to_string()));
    let version = std::env::var(VERSION_VAR)
        .ok()
        .filter(|version| !version.is_empty());
    let slice = place
        .project
        .as_ref()
        .zip(board.as_ref())
        .zip(forge.as_ref())
        .zip(vcs.as_ref())
        .map(|(((project, board), forge), vcs)| SliceContext {
            board: board.as_ref().map(|board| &**board).map_err(Clone::clone),
            forge: forge.as_ref().map(|forge| &**forge).map_err(Clone::clone),
            vcs: vcs.as_ref().map(|vcs| &**vcs).map_err(Clone::clone),
            branches: &project.manifest().branches,
            version: version.as_deref(),
        });
    // The table of the project the run is in; the machine's actions have none. A value that
    // needs a version the caller has not set starts nothing, rather than run without it.
    let env = match place.project.as_ref().zip(place.root.as_deref()) {
        Some((project, root)) => match project.manifest().env.resolve(root, version.as_deref()) {
            Ok(env) => env,
            Err(error) => return not_run(error.to_string()),
        },
        None => BTreeMap::new(),
    };
    run_action(&RunRequest {
        name,
        args,
        cwd: &cwd,
        project_root: place.root.as_deref(),
        home: &place.home,
        timeout_minutes: place.timeout_minutes,
        minute: MINUTE,
        nesting,
        caller,
        only,
        slice,
        env,
    })
}

/// Prints a run's outcome and returns its code.
pub(super) fn print_outcome(cli: &Cli, outcome: &RunOutcome) -> u8 {
    if cli.json {
        println!(
            "{}",
            serde_json::json!({
                "result": outcome.result.to_string(),
                "name": outcome.name,
                "tier": outcome.tier.map(|tier| tier.to_string()),
                "exit": outcome.exit,
                "meaning": outcome.meaning,
                "seconds": outcome.seconds,
                "log": outcome.log.as_ref().map(|log| log.display().to_string()),
            })
        );
        return outcome.exit;
    }
    if let Some(headline) = outcome.headline() {
        eprintln!("{headline}");
    }
    if let Some(log) = &outcome.log {
        if cli.more {
            print_whole(log);
        } else if outcome.result == RunResult::Fail {
            print_tail(log);
        }
    }
    outcome.exit
}

/// `--more`: the whole log after the run.
fn print_whole(log: &Path) {
    let copied = std::fs::File::open(log)
        .and_then(|mut file| std::io::copy(&mut file, &mut std::io::stdout().lock()));
    if let Err(error) = copied {
        eprintln!("zaf do: cannot read {}: {error}", log.display());
    }
}

/// The last lines of the log, under the `FAIL` line they explain.
fn print_tail(log: &Path) {
    match last_lines(log, TAIL_LINES) {
        Ok(lines) => {
            for line in lines {
                eprintln!("{line}");
            }
        }
        Err(error) => eprintln!("zaf do: cannot read {}: {error}", log.display()),
    }
}

/// `zaf do`: name, tier and description of every action the caller may read. A caller that cannot
/// be told is an error, with nothing listed.
fn list(cli: &Cli, path: &str) -> u8 {
    let found = Place::here().and_then(|place| Ok((place, Caller::from_env()?)));
    let (place, caller) = match found {
        Ok(found) => found,
        Err(reason) => {
            eprintln!("zaf {path}: {reason}");
            return BACKEND;
        }
    };
    let listed = match list_actions(place.root.as_deref(), &place.home, &caller) {
        Ok(listed) => listed,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return BACKEND;
        }
    };
    if cli.json {
        let rows: Vec<_> = listed
            .iter()
            .map(|action| {
                serde_json::json!({
                    "name": action.name,
                    "tier": action.tier.to_string(),
                    "description": action.description,
                })
            })
            .collect();
        println!("{}", serde_json::Value::Array(rows));
        return OK;
    }
    let width = listed.iter().map(|a| a.name.len()).max().unwrap_or(0);
    for action in &listed {
        println!(
            "{name:<width$}  {tier:<7}  {description}",
            name = action.name,
            tier = action.tier.to_string(),
            description = action.description,
        );
    }
    OK
}

/// `zaf do --show <name>`: the file, byte for byte, when the caller may read it.
fn show(cli: &Cli, name: &str, only: Option<Tier>) -> u8 {
    let bytes = Place::here()
        .and_then(|place| Ok((place, Caller::from_env()?)))
        .map_err(Refused::before_the_file)
        .and_then(|(place, caller)| {
            read_action(place.root.as_deref(), &place.home, name, only, &caller)
        });
    match bytes {
        Ok(bytes) => {
            let _ = std::io::stdout().write_all(&bytes);
            OK
        }
        Err(refused) => print_outcome(
            cli,
            &RunOutcome::not_run(name, refused.tier, refused.reason),
        ),
    }
}
