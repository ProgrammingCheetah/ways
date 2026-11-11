//! Step 7 of the tick: run the `post-tick` hook with what the tick did (docs/zaf-actions.md §5.1,
//! #421 ruling 12).
//!
//! Every tick ends here, idle ones too. The hook gets the tick's [`TickOutcome`] as one JSON
//! object on stdin, [`input`]:
//!
//! `{"started": {"task": <n>, "action": "<name>", "units": <u>} | null, "waits": [{"task": <n>, "action": "<name>"}], "ran": [{"task": <n>, "action": "<name>", "exit": <code>}], "questions": [<n>], "idle": <bool>}`
//!
//! There is no exit code of the model, because the tick never waits for it. A hook that fails is
//! reported through `log` and changes nothing: [`run`] gives back nothing for the tick to act on,
//! so the tick's exit code is whatever it was before the hook ran.
//!
//! The module is pure: the caller hands in the `hook` that runs the file with the text on its
//! stdin, and a `log` for what failed.

use super::outcome::TickOutcome;

/// What the `post-tick` hook gets on stdin: the outcome as one JSON object, one line.
pub fn input(outcome: &TickOutcome) -> String {
    format!("{}\n", outcome.to_json())
}

/// Runs the `post-tick` hook for `outcome`. `hook(input)` runs the hook with `input` on its stdin
/// and gives `Err(why)` when it failed; the failure goes to `log` and nothing else happens.
pub fn run(
    outcome: &TickOutcome,
    hook: &mut dyn FnMut(&str) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) {
    if let Err(why) = hook(&input(outcome)) {
        log(&format!("the post-tick hook failed: {why}"));
    }
}

#[cfg(test)]
mod tests;
