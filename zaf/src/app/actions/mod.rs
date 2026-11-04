//! Actions: the file format and the two tiers (docs/zaf-actions.md §1 to §3).
//!
//! An action is a runnable file whose opening comment block names it to zaf:
//! [`metadata`] parses that block, [`resolve`] finds the file a name stands for
//! in the project's `.zaf/actions/` or the machine's. zaf ships no actions. A
//! pure use case over the file system; running an action is not here.

mod metadata;
mod resolve;

pub use metadata::{
    ActionMetadata, ActionMode, Bits, MetadataError, ParsedAction, Phase, Target, parse,
};
pub use resolve::{
    ACTIONS_DIR, ResolveError, Resolved, Tier, check_name, list, resolve, resolve_in,
};

#[cfg(test)]
mod tests;
