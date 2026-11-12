//! `zaf forge open|merge|checks|head|log`: the forge's writes and the reads the standard actions
//! wait on, for whichever forge `[forge]` names.
//!
//! `open` prints the pull request's number, and says on stderr when one was already open for the
//! head. `merge` prints nothing, merged now or before. `checks` prints one line per check,
//! `<bucket>\t<name>\t<link>`, the bucket one of `pass`, `fail`, `pending`, `skipping` or
//! `cancel` and the link empty when the forge gives none. `head` prints the head commit, or
//! nothing when the forge reports none. `log` prints what a person reads of a failed check's log,
//! or nothing when its link names no job. A refusal exits 1, a backend failure 2; a merge the
//! forge cannot make yet says `not mergeable yet`, so an action can wait and try again.

use serde_json::json;

use super::exit::{self, OK, REFUSED};
use super::{Cli, ForgeAction};
use crate::app::forge::{self, ForgeError};
use crate::composition::Composition;
use crate::ports::{
    BranchName, CheckLink, CheckName, MergeOutcome, MergeStrategy, NewPullRequest, Opened, PrBody,
    PrTitle,
};

/// Runs the verb.
pub fn run(cli: &Cli, action: &ForgeAction) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let forge = match composition.forge() {
        Ok(forge) => forge,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let forge = forge.as_ref();
    let done: Result<(), ForgeError> = match action {
        ForgeAction::Open {
            head,
            base,
            title,
            body,
        } => {
            let request = NewPullRequest {
                head: BranchName::from(head.as_str()),
                base: BranchName::from(base.as_str()),
                title: PrTitle::from(title.as_str()),
                body: PrBody::from(body.as_str()),
            };
            forge::open(forge, &request).map(|opened| {
                let pr = opened.number();
                if let Opened::AlreadyOpen(_) = opened {
                    eprintln!("pull request {pr} is already open for {head}");
                }
                if cli.json {
                    let new = matches!(opened, Opened::New(_));
                    println!("{}", json!({ "pr": pr.get(), "opened": new }));
                } else {
                    println!("{}", pr.get());
                }
            })
        }
        ForgeAction::Merge {
            pr,
            squash,
            match_head_commit,
        } => {
            let strategy = if *squash {
                MergeStrategy::Squash
            } else {
                MergeStrategy::Merge
            };
            forge::merge(forge, *pr, strategy, match_head_commit.as_deref()).map(|outcome| {
                if cli.json {
                    let now = outcome == MergeOutcome::Merged;
                    println!("{}", json!({ "pr": pr.get(), "merged_now": now }));
                }
            })
        }
        ForgeAction::Checks { pr } => forge::checks(forge, *pr)
            .map(|runs| {
                if cli.json {
                    let rows: Vec<_> = runs
                        .iter()
                        .map(|run| {
                            json!({
                                "bucket": run.bucket.to_string(),
                                "name": run.name.as_str(),
                                "link": run.link.as_ref().map(CheckLink::as_str),
                            })
                        })
                        .collect();
                    println!("{}", json!(rows));
                } else {
                    for run in &runs {
                        println!("{}", forge::check_line(run));
                    }
                }
            })
            .map_err(ForgeError::from),
        ForgeAction::Head { pr } => forge::head(forge, *pr)
            .map(|head| {
                if cli.json {
                    println!("{}", json!({ "pr": pr.get(), "head": head }));
                } else if let Some(head) = head {
                    println!("{head}");
                }
            })
            .map_err(ForgeError::from),
        ForgeAction::Log { name, link } => forge::log(
            forge,
            &CheckName::from(name.as_str()),
            &CheckLink::from(link.as_str()),
        )
        .map(|text| {
            if cli.json {
                println!("{}", json!({ "log": text }));
            } else if let Some(text) = text {
                print!("{text}");
            }
        })
        .map_err(ForgeError::from),
    };
    match done {
        Ok(()) => OK,
        Err(ForgeError::Port(error)) => exit::fail(&path, &error),
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            REFUSED
        }
    }
}
