//! The vocabulary block of a release's design: the text inside the fence ```` ```toml vocabulary ````
//! (D5, I3), read into the [`Vocabulary`] the plan takes.
//!
//! ```toml
//! [areas]
//! "prompts" = "the standing briefs"
//!
//! [names."mcp.catalog"]
//! signature = "fn catalog() -> Vec<Tool>"
//! epic = 1157
//!
//! [names."cli"]
//! no_signature = true
//! existing = "zaf/src/cli.rs"
//! ```
//!
//! Pure: it takes the text of the block and reads no file. Finding the block in a design, the
//! name checks against a plan and the path check of an `existing` name are other tasks'.
//!
//! An entry needs `signature` or `no_signature = true`, and may have `epic` (kept in
//! [`Vocabulary::epic`]: the epic the name is given to) or `existing`. A name
//! with `existing` is code already: it goes to [`Vocabulary::existing`] alone, as the fixtures
//! hold it, whatever its own kind. Any other name goes to [`Vocabulary::signature`] or
//! [`Vocabulary::no_signature`].

use std::fmt;

use super::{Area, Name, Vocabulary};

/// The keys of a name entry.
const ENTRY_KEYS: [&str; 4] = ["signature", "no_signature", "epic", "existing"];

/// The keys of the block.
const BLOCK_KEYS: [&str; 2] = ["areas", "names"];

/// Why a vocabulary block is refused. Each message is one line and names the entry, key or area.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VocabularyError {
    /// The block is not valid TOML.
    #[error("the vocabulary block is not valid TOML: {0}")]
    NotToml(String),
    /// A key the block does not have, at the top or in an entry.
    #[error("unknown key `{key}` in {place}")]
    UnknownKey {
        /// The key.
        key: String,
        /// Where: the block, or `name "<n>"`.
        place: Place,
    },
    /// An area whose name holds anything but lowercase letters, digits, `.`, `-`, `/` and `_`.
    #[error("bad area `{0}`: an area holds lowercase letters, digits, `.`, `-`, `/` and `_`")]
    BadArea(String),
    /// A name that does the same.
    #[error("bad name `{0}`: a name holds lowercase letters, digits, `.`, `-`, `/` and `_`")]
    BadName(String),
    /// An entry with both `signature` and `no_signature`.
    #[error("name \"{0}\" has both signature and no_signature")]
    BothSignatureKinds(String),
    /// An entry with neither.
    #[error("name \"{0}\" has neither signature nor no_signature")]
    NoSignatureKind(String),
    /// An entry with both `epic` and `existing`.
    #[error("name \"{0}\" has both epic and existing")]
    BothProviders(String),
    /// A value of the wrong type.
    #[error("{place}: `{key}` must be {wanted}")]
    BadValue {
        /// Where.
        place: Place,
        /// The key.
        key: String,
        /// What it must be.
        wanted: &'static str,
    },
}

/// Where in the block a refusal is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// The block itself.
    Block,
    /// `[areas]`.
    Areas,
    /// `[names."<n>"]`.
    Name(String),
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Place::Block => f.write_str("the vocabulary block"),
            Place::Areas => f.write_str("[areas]"),
            Place::Name(name) => write!(f, "name \"{name}\""),
        }
    }
}

/// Whether a name or an area is written the way D5 asks.
fn well_formed(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '/' | '_')
        })
}

/// Reads the text of a vocabulary block into the plan's vocabulary.
pub fn parse(block: &str) -> Result<Vocabulary, VocabularyError> {
    let table: toml::Table = block
        .parse()
        .map_err(|error: toml::de::Error| VocabularyError::NotToml(one_line(&error.to_string())))?;
    if let Some(key) = table.keys().find(|key| !BLOCK_KEYS.contains(&key.as_str())) {
        return Err(VocabularyError::UnknownKey {
            key: key.clone(),
            place: Place::Block,
        });
    }
    let mut vocabulary = Vocabulary::default();
    if let Some(areas) = table.get("areas") {
        let areas = areas.as_table().ok_or(VocabularyError::BadValue {
            place: Place::Block,
            key: "areas".to_owned(),
            wanted: "a table",
        })?;
        for (area, note) in areas {
            if !well_formed(area) {
                return Err(VocabularyError::BadArea(area.clone()));
            }
            if !note.is_str() {
                return Err(VocabularyError::BadValue {
                    place: Place::Areas,
                    key: area.clone(),
                    wanted: "a note, a string",
                });
            }
            vocabulary.areas.insert(Area::from(area.as_str()));
        }
    }
    if let Some(names) = table.get("names") {
        let names = names.as_table().ok_or(VocabularyError::BadValue {
            place: Place::Block,
            key: "names".to_owned(),
            wanted: "a table",
        })?;
        for (name, entry) in names {
            read_entry(&mut vocabulary, name, entry)?;
        }
    }
    Ok(vocabulary)
}

/// One `[names."<n>"]` entry into its list.
fn read_entry(
    vocabulary: &mut Vocabulary,
    name: &str,
    entry: &toml::Value,
) -> Result<(), VocabularyError> {
    if !well_formed(name) {
        return Err(VocabularyError::BadName(name.to_owned()));
    }
    let place = || Place::Name(name.to_owned());
    let bad = |key: &str, wanted: &'static str| VocabularyError::BadValue {
        place: place(),
        key: key.to_owned(),
        wanted,
    };
    let entry = entry.as_table().ok_or_else(|| {
        bad(
            name,
            "a table of signature or no_signature, and epic or existing",
        )
    })?;
    if let Some(key) = entry.keys().find(|key| !ENTRY_KEYS.contains(&key.as_str())) {
        return Err(VocabularyError::UnknownKey {
            key: key.clone(),
            place: place(),
        });
    }
    if let Some(shape) = entry.get("signature")
        && !shape.as_str().is_some_and(|shape| !shape.is_empty())
    {
        return Err(bad("signature", "a non-empty string"));
    }
    if let Some(flag) = entry.get("no_signature")
        && flag.as_bool() != Some(true)
    {
        return Err(bad("no_signature", "true"));
    }
    if let Some(epic) = entry.get("epic")
        && !epic.as_integer().is_some_and(|n| n > 0)
    {
        return Err(bad("epic", "a whole number of at least 1"));
    }
    if let Some(path) = entry.get("existing")
        && !path.as_str().is_some_and(|path| !path.is_empty())
    {
        return Err(bad("existing", "a non-empty path string"));
    }
    let signature = entry.contains_key("signature");
    let no_signature = entry.contains_key("no_signature");
    if signature && no_signature {
        return Err(VocabularyError::BothSignatureKinds(name.to_owned()));
    }
    if !signature && !no_signature {
        return Err(VocabularyError::NoSignatureKind(name.to_owned()));
    }
    let existing = entry.contains_key("existing");
    if existing && entry.contains_key("epic") {
        return Err(VocabularyError::BothProviders(name.to_owned()));
    }
    let name = Name::from(name);
    if let Some(epic) = entry.get("epic").and_then(toml::Value::as_integer)
        && let Ok(epic) = u64::try_from(epic)
    {
        vocabulary.epic.insert(name.clone(), epic.into());
    }
    if existing {
        vocabulary.existing.insert(name);
    } else if signature {
        vocabulary.signature.insert(name);
    } else {
        vocabulary.no_signature.insert(name);
    }
    Ok(())
}

/// A parser's message on one line.
fn one_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
