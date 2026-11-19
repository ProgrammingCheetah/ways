//! A version built, and its release pull request (design 0.5.2, M17 and M27): the two reads the
//! scheduler's loop asks of its host after a tick ([`crate::app::schedule::scheduler::Host`]).
//!
//! Built is worked out on every read from the board's epics and stored nowhere: the epics whose
//! `Release:` line is the version are all done, and there is at least one. The pull request is
//! opened by the pack action `release-open`, run through the one action runner
//! ([`build::open_release`]) with `ZAF_VERSION` and `ZAF_BASE` set.

use std::path::Path;

use crate::app::build;
use crate::app::schedule::scheduler::Opened;
use crate::composition::Composition;
use crate::domain::{Epic, EpicStage, of_version};

/// Whether every epic of `version` is done, and at least one epic is of it.
pub fn is_built(epics: &[Epic], version: &str) -> bool {
    of_version(epics, version).next().is_some()
        && of_version(epics, version).all(|epic| epic.stage() == EpicStage::Done)
}

/// Whether `version` is built on the board of `composition` as it stands now; a board that cannot
/// be read says it is not.
pub fn version_built(composition: &Composition, version: &str) -> bool {
    composition
        .board()
        .ok()
        .and_then(|board| board.epics().ok())
        .is_some_and(|epics| is_built(&epics, version))
}

/// Runs `release-open` for `version`, with the zaf home `home` when given, and hands back the pull
/// request: a refusal or a failure is its sentence.
pub fn open_release(
    composition: &Composition,
    version: &str,
    home: Option<&Path>,
) -> Result<Opened, String> {
    let pull_request = match home {
        Some(home) => {
            let home = std::path::absolute(home).map_err(|error| error.to_string())?;
            build::open_release_with_home(composition, version, &home)
        }
        None => build::open_release(composition, version),
    }
    .map_err(|error| error.to_string())?;
    Ok(Opened {
        version: version.to_owned(),
        number: pull_request.number,
        link: pull_request.link,
    })
}

#[cfg(test)]
mod tests;
