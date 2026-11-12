//! `zaf update`: replace this zaf with the newest release.
//!
//! Nothing is remembered between runs: the newest release is read from the
//! releases each time ([`newest`]), and the running version is the binary's own.
//! An update downloads this platform's archive and its `.sha256` beside the
//! binary, checks the sum, unpacks it, runs the new binary's `--version` to see it
//! is the release it claims to be, and only then renames it over the running one,
//! so a failure at any step leaves the old binary as it was.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;

use crate::ports::{PortError, Releases};

/// The repository zaf is released from.
pub const HOME_REPO: &str = "ProgrammingCheetah/ways";

/// The prefix of every zaf release tag: `zaf-v0.2.1`.
pub const TAG_PREFIX: &str = "zaf-v";

/// The release archive's target for this build, or none where zaf publishes no archive.
pub fn target() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("x86_64-unknown-linux-musl")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else {
        None
    }
}

/// The newest stable release among `tags`: the highest `zaf-v<semver>` with no pre-release
/// part, with its tag. Other tags are passed by.
pub fn newest(tags: &[String]) -> Option<(Version, String)> {
    tags.iter()
        .filter_map(|tag| {
            let version = Version::parse(tag.strip_prefix(TAG_PREFIX)?).ok()?;
            version.pre.is_empty().then(|| (version, tag.clone()))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
}

/// The release archive for `tag` and `target`, as the release workflow names it.
pub fn archive(tag: &str, target: &str) -> String {
    format!("{tag}-{target}.tar.gz")
}

/// What `zaf update` found or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// This zaf is the newest release, or newer.
    Current {
        /// The running version.
        version: Version,
    },
    /// A newer release is out; nothing was changed (a check).
    Behind {
        /// The running version.
        current: Version,
        /// The newest release.
        newest: Version,
    },
    /// The binary was replaced.
    Updated {
        /// The version that ran.
        from: Version,
        /// The version now installed.
        to: Version,
        /// The binary replaced.
        path: PathBuf,
    },
}

/// Why an update could not be made.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// The releases could not be read or fetched.
    #[error(transparent)]
    Port(#[from] PortError),
    /// No stable `zaf-v*` release is published.
    #[error("no zaf release is published at {HOME_REPO}")]
    NoRelease,
    /// zaf publishes no archive for this platform.
    #[error(
        "no release archive for {platform}; build {tag} instead: cargo install --git https://github.com/{HOME_REPO} --tag {tag} --locked --force zaf"
    )]
    NoArchive {
        /// The OS and architecture.
        platform: String,
        /// The newest release's tag.
        tag: String,
    },
    /// A step of the install failed; the old binary is untouched.
    #[error("cannot install {tag}: {reason}")]
    Install {
        /// The release.
        tag: String,
        /// What failed.
        reason: String,
    },
}

/// Says whether a release newer than `current` is out, changing nothing.
pub fn check(releases: &dyn Releases, current: &Version) -> Result<Outcome, UpdateError> {
    let (newest, _) = newest(&releases.tags()?).ok_or(UpdateError::NoRelease)?;
    Ok(if &newest > current {
        Outcome::Behind {
            current: current.clone(),
            newest,
        }
    } else {
        Outcome::Current {
            version: current.clone(),
        }
    })
}

/// Replaces the binary at `exe` (version `current`) with the newest release for `target`,
/// when one is newer. `target` is none where zaf publishes no archive.
pub fn update(
    releases: &dyn Releases,
    current: &Version,
    exe: &Path,
    target: Option<&str>,
) -> Result<Outcome, UpdateError> {
    let (newest, tag) = newest(&releases.tags()?).ok_or(UpdateError::NoRelease)?;
    if &newest <= current {
        return Ok(Outcome::Current {
            version: current.clone(),
        });
    }
    let Some(target) = target else {
        return Err(UpdateError::NoArchive {
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            tag,
        });
    };
    let install = |reason: String| UpdateError::Install {
        tag: tag.clone(),
        reason,
    };
    let parent = exe
        .parent()
        .ok_or_else(|| install(format!("{} has no folder", exe.display())))?;
    // Beside the binary, so the final rename stays on one file system.
    let work = WorkDir::create(parent)
        .map_err(|error| install(format!("cannot write in {}: {error}", parent.display())))?;
    let name = archive(&tag, target);
    let sum = format!("{name}.sha256");
    releases.download(&tag, &[name.clone(), sum.clone()], work.path())?;
    verify_sum(&work.path().join(&name), &work.path().join(&sum)).map_err(install)?;
    let fresh = unpack(&work.path().join(&name), work.path(), &tag, target).map_err(install)?;
    probe(&fresh, &newest).map_err(install)?;
    std::fs::rename(&fresh, exe)
        .map_err(|error| install(format!("cannot replace {}: {error}", exe.display())))?;
    Ok(Outcome::Updated {
        from: current.clone(),
        to: newest,
        path: exe.to_owned(),
    })
}

/// A folder for one update's downloads, removed with everything in it when dropped.
struct WorkDir(PathBuf);

impl WorkDir {
    fn create(parent: &Path) -> std::io::Result<Self> {
        let dir = parent.join(format!(".zaf-update-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir(&dir)?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Checks the archive against the first word of its `.sha256` file.
fn verify_sum(archive: &Path, sum_file: &Path) -> Result<(), String> {
    let expected = std::fs::read_to_string(sum_file)
        .map_err(|error| format!("cannot read {}: {error}", sum_file.display()))?;
    let expected = expected
        .split_whitespace()
        .next()
        .ok_or_else(|| format!("{} is empty", sum_file.display()))?
        .to_lowercase();
    let actual = sha256(archive)?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "the archive's sha256 is {actual}, but the release says {expected}"
        ))
    }
}

/// The file's sha256 in hex, from `sha256sum`, else `shasum -a 256`.
fn sha256(path: &Path) -> Result<String, String> {
    let tries: [(&str, &[&str]); 2] = [("sha256sum", &[]), ("shasum", &["-a", "256"])];
    for (program, args) in tries {
        let Ok(output) = Command::new(program).args(args).arg(path).output() else {
            continue;
        };
        if output.status.success()
            && let Some(sum) = String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
        {
            return Ok(sum.to_lowercase());
        }
    }
    Err("neither sha256sum nor shasum could hash the archive".to_owned())
}

/// Unpacks the archive into `dir` and returns its `zaf`, made executable.
fn unpack(archive: &Path, dir: &Path, tag: &str, target: &str) -> Result<PathBuf, String> {
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(dir)
        .status()
        .map_err(|error| format!("cannot run tar: {error}"))?;
    if !status.success() {
        return Err(format!("tar could not unpack {}", archive.display()));
    }
    let fresh = dir.join(format!("{tag}-{target}")).join("zaf");
    if !fresh.is_file() {
        return Err(format!("the archive holds no {tag}-{target}/zaf"));
    }
    std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot make {} executable: {error}", fresh.display()))?;
    Ok(fresh)
}

/// Runs the new binary's `--version` and checks it names `version`.
fn probe(fresh: &Path, version: &Version) -> Result<(), String> {
    let output = Command::new(fresh)
        .arg("--version")
        .output()
        .map_err(|error| format!("the new binary does not run: {error}"))?;
    let said = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() && said == format!("zaf {version}") {
        Ok(())
    } else {
        Err(format!("the new binary says `{said}`, not `zaf {version}`"))
    }
}

#[cfg(test)]
mod tests;
