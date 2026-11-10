//! The record a slotted gate run keeps while it holds the machine's build slot.

/// The folder under the state folder that holds one record per slotted run, named by pid.
pub const SLOTS_DIR: &str = "slots";

/// Who holds the slot, for which gate level, since when: the content of `.zaf/slots/<pid>`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SlotRecord {
    /// The instance from `ZAF_INSTANCE`, else `the Engineer`.
    pub holder: String,
    /// The gate level: `full`, `light` or `tests`.
    pub level: String,
    /// When the run took the slot, RFC 3339.
    pub since: String,
    /// When the holding process started, as the process table reports it: with the pid in
    /// the file name it tells a live holder from a reused pid.
    pub start: String,
}
