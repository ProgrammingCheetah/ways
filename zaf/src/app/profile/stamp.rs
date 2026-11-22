//! The stamp a generated profile carries: what it was made from.
//!
//! `zaf profile init` writes `.zaf/profiles/<name>/.zaf-stamp.json` last:
//! `{"zaf": "<version>", "generation": <n>, "parts": {"<part>": "<digest>", ...}}`, the version
//! of zaf that generated the profile, the [`GENERATION`] of what `init` writes and one digest per
//! input the generator reads ([`PARTS`]). [`stale`] compares each generated profile's stamp with
//! the live manifest and this zaf; doctor, status and the spawner call it, so a profile built
//! from an older manifest (or an older generation) is named instead of run with yesterday's deny
//! list.
//!
//! A stamp is stale when its generation differs from this zaf's or is missing (stale once, for
//! stamps written before the number existed), or when an input's digest differs. The zaf version
//! alone no longer makes it stale: a profile another zaf wrote stays current while the
//! generation and every digest match. The version is kept to word the reason.
//!
//! A digest is FNV-1a over the part's text, the debug text (`{:?}`) of the manifest values for
//! most parts. That text has to stay the same from one zaf to the next, or every profile reads
//! as older than the manifest after an upgrade. The test
//! `stamp_digests_of_a_fixed_manifest_do_not_move` pins each part's digest for one fixed
//! manifest: when it fails, raise `GENERATION` and fix its values together. It guards the
//! parts it covers; a manifest field it does not hold, or a change only a different value
//! shows, is still unguarded. The bytes are deterministic, so a rerun with nothing changed
//! writes nothing.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use super::{GENERATION, generated, home, read_project_rules};
use crate::domain::Profile;
use crate::manifest::{Manifest, VcsKind};

/// The stamp file, under a profile's home.
pub const STAMP_FILE: &str = ".zaf-stamp.json";

/// The inputs the generator reads, one stamped part each.
pub const PARTS: [&str; 6] = [
    "requires",
    "gate",
    "forbidden",
    "roles",
    "pack",
    "roles/project.md",
];

/// What a profile records about what it was made from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Stamp {
    /// The zaf version that generated the profile.
    zaf: String,
    /// The profile generation `init` wrote; none in a stamp written before it existed.
    #[serde(default)]
    generation: Option<u32>,
    /// Part name to digest.
    parts: BTreeMap<String, String>,
}

/// FNV-1a, 64 bits, as 16 hex digits.
fn digest(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The digest of each input the generator reads, by part name.
///
/// `project_rules` is the text of `roles/project.md`, when there is one. Only what the generator
/// reads counts: the `[requires]` tool names, the gate lines, the forbidden commands, each
/// role's profile, model and effort, the `[pack]` section and the project's rules.
pub(super) fn parts(manifest: &Manifest, project_rules: Option<&str>) -> BTreeMap<String, String> {
    let mut requires: Vec<String> = manifest.requires.keys().map(ToString::to_string).collect();
    // A git project's rules differ from a jj or plain one's, so its profile is stale when the
    // kind changes. Only git adds a word, so every other project's digest stays what it was.
    if manifest
        .vcs
        .as_ref()
        .is_some_and(|vcs| vcs.kind == VcsKind::Git)
    {
        requires.push("[vcs] git".to_owned());
    }
    let gate = manifest
        .gate
        .as_ref()
        .map(|gate| (&gate.full, &gate.light, &gate.tests));
    let roles: Vec<String> = manifest
        .roles
        .iter()
        .map(|(name, role)| {
            format!(
                "{name} {} {} {}",
                role.profile,
                role.model.as_ref() as &str,
                role.effort
            )
        })
        .collect();
    let rules = project_rules.map(str::trim).unwrap_or_default();
    BTreeMap::from([
        ("requires", format!("{requires:?}")),
        ("gate", format!("{gate:?}")),
        ("forbidden", format!("{:?}", manifest.forbidden.commands)),
        ("roles", format!("{roles:?}")),
        ("pack", format!("{:?}", manifest.pack)),
        ("roles/project.md", rules.to_owned()),
    ])
    .into_iter()
    .map(|(part, text)| (part.to_owned(), digest(&text)))
    .collect()
}

/// The bytes of the stamp for a profile generated now by this zaf from `manifest`.
pub(super) fn bytes(manifest: &Manifest, project_rules: Option<&str>) -> Vec<u8> {
    let stamp = Stamp {
        zaf: env!("CARGO_PKG_VERSION").to_owned(),
        generation: Some(GENERATION),
        parts: parts(manifest, project_rules),
    };
    let mut text = serde_json::to_string_pretty(&stamp).unwrap_or_default();
    text.push('\n');
    text.into_bytes()
}

/// Stamps every generated profile of `manifest` under `state_dir` as `init` would, making each
/// home as it goes: for tests of other verbs that need current profiles without running `init`.
#[cfg(test)]
pub fn write_stamps(state_dir: &Path, manifest: &Manifest) {
    let rules = read_project_rules(state_dir).ok().flatten();
    for profile in generated(manifest) {
        let home = home(state_dir, profile);
        fs::create_dir_all(&home).expect("profile home");
        fs::write(home.join(STAMP_FILE), bytes(manifest, rules.as_deref())).expect("stamp");
    }
}

/// Why a profile is stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The profile has no readable stamp.
    NoStamp,
    /// It was written under another profile generation, or under none, by the zaf version held.
    Generation(String),
    /// This input changed since the profile was made; it reads `<part> changed`.
    Changed(String),
}

impl fmt::Display for Reason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoStamp => write!(formatter, "no stamp"),
            Self::Generation(old) => write!(formatter, "written by zaf {old}"),
            Self::Changed(part) => write!(formatter, "{part} changed"),
        }
    }
}

/// A generated profile that no longer matches what it would be generated from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StaleProfile {
    /// The profile's name.
    #[serde(serialize_with = "as_text")]
    pub profile: Profile,
    /// Why it is stale, as [`Reason`] words.
    #[serde(serialize_with = "each_as_text")]
    pub reasons: Vec<Reason>,
}

fn as_text<S: serde::Serializer>(profile: &Profile, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(profile)
}

fn each_as_text<S: serde::Serializer>(
    reasons: &[Reason],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(reasons.iter().map(ToString::to_string))
}

impl StaleProfile {
    /// The line doctor, status and the spawner print:
    /// `profile <name> is older than the manifest (forbidden changed)`,
    /// `profile <name> is of another profile generation (written by zaf <old>): run zaf profile init`
    /// or `profile <name> has no stamp: run zaf profile init`.
    pub fn line(&self) -> String {
        let name = &self.profile;
        format!("profile {name} {}{}", self.predicate(false), self.fix())
    }

    /// What follows the words when the profile has no stamp or is of another generation: the
    /// fix. Other reasons end where their words do.
    fn fix(&self) -> &'static str {
        let regenerate = self
            .reasons
            .iter()
            .any(|reason| matches!(reason, Reason::NoStamp | Reason::Generation(_)));
        if regenerate {
            ": run zaf profile init"
        } else {
            ""
        }
    }
}

impl StaleProfile {
    /// What is said of this profile after its name, in the singular or the plural:
    /// `is of another profile generation (written by zaf 0.4.3)`, `has no stamp`. Profiles that
    /// share these words share a reason.
    fn predicate(&self, plural: bool) -> String {
        if self.reasons.contains(&Reason::NoStamp) {
            return format!("{} no stamp", if plural { "have" } else { "has" });
        }
        let words: Vec<String> = self.reasons.iter().map(ToString::to_string).collect();
        let verb = if plural { "are" } else { "is" };
        let generation = self
            .reasons
            .iter()
            .any(|reason| matches!(reason, Reason::Generation(_)));
        let what = if generation {
            "of another profile generation"
        } else {
            "older than the manifest"
        };
        format!("{verb} {what} ({})", words.join(", "))
    }
}

/// One sentence for all of a project's stale profiles, grouped by reason, in the order each
/// reason first appears; none when none is stale.
///
/// Profiles that share a reason are named together: `profiles build and lean are of another
/// profile generation (written by zaf 0.4.3)`. Several reasons are joined with `; `:
/// `profile build has no stamp; profile lean is older than the manifest (forbidden changed)`.
/// One stale profile reads as its [`StaleProfile::line`].
pub fn stale_line(stale: &[StaleProfile]) -> Option<String> {
    if let [only] = stale {
        return Some(only.line());
    }
    let mut groups: Vec<(String, Vec<&StaleProfile>)> = Vec::new();
    for profile in stale {
        let reason = profile.predicate(false);
        match groups.iter_mut().find(|(known, _)| *known == reason) {
            Some((_, members)) => members.push(profile),
            None => groups.push((reason, vec![profile])),
        }
    }
    let lone = groups.len() == 1;
    let sentences: Vec<String> = groups
        .iter()
        .map(|(_, members)| {
            let names: Vec<String> = members.iter().map(|m| m.profile.to_string()).collect();
            let plural = names.len() > 1;
            let predicate = members[0].predicate(plural);
            let fix = if lone { members[0].fix() } else { "" };
            format!(
                "{} {} {predicate}{fix}",
                if plural { "profiles" } else { "profile" },
                names_in_words(&names)
            )
        })
        .collect();
    (!sentences.is_empty()).then(|| sentences.join("; "))
}

/// `a`, `a and b`, `a, b and c`.
fn names_in_words(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// `profile::stale(state_dir, manifest) -> Vec<StaleProfile>`: the generated profiles whose stamp
/// is missing or differs from what this zaf would write now from `manifest` and the project's
/// rules under `state_dir`, in profile order, each with its reasons: `no stamp`,
/// `written by zaf <old>`, `<part> changed`.
///
/// A stamp from another zaf version is current while its generation and every digest match; one
/// of another generation, or with none, is stale for that alone: its digests mean nothing here.
pub fn stale(state_dir: &Path, manifest: &Manifest) -> Vec<StaleProfile> {
    let rules = read_project_rules(state_dir).ok().flatten();
    let live = parts(manifest, rules.as_deref());
    generated(manifest)
        .into_iter()
        .filter_map(|profile| {
            let reasons = reasons(&home(state_dir, profile).join(STAMP_FILE), &live);
            (!reasons.is_empty()).then_some(StaleProfile { profile, reasons })
        })
        .collect()
}

/// [`stale`] without the profiles whose home is not there yet: doctor and status leave a profile
/// that was never generated to their pack check and the worksheet, and name only one that exists
/// and is out of date. The spawner uses [`stale`], which refuses a missing home too: an instance
/// started in an empty home would run with no deny list.
pub fn stale_homes(state_dir: &Path, manifest: &Manifest) -> Vec<StaleProfile> {
    stale(state_dir, manifest)
        .into_iter()
        .filter(|stale| home(state_dir, stale.profile).is_dir())
        .collect()
}

/// What differs between the stamp at `file` and the `live` parts; empty when none does.
fn reasons(file: &Path, live: &BTreeMap<String, String>) -> Vec<Reason> {
    let Some(stamp) = fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Stamp>(&text).ok())
    else {
        return vec![Reason::NoStamp];
    };
    if stamp.generation != Some(GENERATION) {
        return vec![Reason::Generation(stamp.zaf)];
    }
    live.iter()
        .filter(|(part, digest)| stamp.parts.get(*part) != Some(*digest))
        .map(|(part, _)| Reason::Changed(part.clone()))
        .collect()
}
