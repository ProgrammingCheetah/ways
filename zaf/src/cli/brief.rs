//! `zaf brief`: writes the task's context pack and prints its path and token estimate.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::Cli;
use super::exit::{BACKEND, OK, REFUSED, fail, fail_composition, note_design_paths};
use super::slice::{VERSION_VAR, fail_slice};

use crate::app::actions::{ActionName, Caller, SliceRecords};
use crate::app::brief::{
    ActionsView, BriefActionsError, BriefError, BriefPorts, BriefQuery, NodesChain, PhaseView,
    brief_for_phase,
};
use crate::app::slice::{self, SliceQuery};
use crate::composition::{Composition, zaf_home};
use crate::domain::{Ticket, TicketId, TicketType};
use crate::ports::{BaseName, Board, Forge, PrNumber, Vcs};

/// The environment variable naming the instance a brief or an ask is for.
pub const INSTANCE_VAR: &str = "ZAF_INSTANCE";

/// The instance name when the environment names none: a run by hand.
pub const DEFAULT_INSTANCE: &str = "local";

/// The node tool the pack calls for each owned path.
const NODES_BINARY: &str = "nodes";

/// The file at the project root that marks a node tree.
const NODE_TREE_FILE: &str = "NODE.inf.json";

/// What a resumed phase's pack reads its diff and findings from, owned until the pack is built.
struct Resumed {
    pr: PrNumber,
    workspace: Option<PathBuf>,
    base: BaseName,
    forge: Box<dyn Forge>,
    vcs: Box<dyn Vcs>,
}

impl Resumed {
    fn view(&self) -> PhaseView<'_> {
        PhaseView {
            pr: self.pr,
            workspace: self.workspace.as_deref(),
            base: &self.base,
            forge: self.forge.as_ref(),
            vcs: self.vcs.as_ref(),
        }
    }
}

/// Refuses a phase the task's type does not list: its `[phases]` list is the one for the
/// ticket's type, and a task with no type or no list for it has no phase to resume.
fn check_phase(
    path: &str,
    composition: &Composition,
    ticket: &Ticket,
    phase: &str,
) -> Result<(), u8> {
    let kind = ticket.kind.as_ref().map(TicketType::as_str);
    let list = kind.and_then(|kind| composition.manifest().phase_list(kind));
    if list.is_some_and(|list| list.iter().any(|name| name.as_str() == phase)) {
        return Ok(());
    }
    let reason = match (kind, list) {
        (None, _) => format!("{} has no type, so no phase list", ticket.id),
        (Some(kind), None) => format!("[phases] has no list for {kind}"),
        (Some(kind), Some(list)) => {
            let listed: Vec<&str> = list.iter().map(ActionName::as_str).collect();
            format!("the {kind} list is {}", listed.join(", "))
        }
    };
    eprintln!(
        "zaf {path}: `{phase}` is not a phase of {}: {reason}",
        ticket.id
    );
    Err(REFUSED)
}

/// The recorded pull request of the task and what reads its change; none when the task has no
/// pull request, and then the pack is today's.
fn resume(
    path: &str,
    cli: &Cli,
    composition: &Composition,
    board: &dyn Board,
    task: TicketId,
) -> Result<Option<Resumed>, u8> {
    let records = SliceRecords::new(composition.root());
    let Some(pr) = records.pr(task) else {
        return Ok(None);
    };
    let query = SliceQuery {
        task,
        version: std::env::var(VERSION_VAR)
            .ok()
            .filter(|version| !version.is_empty()),
    };
    let target = slice::resolve(board, &composition.manifest().branches, &query)
        .map_err(|error| fail_slice(path, cli, &error))?;
    let forge = composition
        .forge()
        .map_err(|error| fail_composition(path, &error))?;
    let vcs = composition
        .vcs()
        .map_err(|error| fail_composition(path, &error))?;
    Ok(Some(Resumed {
        pr,
        workspace: records.workspace(task),
        base: target.base,
        forge,
        vcs,
    }))
}

/// Builds a task's context pack; with a phase, a resumed one when the task has a pull request.
pub fn run(cli: &Cli, task: TicketId, phase: Option<&str>) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return fail_composition(&path, &error),
    };
    let resumed = match phase {
        None => None,
        Some(phase) => {
            let ticket = match board.task(task) {
                Ok(ticket) => ticket,
                Err(error) => return fail(&path, &error),
            };
            if let Err(code) = check_phase(&path, &composition, &ticket, phase) {
                return code;
            }
            match resume(&path, cli, &composition, board.as_ref(), task) {
                Ok(resumed) => resumed,
                Err(code) => return code,
            }
        }
    };
    let design = match composition.design() {
        Ok(design) => design,
        Err(error) => return fail_composition(&path, &error),
    };
    let caller = match Caller::from_env() {
        Ok(caller) => caller,
        Err(reason) => {
            eprintln!("zaf {path}: {reason}");
            return REFUSED;
        }
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
    let home = zaf_home();
    let view = ActionsView {
        project_root: composition.root(),
        home: home.as_deref(),
        caller: &caller,
    };
    let phase_view = resumed.as_ref().map(Resumed::view);
    match brief_for_phase(&query, &ports, view, phase_view.as_ref()) {
        Ok(brief) if cli.json => {
            println!("{}", serde_json::json!(brief));
            OK
        }
        Ok(brief) => {
            println!("{}", brief.path.display());
            println!("~{} tokens", brief.tokens);
            OK
        }
        Err(BriefActionsError::Brief(error)) => fail_brief(&path, error, &composition),
        Err(error @ BriefActionsError::Actions(_)) => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
    }
}

/// Reports why no pack was written and returns the exit code.
fn fail_brief(path: &str, error: BriefError, composition: &Composition) -> u8 {
    match error {
        BriefError::Board(error) | BriefError::Forge(error) | BriefError::Vcs(error) => {
            fail(path, &error)
        }
        BriefError::Design(error) => {
            note_design_paths(path, composition);
            fail(path, &error)
        }
        BriefError::Instance(_) => {
            eprintln!("zaf {path}: {error}");
            REFUSED
        }
        BriefError::Write { .. } => {
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
