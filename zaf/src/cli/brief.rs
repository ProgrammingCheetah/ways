//! `zaf brief`: writes the task's context pack and prints its path and token estimate.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::Cli;
use super::exit::{BACKEND, OK, REFUSED, fail, fail_composition};

use crate::app::brief::{BriefError, BriefPorts, BriefQuery, NodesChain, brief};
use crate::composition::Composition;
use crate::domain::IssueNumber;

/// The environment variable naming the instance a brief or an ask is for.
pub const INSTANCE_VAR: &str = "ZAF_INSTANCE";

/// The instance name when the environment names none: a run by hand.
pub const DEFAULT_INSTANCE: &str = "local";

/// The node tool the pack calls for each owned path.
const NODES_BINARY: &str = "nodes";

/// Builds a task's context pack.
pub fn run(cli: &Cli, task: IssueNumber) -> u8 {
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
    let instance = instance();
    let state_dir = composition.state_dir();
    let root = composition.root().to_owned();
    let nodes = find_on_path(NODES_BINARY);
    let chain = nodes
        .as_deref()
        .map(|binary| move |owned: &str| nodes_chain(binary, &root, owned));
    let style = style_file(&composition);
    let query = BriefQuery {
        task,
        instance: &instance,
        state_dir: &state_dir,
        style: style.as_deref(),
    };
    let ports = BriefPorts {
        board: board.as_ref(),
        design: design.as_deref(),
        nodes: chain.as_ref().map(|chain| chain as NodesChain<'_>),
    };
    match brief(&query, &ports) {
        Ok(brief) if cli.json => {
            println!("{}", serde_json::json!(brief));
            OK
        }
        Ok(brief) => {
            println!("{}", brief.path.display());
            println!("~{} tokens", brief.tokens);
            OK
        }
        Err(BriefError::Board(error) | BriefError::Design(error)) => fail(&path, &error),
        Err(error @ BriefError::Instance(_)) => {
            eprintln!("zaf {path}: {error}");
            REFUSED
        }
        Err(error @ BriefError::Write { .. }) => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
    }
}

/// The style file `[style] path` names, resolved against the project root.
fn style_file(composition: &Composition) -> Option<PathBuf> {
    let style = composition.manifest().style.as_ref()?;
    Some(composition.root().join(&style.path))
}

/// The instance this run is for, from `ZAF_INSTANCE`, or `local`.
pub(crate) fn instance() -> String {
    std::env::var(INSTANCE_VAR)
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_INSTANCE.to_owned())
}

/// The first executable file named `name` on PATH.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Runs `nodes chain <owned>` at the project root.
fn nodes_chain(binary: &Path, root: &Path, owned: &str) -> Result<String, String> {
    let output = Command::new(binary)
        .arg("chain")
        .arg(owned)
        .current_dir(root)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("{}: {}", output.status, stderr.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn composition(extra: &str) -> (tempfile::TempDir, Composition) {
        let root = tempfile::tempdir().expect("temp dir");
        let manifest = format!(
            "[project]\nname = \"example\"\n[board]\nkind = \"github-issues\"\nrepo = \"owner/example\"\n{extra}"
        );
        std::fs::write(root.path().join("ZAF.toml"), manifest).expect("manifest");
        let composition = Composition::load(root.path()).expect("composition");
        (root, composition)
    }

    #[test]
    fn the_style_file_comes_from_the_manifest_and_is_absent_without_one() {
        let (_root, none) = composition("");
        assert_eq!(style_file(&none), None);
        let (root, declared) = composition("[style]\npath = \"docs/style.md\"\n");
        assert_eq!(
            style_file(&declared),
            Some(root.path().join("docs/style.md"))
        );
    }
}
