//! `zaf ask`: asks the keeper, prints the verified answer, and logs every question.

use std::time::{Duration, SystemTime};

use super::Cli;
use super::brief::instance;
use super::exit::{BACKEND, OK, REFUSED, fail, fail_composition};

use crate::app::ask::{AskFailure, AskPorts, AskQuery, LadderRung, ask};
use crate::composition::{Composition, CompositionError};
use crate::domain::IssueNumber;
use crate::manifest::{ModelName, Rung, RunnerName};
use crate::ports::Runner;

/// The runner a model rung runs on, with `{model}` set to the rung.
const MODEL_RUNNER: &str = "claude";

/// Asks the keeper up to five questions.
pub fn run(cli: &Cli, task: IssueNumber, questions: &[String]) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return fail_composition(&path, &error),
    };
    let design = match composition.design() {
        Ok(design) => design,
        Err(error) => return fail_composition(&path, &error),
    };
    let manifest = composition.manifest();
    let ladder = &manifest.ask.ladder;
    let runners: Vec<Result<Box<dyn Runner>, String>> = ladder
        .iter()
        .map(|rung| runner_for(&composition, rung))
        .collect();
    let ports = AskPorts {
        board: board.as_ref(),
        design: design.as_deref(),
        ladder: ladder
            .iter()
            .zip(&runners)
            .map(|(name, runner)| LadderRung {
                name: name.clone(),
                runner: runner.as_ref().map(AsRef::as_ref).map_err(Clone::clone),
            })
            .collect(),
    };
    let from = instance();
    let state_dir = composition.state_dir();
    let query = AskQuery {
        task,
        from: &from,
        questions,
        max: manifest.ask.max,
        root: composition.root(),
        state_dir: &state_dir,
        cache_window: cache_window(&composition),
        now: SystemTime::now(),
    };
    match ask(&query, &ports) {
        Ok(outcome) if cli.json => {
            println!("{}", serde_json::json!(outcome.lines));
            OK
        }
        Ok(outcome) => {
            print!("{}", outcome.answer);
            if cli.more {
                for (entry, line) in outcome.answer.questions.iter().zip(&outcome.lines) {
                    let rung = line.rung.as_deref().unwrap_or("none");
                    println!("{}: rung {rung}, check {}", entry.id, line.check);
                }
            }
            OK
        }
        Err(AskFailure::Board(error) | AskFailure::Design(error)) => fail(&path, &error),
        Err(error @ AskFailure::State { .. }) => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            REFUSED
        }
    }
}

/// How long an answered question is served from the log, from `[ask] cache_hours`.
fn cache_window(composition: &Composition) -> Duration {
    Duration::from_secs(composition.manifest().ask.cache_hours.saturating_mul(3600))
}

/// A rung naming a declared runner runs on it (`local`); any other rung is a model
/// on `[runners.claude]`, with `{model}` substituted (`haiku`, `sonnet`).
fn runner_for(composition: &Composition, rung: &Rung) -> Result<Box<dyn Runner>, String> {
    let runners = &composition.manifest().runners;
    let own = RunnerName::from(rung.as_ref());
    if runners.contains_key(&own) {
        return composition.runner(&own).map_err(|error| error.to_string());
    }
    let model_runner = RunnerName::from(MODEL_RUNNER);
    composition
        .runner_with_model(&model_runner, &ModelName::from(rung.as_ref()))
        .map_err(|error| match error {
            CompositionError::NoRunner(_) => {
                format!("no [runners.{rung}] and no [runners.{MODEL_RUNNER}]")
            }
            other => other.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::{Profile, RoleName};
    use crate::ports::RunRequest;

    const MANIFEST: &str = "\
[project]
name = \"example\"

[board]
kind = \"github-issues\"
repo = \"owner/example\"

[runners.local]
command = \"echo local-model\"
";

    fn composition(extra: &str) -> (tempfile::TempDir, Composition) {
        let root = tempfile::tempdir().expect("temp dir");
        let manifest = format!("{MANIFEST}{extra}");
        std::fs::write(root.path().join("ZAF.toml"), manifest).expect("manifest");
        let composition = Composition::load(root.path()).expect("composition");
        (root, composition)
    }

    fn transcript(runner: &dyn Runner, dir: &Path) -> String {
        let request = RunRequest {
            role: RoleName::from("keeper"),
            profile: Profile::Lean,
            prompt: String::new(),
            dir: dir.to_owned(),
            env: std::collections::BTreeMap::new(),
        };
        runner.run(&request).expect("run").transcript
    }

    #[test]
    fn the_cache_window_comes_from_the_manifest() {
        let (_root, default) = composition("");
        assert_eq!(cache_window(&default), Duration::from_secs(24 * 3600));
        let (_root, short) = composition("\n[ask]\ncache_hours = 2\n");
        assert_eq!(cache_window(&short), Duration::from_secs(2 * 3600));
    }

    #[test]
    fn a_rung_naming_a_runner_runs_on_it_and_a_model_rung_fills_model() {
        let (root, composition) =
            composition("\n[runners.claude]\ncommand = \"echo model={model}\"\n");
        let local = runner_for(&composition, &Rung::from("local")).expect("local");
        assert_eq!(transcript(local.as_ref(), root.path()), "local-model\n");
        let haiku = runner_for(&composition, &Rung::from("haiku")).expect("haiku");
        assert_eq!(transcript(haiku.as_ref(), root.path()), "model=haiku\n");
    }

    #[test]
    fn a_model_rung_without_the_model_runner_fails_only_itself() {
        let (_root, composition) = composition("");
        assert!(runner_for(&composition, &Rung::from("local")).is_ok());
        let error = runner_for(&composition, &Rung::from("sonnet"))
            .err()
            .expect("no runner");
        assert_eq!(error, "no [runners.sonnet] and no [runners.claude]");
    }
}
