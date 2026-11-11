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
pub use metadata::{
    ActionKind, ActionMetadata, ActionMode, ActionName, Bits, MetadataError, ParsedAction,
    PhaseEnd, Target, parse,
};
pub use phase::{
    CheckResult, ListedPhase, PHASES_DIR, Phase, PhaseFacts, PhaseRun, PhaseUnreadable, PrFacts,
    PullFacts, ReviewFact, RunExit, Standing, StandingFacts, StandingUnreadable, WaitFailure,
    WorkspaceFacts, derive_phase, derive_standing, gather_phase_facts, gather_standing_facts,
    running_tasks, wait_failure,
};
pub use records::{SliceRecords, hand_back};
pub use resolve::{
    ACTIONS_DIR, HOOK_EVENTS, HOOKS_DIR, ResolveError, Resolved, Tier, can_shadow, check_name,
    hook, hook_file, hook_file_in, list, list_hooks, resolve, resolve_in, same_file,
};
pub use run::{
    ACTION_VAR, CHAIN_VAR, DEPTH_VAR, LOG_VAR, ListedAction, MAX_DEPTH, MINUTE, NO_SUCH_ACTION,
    NOT_RUN_EXIT, Nesting, ROOT_VAR, Refused, RunOutcome, RunRequest, RunResult, TIMEOUT_EXIT,
    forbidden_action, last_lines, list_actions, read_action, refuse_forbidden, run_action,
    run_hook,
};
pub use slice_env::{BASE_VAR, PR_VAR, SLICE_VAR, SliceContext, TASK_TITLE_VAR};

#[cfg(test)]
mod tests;
