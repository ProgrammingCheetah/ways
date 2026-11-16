//! The hub's guard (docs/zaf-actions.md §5, epic #1 §7): a page that can act must not act for
//! anyone on the LAN, nor for a model on this machine.
//!
//! When the hub starts it makes a [`Key`] from the operating system's random source and keeps it
//! in memory only. Every `POST /api/<control>` must carry it in the [`KEY_HEADER`] request header;
//! the [`Guard`] answers whether one does, before the request is given any work. A request with no
//! key or the wrong one is refused and changes nothing. Reading stays open.
//!
//! The key is never written: it is not in a file, a log, a run folder, the page's source or a
//! `Debug` print. The one way out of this module is [`Guard::link`], the link the Engineer is
//! handed; the key rides in the link's fragment, which a browser keeps to itself and never sends
//! in a request, so no access log of the hub's own or of a proxy holds it.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};

use sha2::{Digest, Sha256};

/// The request header that carries the key: `X-Zaf-Key: <key>`. A header, not a query or a path,
/// so the key is in no URL a log would keep, and a page on another origin cannot send it without
/// a preflight, which the hub refuses.
pub const KEY_HEADER: &str = "X-Zaf-Key";

/// The name of the fragment parameter that carries the key in the link: `<page>#key=<key>`.
pub const LINK_PARAMETER: &str = "key";

/// The random bytes in a key: 256 bits, which nobody on a LAN guesses.
const KEY_BYTES: usize = 32;

/// The operating system's random source, on Linux and macOS.
const RANDOM_SOURCE: &str = "/dev/urandom";

/// The key a running hub made when it started.
///
/// It has no `Clone`, no `Display` and a `Debug` that hides it, so it does not leave by accident.
pub struct Key(String);

impl Key {
    /// Makes a key from the operating system's random source. Fails when that cannot be read, and
    /// a hub that cannot make a key serves no controls: it never falls back on a guessable one.
    pub fn generate() -> io::Result<Self> {
        let mut bytes = [0_u8; KEY_BYTES];
        File::open(RANDOM_SOURCE)?.read_exact(&mut bytes)?;
        Ok(Self::from_bytes(&bytes))
    }

    fn from_bytes(bytes: &[u8; KEY_BYTES]) -> Self {
        Self(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// Whether `presented` is this key: the interface `check(&self, presented) -> bool`, where
    /// `presented` is what the [`KEY_HEADER`] carried and `None` is a request that carried none.
    /// The compare takes the same time wherever the two differ: both sides are hashed to the same
    /// length, and every byte of the two digests is looked at before the answer is made.
    pub fn check(&self, presented: Option<&str>) -> bool {
        let Some(presented) = presented else {
            return false;
        };
        let ours = Sha256::digest(self.0.as_bytes());
        let theirs = Sha256::digest(presented.as_bytes());
        let difference = ours
            .iter()
            .zip(theirs.iter())
            .fold(0_u8, |difference, (ours, theirs)| {
                difference | (ours ^ theirs)
            });
        std::hint::black_box(difference) == 0
    }

    /// The key itself. Only what hands it to the Engineer reads it.
    pub(crate) fn reveal(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(..)")
    }
}

/// Why a control was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The hub serves no controls: it was started with `--read-only`.
    #[error("this hub is read-only: it serves no controls")]
    ReadOnly,
    /// The request carried no key, or not the hub's. One sentence for both, so a refusal says
    /// nothing about what was close.
    #[error("a control needs the key this hub made when it started, in the X-Zaf-Key header")]
    Key,
}

/// What lets a request act: the hub's key, and whether the hub serves controls at all.
#[derive(Debug)]
pub struct Guard {
    key: Key,
    read_only: bool,
}

impl Guard {
    /// A guard on `key`; a `read_only` one refuses every control, whatever key comes with it.
    pub fn new(key: Key, read_only: bool) -> Self {
        Self { key, read_only }
    }

    /// A guard on a key made now, or why none could be made.
    pub fn start(read_only: bool) -> io::Result<Self> {
        Key::generate().map(|key| Self::new(key, read_only))
    }

    /// Whether the hub serves controls: what `GET /api/state` tells the page in `controls`.
    pub fn controls(&self) -> bool {
        !self.read_only
    }

    /// Lets a request through to its control, or says why not. Every `POST /api/<control>` asks
    /// this before any work, with the key its [`KEY_HEADER`] carried, if it carried one.
    pub fn admit(&self, presented: Option<&str>) -> Result<(), Refusal> {
        if self.read_only {
            return Err(Refusal::ReadOnly);
        }
        if self.key.check(presented) {
            Ok(())
        } else {
            Err(Refusal::Key)
        }
    }

    /// `page` with the key in its fragment, the link that opens the page able to act. None for a
    /// read-only hub, which has nothing to open.
    pub fn link(&self, page: &str) -> Option<String> {
        self.controls().then(|| {
            format!(
                "{}#{LINK_PARAMETER}={}",
                page.trim_end_matches('#'),
                self.key.reveal()
            )
        })
    }
}

#[cfg(test)]
mod tests;
