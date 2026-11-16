//! Actions: the file format, the two tiers and the runner (docs/zaf-actions.md §1 to §3, §5).
//!
//! An action is a runnable file whose opening comment block names it to zaf:
//! [`metadata`] parses that block, [`resolve`] finds the file a name stands for
//! in the project's `.zaf/actions/` or the machine's, [`caller`] decides whether
//! its mode lets the caller run it, and [`run`] runs it, logs it and bounds it in
//! time, or hands it the caller's terminal when it declares `terminal: yes`. [`phase`] derives the Team Phase an action's `phases:` line is held
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
    ActionKind, ActionMetadata, ActionMode, ActionName, Bits, ListedPhase, MetadataError,
    ParsedAction, PhaseEnd, Target, parse,
};
pub use phase::{
    LivePhase, PHASES_DIR, Phase, PhaseFacts, PhaseUnreadable, PrFacts, WorkspaceFacts,
    derive_phase, ended_instances, gather_phase_facts, live_phases, running_tasks,
};
pub use records::{RecordUnreadable, SliceRecords, hand_back};
pub use resolve::{
    ACTIONS_DIR, HOOK_EVENTS, HOOKS_DIR, ResolveError, Resolved, Tier, can_shadow, check_name,
    hook, hook_file, hook_file_in, list, list_hooks, resolve, resolve_in, same_file,
};
pub use run::{
    ACTION_VAR, CHAIN_VAR, DEPTH_VAR, HOOK_HAS_NO_TERMINAL, LOG_VAR, ListedAction, MAX_DEPTH,
    MINUTE, NO_SUCH_ACTION, NO_TERMINAL, NOT_RUN_EXIT, Nesting, ROOT_VAR, Refused, RunOutcome,
    RunRequest, RunResult, RunStep, RunsAs, StdioPlan, TIMEOUT_EXIT, Terminal, forbidden_action,
    last_lines, list_actions, read_action, refuse_forbidden, run_action, run_hook,
    run_hook_with_input, stdio_plan,
};
pub use slice_env::{BASE_VAR, PR_VAR, SLICE_VAR, SliceContext, TASK_TITLE_VAR, VERSION_VAR};

#[cfg(test)]
mod tests;
