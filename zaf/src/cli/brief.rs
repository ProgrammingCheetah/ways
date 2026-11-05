//! `zaf brief`: writes the task's context pack and prints its path and token estimate.

use std::ffi::OsStr;
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

/// The file at the project root that marks a node tree.
const NODE_TREE_FILE: &str = "NODE.inf.json";

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
    let nodes = nodes_for(&root, std::env::var_os("PATH").as_deref());
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

/// The `nodes` binary to chain with, only when the project root holds a node tree.
///
/// Without `<root>/NODE.inf.json`, `nodes` would walk up and chart whatever
/// tree lies above the project, so the chains are skipped instead.
fn nodes_for(root: &Path, path_var: Option<&OsStr>) -> Option<PathBuf> {
    if !root.join(NODE_TREE_FILE).is_file() {
        return None;
    }
    find_on_path(NODES_BINARY, path_var?)
}

/// The first executable file named `name` in the PATH value `path`.
fn find_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
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
        let composition = Composition::load_with_home(root.path(), None).expect("composition");
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

    /// A temp dir holding a fake `nodes` file, and its path as a PATH value.
    fn fake_nodes_on_path() -> (tempfile::TempDir, std::ffi::OsString, PathBuf) {
        let bin = tempfile::tempdir().expect("temp dir");
        let binary = bin.path().join(NODES_BINARY);
        std::fs::write(&binary, "#!/bin/sh\n").expect("fake nodes");
        let path_var = std::env::join_paths([bin.path()]).expect("path value");
        (bin, path_var, binary)
    }

    #[test]
    fn a_root_without_a_node_tree_gets_no_nodes_even_with_the_binary_on_path() {
        let (_bin, path_var, _binary) = fake_nodes_on_path();
        let root = tempfile::tempdir().expect("temp dir");
        assert_eq!(nodes_for(root.path(), Some(&path_var)), None);
    }

    #[test]
    fn a_root_with_a_node_tree_gets_the_binary_found_on_path() {
        let (_bin, path_var, binary) = fake_nodes_on_path();
        let root = tempfile::tempdir().expect("temp dir");
        std::fs::write(root.path().join(NODE_TREE_FILE), "{}").expect("node tree");
        assert_eq!(nodes_for(root.path(), Some(&path_var)), Some(binary));
    }

    #[test]
    fn a_root_with_a_node_tree_but_no_binary_gets_no_nodes() {
        let root = tempfile::tempdir().expect("temp dir");
        std::fs::write(root.path().join(NODE_TREE_FILE), "{}").expect("node tree");
        let empty = tempfile::tempdir().expect("temp dir");
        let path_var = std::env::join_paths([empty.path()]).expect("path value");
        assert_eq!(nodes_for(root.path(), Some(&path_var)), None);
        assert_eq!(nodes_for(root.path(), None), None);
    }
}
