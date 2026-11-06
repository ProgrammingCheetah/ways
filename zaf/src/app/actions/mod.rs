//! Actions: the file format, the two tiers and the runner (docs/zaf-actions.md §1 to §3, §5).
//!
//! An action is a runnable file whose opening comment block names it to zaf:
//! [`metadata`] parses that block, [`resolve`] finds the file a name stands for
//! in the project's `.zaf/actions/` or the machine's, [`caller`] decides whether
//! its mode lets the caller run it, and [`run`] runs it, logs it and bounds it in
//! time. [`phase`] derives the Team Phase an action's `phases:` line is held
//! against, [`records`] keeps what `slice-open` and `slice-pr` hand back, and [`slice_env`]
//! sets the slice's variables on a run, last. zaf ships no actions.

mod caller;
mod metadata;
mod phase;
mod records;
mod resolve;
mod run;
mod slice_env;

pub use caller::{Caller, INSTANCE_VAR};
pub use metadata::{ActionMetadata, ActionMode, Bits, MetadataError, ParsedAction, Target, parse};
pub use phase::{
    Phase, PhaseFacts, PhaseUnreadable, PrFacts, WorkspaceFacts, derive_phase, gather_phase_facts,
};
pub use records::{SliceRecords, hand_back};
pub use resolve::{
    ACTIONS_DIR, ResolveError, Resolved, Tier, can_shadow, check_name, list, resolve, resolve_in,
    same_file,
};
pub use run::{
    ACTION_VAR, CHAIN_VAR, DEPTH_VAR, LOG_VAR, ListedAction, MAX_DEPTH, MINUTE, NOT_RUN_EXIT,
    Nesting, ROOT_VAR, Refused, RunOutcome, RunRequest, RunResult, TIMEOUT_EXIT, forbidden_action,
    last_lines, list_actions, read_action, refuse_forbidden, run_action,
};
pub use slice_env::{BASE_VAR, PR_VAR, SLICE_VAR, SliceContext, TASK_TITLE_VAR};

#[cfg(test)]
mod tests;
