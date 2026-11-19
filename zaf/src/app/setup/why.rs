//! Why a pack action changed: the pack copy's `# changed:` comment (I15).
//!
//! The comment is no `# zaf` key (zaf refuses unknown keys): it is the first line right after
//! the `# zaf` block, so the block still parses as it did.

use crate::app::actions::block_end;
use crate::domain::screen::Text;

const MARKER: &str = "# changed:";

/// The reason the pack's copy of an action changed: the text after the colon, trimmed, of the
/// `# changed:` line right after `pack`'s `# zaf` block, when `installed` holds no identical
/// line. None when `pack` has no block, no such line right after it, or `installed` carries it
/// already. A `# changed:` line anywhere else in the file is not the why.
pub fn why(installed: &str, pack: &str) -> Option<Text> {
    let end = block_end(pack)?;
    let line = pack.lines().nth(end)?;
    let reason = line.strip_prefix(MARKER)?.trim();
    if reason.is_empty() || installed.lines().any(|held| held == line) {
        return None;
    }
    Some(Text::from(reason))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::app::actions::parse;

    const BLOCK: &str = "#!/bin/sh\n# zaf description: land it\n# zaf 1: failed\n";
    const WHY: &str = "# changed: it now waits for the checks\n";

    fn with(comment: &str) -> String {
        format!("{BLOCK}{comment}echo body\n")
    }

    #[test]
    fn why_reads_the_line_after_the_block() {
        assert_eq!(
            why(&with(""), &with(WHY)),
            Some(Text::from("it now waits for the checks"))
        );
    }

    #[test]
    fn why_is_none_when_installed_has_it() {
        assert_eq!(why(&with(WHY), &with(WHY)), None);
    }

    #[test]
    fn why_ignores_a_comment_elsewhere() {
        let pack = format!("{BLOCK}echo body\n{WHY}");
        assert_eq!(why(&with(""), &pack), None);
        let after_blank = format!("{BLOCK}\n{WHY}echo body\n");
        assert_eq!(why(&with(""), &after_blank), None);
        assert_eq!(why("", "echo body\n# changed: no block\n"), None);
    }

    #[test]
    fn a_changed_comment_keeps_the_metadata_parsing() {
        let file = Path::new(".zaf/actions/land");
        let plain = parse(file, &with("")).expect("plain parses");
        let marked = parse(file, &with(WHY)).expect("marked parses");
        assert_eq!(plain.metadata, marked.metadata);
    }
}
