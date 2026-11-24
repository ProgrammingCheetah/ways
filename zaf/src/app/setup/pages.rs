//! The pack's standard design pages: `design/` in the installed pack, copied by `setup` into the
//! project's design folder as the standard actions are copied into the machine's.
//!
//! Only a project whose design source is a folder in the repository (`repo-dir`) gets them; any
//! other source is said so in one line and left alone. A page the project lacks is copied and its
//! line, with status `current`, added to the design index. A page the project holds equal to the
//! pack's is left; one that differs is offered to the asker with its difference and kept unless it
//! says yes. A page the pack does not carry is never touched, and nothing is deleted. Refreshing
//! the pages is not setup's job after the first install beyond this asking.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use super::{ReplaceAction, difference, replace_with, why};
use crate::app::doctor::Commands;
use crate::domain::PageId;
use crate::manifest::{DesignKind, DesignSection};

/// The folder of the installed pack that holds the standard design pages.
pub const PAGES_DIR: &str = "design";

/// The status the index line of an installed page carries.
const STATUS: &str = "current";

/// What the pages install did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackPages {
    /// The pages copied into the design folder, by file name, sorted.
    pub installed: Vec<String>,
    /// The pages that differed and were replaced on a yes, by file name.
    pub replaced: Vec<String>,
    /// The pages that differ from the pack's and were kept with no one asked: there was no asker or
    /// the difference could not be shown. Sorted.
    pub differs: Vec<String>,
    /// The ids whose index line was added, sorted.
    pub indexed: Vec<String>,
    /// How many standard pages were left out for want of a `repo-dir` design source; 0 when the
    /// source is one or the pack carries none.
    pub skipped: usize,
}

impl PackPages {
    /// The lines `setup` prints; none when nothing happened.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.skipped > 0 {
            lines.push(
                "installed no standard design page: the design source is not a folder in the repository (kind repo-dir)"
                    .to_owned(),
            );
        }
        if !self.installed.is_empty() {
            lines.push(format!(
                "installed {} standard design pages: {}",
                self.installed.len(),
                self.installed.join(", ")
            ));
        }
        for name in &self.replaced {
            lines.push(format!("replaced the design page {name}"));
        }
        if !self.indexed.is_empty() {
            lines.push(format!(
                "added to the design index with status {STATUS}: {}",
                self.indexed.join(", ")
            ));
        }
        if !self.differs.is_empty() {
            lines.push(format!(
                "design page differs from the pack's, kept: {}; run zaf setup in a terminal to review",
                self.differs.join(", ")
            ));
        }
        lines
    }
}

/// One standard page of the pack: its id, file name, path and the index title.
struct Standard {
    id: PageId,
    name: String,
    path: PathBuf,
    title: String,
}

/// The standard pages of the pack at `pages`: the regular `<id>-<slug>.md` files, sorted by name,
/// each with the first heading of its text as its title. None when the folder cannot be read.
fn standard_pages(pages: &Path) -> Vec<Standard> {
    let Ok(entries) = fs::read_dir(pages) else {
        return Vec::new();
    };
    let mut found: Vec<Standard> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let path = entry.path();
            if !name.ends_with(".md") || !fs::symlink_metadata(&path).ok()?.is_file() {
                return None;
            }
            let id = PageId::from_str(name.split_once('-')?.0).ok()?;
            let text = fs::read_to_string(&path).ok()?;
            let title = text
                .lines()
                .find_map(|line| line.strip_prefix("# "))?
                .trim()
                .to_owned();
            (!title.is_empty()).then_some(Standard {
                id,
                name,
                path,
                title,
            })
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// The design folder and index of a project whose source is a `repo-dir`; none for any other.
fn source(root: &Path, design: &DesignSection) -> Option<(PathBuf, PathBuf)> {
    if design.kind != DesignKind::RepoDir {
        return None;
    }
    Some((design.dir(root)?, design.index_path(root)?))
}

/// Whether the index text holds an entry for `id`: `- <id> <status> — <title>`.
fn indexed(text: &str, id: &str) -> bool {
    text.lines().any(|line| {
        line.trim()
            .strip_prefix("- ")
            .and_then(|entry| entry.split_whitespace().next())
            == Some(id)
    })
}

/// The standard pages of the pack at `pages` that a run would write: the project's design folder
/// lacks the file, or the index lacks its line. Reads the disk and writes nothing; none unless the
/// design source is a `repo-dir`.
pub fn new_pages(root: &Path, design: &DesignSection, pages: &Path) -> Vec<String> {
    let Some((dir, index)) = source(root, design) else {
        return Vec::new();
    };
    let index_text = fs::read_to_string(index).unwrap_or_default();
    standard_pages(pages)
        .into_iter()
        .filter(|page| {
            fs::symlink_metadata(dir.join(&page.name)).is_err()
                || !indexed(&index_text, page.id.as_ref())
        })
        .map(|page| page.name)
        .collect()
}

/// Copies the standard pages of the pack at `pages` into the project's design folder and adds each
/// one's index line with status `current` (see the module). A page the design folder holds
/// differing from the pack's is offered to `asker` and replaced only on yes. Installs nothing, and
/// says so in [`PackPages::skipped`], when the design source is not a `repo-dir`.
pub fn install_pack_pages(
    root: &Path,
    design: &DesignSection,
    pages: &Path,
    commands: &dyn Commands,
    mut asker: Option<&mut dyn ReplaceAction>,
) -> io::Result<PackPages> {
    let mut done = PackPages::default();
    let standard = standard_pages(pages);
    let Some((dir, index)) = source(root, design) else {
        done.skipped = standard.len();
        return Ok(done);
    };
    for page in standard {
        let installed = dir.join(&page.name);
        match fs::symlink_metadata(&installed) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir_all(&dir)?;
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&installed)?;
                file.write_all(&fs::read(&page.path)?)?;
                file.sync_all()?;
                done.installed.push(page.name.clone());
            }
            Err(error) => return Err(error),
            // A link or a folder is never offered.
            Ok(meta) if !meta.is_file() => {}
            Ok(_) => {
                let held = fs::read(&installed)?;
                let offered = fs::read(&page.path)?;
                if held != offered {
                    offer(
                        &mut done,
                        &mut asker,
                        commands,
                        &page,
                        (&installed, &held, &offered),
                    )?;
                }
            }
        }
        if add_to_index(&index, &page)? {
            done.indexed.push(page.id.to_string());
        }
    }
    Ok(done)
}

/// Offers `asker` the replacement of the differing page, recording what came of it.
fn offer(
    done: &mut PackPages,
    asker: &mut Option<&mut dyn ReplaceAction>,
    commands: &dyn Commands,
    page: &Standard,
    (installed, held, offered): (&Path, &[u8], &[u8]),
) -> io::Result<()> {
    let shown = asker
        .as_deref_mut()
        .and_then(|asker| Some((asker, difference(commands, installed, &page.path)?)));
    let Some((asker, shown)) = shown else {
        done.differs.push(page.name.clone());
        return Ok(());
    };
    let reason = match (std::str::from_utf8(held), std::str::from_utf8(offered)) {
        (Ok(held), Ok(offered)) => why(held, offered),
        _ => None,
    };
    let name = format!("the design page {}", page.name);
    if asker.replace(&name, reason.as_ref(), &shown) {
        let folder = installed.parent().unwrap_or(Path::new("."));
        replace_with(&page.path, folder, &page.name)?;
        done.replaced.push(page.name.clone());
    }
    Ok(())
}

/// Appends the page's line to the index, making the index (with a heading) when it is not there;
/// whether it did. A page the index already lists, whatever its status, is left as listed.
fn add_to_index(index: &Path, page: &Standard) -> io::Result<bool> {
    let text = match fs::read_to_string(index) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    if indexed(&text, page.id.as_ref()) {
        return Ok(false);
    }
    let mut next = if text.is_empty() {
        "# Design pages\n\n".to_owned()
    } else if text.ends_with('\n') {
        text
    } else {
        text + "\n"
    };
    next.push_str(&format!("- {} {STATUS} — {}\n", page.id, page.title));
    if let Some(folder) = index.parent() {
        fs::create_dir_all(folder)?;
    }
    fs::write(index, next)?;
    Ok(true)
}
