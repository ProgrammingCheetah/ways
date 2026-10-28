//! `zaf changes`: a task's change against its base, stat first, hunks with `--path`.

use serde_json::json;

use super::Cli;
use super::exit::fail_composition;
use super::slice::{fail_slice, prepare, print};
use crate::app::changes::{self, Changes, ChangesQuery};
use crate::domain::IssueNumber;

/// Shows a task's change: stat first, hunks on request.
pub fn run(cli: &Cli, task: IssueNumber, path: Option<&str>) -> u8 {
    let verb = cli.verb.path();
    let (composition, target) = match prepare(&verb, cli, task) {
        Ok(prepared) => prepared,
        Err(code) => return code,
    };
    let vcs = match composition.vcs() {
        Ok(vcs) => vcs,
        Err(error) => return fail_composition(&verb, &error),
    };
    let query = ChangesQuery {
        target,
        path: path.map(str::to_owned),
    };
    match changes::changes(vcs.as_ref(), &query) {
        Ok(Changes::Stat(stat)) => {
            let files: Vec<_> = stat
                .files
                .iter()
                .map(|file| json!({ "path": file.path, "added": file.added, "removed": file.removed }))
                .collect();
            let json = json!({
                "task": task.get(),
                "slice": query.target.slice.as_ref(),
                "files": files,
                "added": stat.files.iter().map(|file| file.added).sum::<u32>(),
                "removed": stat.files.iter().map(|file| file.removed).sum::<u32>(),
            });
            print(cli, &json, stat.to_string().trim_end())
        }
        Ok(Changes::Hunks { path, hunks }) => {
            let json = json!({ "task": task.get(), "path": path, "hunks": hunks });
            print(cli, &json, hunks.trim_end())
        }
        Err(error) => fail_slice(&verb, cli, &error),
    }
}
