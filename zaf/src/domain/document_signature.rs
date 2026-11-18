//! A release document's signature block, read and written.
//!
//! The block sits at the very top of the document:
//!
//! ```text
//! ---
//! signed: 2026-11-17T10:00:00Z
//! hash: <64 lowercase hex digits>
//! ---
//! ```
//!
//! The hash is [`BodyHash::of`] over every byte after the closing line. A document is
//! signed while that hash still matches its body as it reads now; the stage is never stored.

use std::time::SystemTime;

use super::{BodyHash, TicketBody};

/// The line that opens and closes the block.
const FENCE: &str = "---";

/// What the top of a document says about its signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureState {
    /// A block whose hash matches the body below it.
    Signed {
        /// When it was signed.
        at: SystemTime,
        /// The hash the block holds, equal to the body's.
        hash: BodyHash,
    },
    /// No block, or a block whose hash differs from the body as it reads now.
    Unsigned,
    /// A block that does not read.
    Malformed {
        /// The 1-based line at fault; for a block that never closes, the line that opens it.
        line: usize,
        /// What is wrong there.
        reason: String,
    },
}

/// The signature block of a document: read it, write it, find the body below it.
pub struct DocumentSignature;

/// A block as it reads, and where the body starts.
struct Block {
    /// The time and hash, or the line at fault and why.
    read: Result<(SystemTime, BodyHash), (usize, String)>,
    /// The byte offset of the first byte after the closing line.
    body: usize,
}

impl DocumentSignature {
    /// What `text` says about its signature.
    pub fn read(text: &str) -> SignatureState {
        let Some(block) = Block::of(text) else {
            return SignatureState::Unsigned;
        };
        match block.read {
            Err((line, reason)) => SignatureState::Malformed { line, reason },
            Ok((at, hash)) => {
                if hash == Self::hash_of(&text[block.body..]) {
                    SignatureState::Signed { at, hash }
                } else {
                    SignatureState::Unsigned
                }
            }
        }
    }

    /// `text` with a fresh block above its body, replacing any block already there.
    pub fn sign(text: &str, at: SystemTime) -> String {
        let body = Self::body(text);
        let hash = Self::hash_of(body);
        let at = humantime::format_rfc3339_seconds(at);
        format!("{FENCE}\nsigned: {at}\nhash: {hash}\n{FENCE}\n{body}")
    }

    /// The bytes after the block; the whole `text` when it has none.
    pub fn body(text: &str) -> &str {
        match Block::of(text) {
            Some(block) => &text[block.body..],
            None => text,
        }
    }

    fn hash_of(body: &str) -> BodyHash {
        BodyHash::of(&TicketBody::from(body))
    }
}

impl Block {
    /// The block at the top of `text`; none when its first line is not a fence. A block that
    /// never closes is a block, malformed at its opening line, that cuts no body off.
    fn of(text: &str) -> Option<Self> {
        let mut lines = text.split_inclusive('\n');
        let first = lines.next()?;
        if content(first) != FENCE {
            return None;
        }
        let mut offset = first.len();
        let mut signed: Option<SystemTime> = None;
        let mut hash: Option<BodyHash> = None;
        let mut fault: Option<(usize, String)> = None;
        for (index, raw) in lines.enumerate() {
            let number = index + 2;
            offset += raw.len();
            let line = content(raw);
            if line == FENCE {
                let read = match (fault, signed, hash) {
                    (Some(fault), _, _) => Err(fault),
                    (None, Some(at), Some(hash)) => Ok((at, hash)),
                    (None, None, _) => Err((number, "no `signed` line".to_owned())),
                    (None, _, None) => Err((number, "no `hash` line".to_owned())),
                };
                return Some(Self { read, body: offset });
            }
            if fault.is_none()
                && let Err(why) = take(line, &mut signed, &mut hash)
            {
                fault = Some((number, why));
            }
        }
        Some(Self {
            read: Err((1, "no closing `---` line".to_owned())),
            body: 0,
        })
    }
}

/// A line without its terminator.
fn content(raw: &str) -> &str {
    raw.strip_suffix('\n')
        .map_or(raw, |line| line.strip_suffix('\r').unwrap_or(line))
}

/// Reads one `key: value` line into its slot.
fn take(
    line: &str,
    signed: &mut Option<SystemTime>,
    hash: &mut Option<BodyHash>,
) -> Result<(), String> {
    let (key, value) = line
        .split_once(':')
        .ok_or_else(|| format!("not a `key: value` line: `{line}`"))?;
    let value = value.trim();
    match key {
        "signed" => {
            if signed.is_some() {
                return Err("`signed` twice".to_owned());
            }
            let at = humantime::parse_rfc3339(value)
                .map_err(|_| format!("`{value}` is not an RFC 3339 UTC time"))?;
            *signed = Some(at);
        }
        "hash" => {
            if hash.is_some() {
                return Err("`hash` twice".to_owned());
            }
            let lowercase_hex = value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
            if value.len() != 64 || !lowercase_hex {
                return Err(format!("`{value}` is not 64 lowercase hex digits"));
            }
            *hash = Some(
                value
                    .parse()
                    .map_err(|_| format!("`{value}` is not a hash"))?,
            );
        }
        other => return Err(format!("unknown key `{other}`")),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
