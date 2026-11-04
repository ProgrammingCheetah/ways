//! Actions: the file format, the two tiers and the runner (docs/zaf-actions.md §1 to §3, §5).
//!
//! An action is a runnable file whose opening comment block names it to zaf:
//! [`metadata`] parses that block, [`resolve`] finds the file a name stands for
//! in the project's `.zaf/actions/` or the machine's, and [`run`] runs it, logs
//! it and bounds it in time. zaf ships no actions.

mod metadata;
mod resolve;
mod run;

pub use metadata::{
    ActionMetadata, ActionMode, Bits, MetadataError, ParsedAction, Phase, Target, parse,
};
pub use resolve::{
    ACTIONS_DIR, ResolveError, Resolved, Tier, check_name, list, resolve, resolve_in,
};
pub use run::{
    ACTION_VAR, CHAIN_VAR, DEPTH_VAR, LOG_VAR, ListedAction, MAX_DEPTH, MINUTE, NOT_RUN_EXIT,
    Nesting, ROOT_VAR, RunOutcome, RunRequest, RunResult, TIMEOUT_EXIT, find_action, last_lines,
    list_actions, run_action,
};

#[cfg(test)]
mod tests;
