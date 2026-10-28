//! Context profiles: a saved turn-one context per role.

/// The profile an instance runs with; it fixes everything loaded before the brief.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    strum::Display,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::VariantArray,
    serde::Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Short roles: no servers, no plugins, one skill, a one-page interface.
    Lean,
    /// Planning, team and release instances: the pack only, trimmed memory.
    Build,
    /// The Engineer's attended session: everything.
    Design,
    /// An outside reviewer on its own settings.
    Review,
}

#[cfg(test)]
mod tests;
