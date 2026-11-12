//! `zaf setup`: in a folder with no manifest ask for one, move a root `ZAF.toml`, make the two homes, generate the profiles, then hand
//! the Engineer what it cannot do.

use std::fs;
use std::io::{self, ErrorKind};
use std::time::Duration;

use super::brief::INSTANCE_VAR;
use super::doctor::{ShellCommands, add_backend_checks};
use super::exit::{self, BACKEND, OK, REFUSED};
use super::{Cli, first_run};

use crate::adapters::forgejo::ForgejoApi;
use crate::adapters::shell::SystemShell;
use crate::app::init::{InitError, init_with_home};
use crate::app::profile::{ProfileError, current_pack_tag};
use crate::app::setup::{move_legacy_manifest, setup};
use crate::composition::{Composition, CompositionError, zaf_home};
use crate::manifest::{EnvVarName, ForgeUrl, RepoName};

/// Whether this run is an instance's: `ZAF_INSTANCE` set and not blank.
fn as_instance() -> bool {
    std::env::var_os(INSTANCE_VAR).is_some_and(|name| !name.to_string_lossy().trim().is_empty())
}

/// The marketplace's current pack tag, read from a scratch profile that is removed after.
fn learn_pack_tag() -> Result<String, ProfileError> {
    let scratch = std::env::temp_dir().join(format!("zaf-pack-tag-{}", std::process::id()));
    let shell = SystemShell::new(&std::env::temp_dir(), Duration::from_secs(120));
    let tag = fs::create_dir_all(&scratch)
        .map_err(|error| ProfileError::Install {
            line: format!("mkdir {}", scratch.display()),
            output: error.to_string(),
        })
        .and_then(|()| current_pack_tag(&shell, &scratch));
    let _ = fs::remove_dir_all(&scratch);
    tag
}

/// Seconds a probe waits to connect to one candidate, and to hear from it in all.
const PROBE_CONNECT_SECONDS: u32 = 2;
const PROBE_TOTAL_SECONDS: u32 = 4;

/// Whether `url` answers `GET /api/v1/version` as a Forgejo or Gitea instance; needs no token.
fn probe_forgejo(url: &ForgeUrl) -> bool {
    ForgejoApi::new(
        url.clone(),
        RepoName::from("probe/probe"),
        EnvVarName::from("FORGEJO_TOKEN"),
        None,
    )
    .time_limits(PROBE_CONNECT_SECONDS, PROBE_TOTAL_SECONDS)
    .version()
    .is_ok()
}

/// The exit code for an asking failure: 2 for a backend failure (the pack's tag, reading or
/// writing), 1 for a refusal.
fn init_exit(error: &InitError) -> u8 {
    match error {
        InitError::Pack(_) | InitError::Io(_) => BACKEND,
        _ => REFUSED,
    }
}

/// Where no manifest is found: asks init's questions on stdin and stdout, writes `.zaf.toml`
/// in `dir`, says so in one line and loads it. Closed input or a blank required answer writes
/// nothing and exits 1 naming the question; a failed pack tag read exits 2.
fn ask_for_manifest(
    dir: &std::path::Path,
    zaf_home: Option<&std::path::Path>,
) -> Result<Composition, u8> {
    let written = init_with_home(
        dir,
        zaf_home,
        &mut learn_pack_tag,
        &mut probe_forgejo,
        &mut io::stdin().lock(),
        &mut io::stdout(),
    )
    .map_err(|error| {
        eprintln!("zaf setup: {error}");
        init_exit(&error)
    })?;
    let shown = written.strip_prefix(dir).unwrap_or(&written);
    println!("wrote {}", shown.display());
    Composition::load_with_home(dir, zaf_home).map_err(|error| {
        eprintln!("zaf setup: {error}");
        REFUSED
    })
}

/// Renames a root `ZAF.toml` to `.zaf.toml` (the Engineer's run only, said in one line),
/// makes the project's `.zaf/` and the zaf home, replaces an old `.zaf/.gitignore` that holds
/// only `*`, clones a missing design repo, generates the profiles as `zaf profile init` does,
/// runs doctor (with the board's and the forge's backends asked, as `zaf doctor` does, so a Forgejo
/// project's missing token is reported) and prints the worksheet; exits 1 while anything is left for the Engineer (a
/// pack refusal included) and 2 when a profile cannot be generated. A `ZAF.toml` beside a new manifest file exits 1 and
/// moves nothing. It installs no tool; the one thing it copies is the installed pack's actions into
/// the zaf home's `actions/`, never over a file there, in one line.
///
/// Where no manifest is found it first asks init's questions and writes `.zaf.toml` (see
/// [`ask_for_manifest`]), then goes on in the same run; a folder with any manifest is asked nothing.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    // The one verb that does not warn about the retired name: it is the one that moves it.
    let zaf_home = zaf_home();
    let composition = match Composition::discover_here_quiet() {
        Ok(composition) => composition,
        Err(CompositionError::NoProject { start }) => {
            match ask_for_manifest(&start, zaf_home.as_deref()) {
                Ok(composition) => composition,
                Err(code) => return code,
            }
        }
        Err(error) => {
            first_run();
            eprintln!("zaf {path}: {error}");
            return REFUSED;
        }
    };
    match move_legacy_manifest(composition.root(), zaf_home.as_deref(), as_instance()) {
        Ok(moved) => {
            if let Some(line) = moved.line() {
                println!("{line}");
            }
        }
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return if error.kind() == ErrorKind::AlreadyExists {
                REFUSED
            } else {
                exit::BACKEND
            };
        }
    }
    let mut report = match setup(
        composition.root(),
        zaf_home.as_deref(),
        composition.manifest(),
        &ShellCommands::new(composition.shell()),
        composition.shell().as_ref(),
    ) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return exit::BACKEND;
        }
    };
    add_backend_checks(&mut report.doctor, &composition);
    if let Some(profiles) = report
        .profiles
        .as_ref()
        .filter(|profiles| !profiles.unchanged())
    {
        for line in profiles.lines() {
            println!("{line}");
        }
    }
    for line in report.actions.lines() {
        println!("{line}");
    }
    for line in report.doctor.lines() {
        println!("{line}");
    }
    for line in report.worksheet_lines() {
        println!("{line}");
    }
    if report.is_ready() { OK } else { REFUSED }
}
