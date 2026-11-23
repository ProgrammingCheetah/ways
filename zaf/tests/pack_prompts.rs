//! No prompt zaf or the pack hands a model tells it to type a `zaf` command in Bash (D15, D24):
//! the verbs are MCP tools, and `zaf` is denied in Bash. Read: every markdown page under
//! `zaf/assets/` and `plugins/teams/` (agents, skills, briefs, the interface page, the keeper
//! prompt, the READMEs) and the prompt strings of the actions under `plugins/teams/actions/` and
//! `.zaf/actions/`. A code span, or a code line, that begins with `zaf ` and one of clap's visible
//! verbs fails, naming the file and the line. Settings files, `NODE.inf.json` and the actions'
//! own shell lines are not read. Outside the ways repository, where the pack is not beside the
//! crate, nothing runs, as in `pack_briefs`.

use std::path::{Path, PathBuf};

use clap::CommandFactory;
use zaf::cli::Cli;

/// D15's rule sentence, the one line that writes `zaf <verb> <words>`; it is matched exactly.
const MCP_RULE: &str = "zaf's verbs are MCP tools. Wherever a page says `zaf <verb> <words>`, call the tool `<verb>` (`mcp__zaf__<verb>`) with `args` set to those words. Never type `zaf` in Bash; it is denied.";

/// The visible top-level verbs of clap's command but `help`, as the MCP catalog reads them.
fn visible_verbs() -> Vec<String> {
    let mut command = Cli::command();
    command.build();
    command
        .get_subcommands()
        .filter(|verb| !verb.is_hide_set() && verb.get_name() != "help")
        .map(|verb| verb.get_name().to_owned())
        .collect()
}

/// Whether `text` begins with `zaf <verb>`, the verb a whole word.
fn begins_with_zaf_verb(text: &str, verbs: &[String]) -> bool {
    let text = text.trim_start();
    let text = text.strip_prefix("$ ").unwrap_or(text).trim_start();
    let Some(rest) = text.strip_prefix("zaf ") else {
        return false;
    };
    let word = rest.split_whitespace().next().unwrap_or("");
    verbs.iter().any(|verb| verb == word)
}

/// The text between backtick pairs on one line.
fn code_spans(line: &str) -> impl Iterator<Item = &str> {
    line.split('`').skip(1).step_by(2)
}

/// What one line says in code, if it tells a model to run `zaf <verb>`.
fn offence<'a>(line: &'a str, in_fence: bool, verbs: &[String]) -> Option<&'a str> {
    if line.trim() == MCP_RULE {
        return None;
    }
    if in_fence {
        return begins_with_zaf_verb(line, verbs).then_some(line.trim());
    }
    code_spans(line).find(|span| begins_with_zaf_verb(span, verbs))
}

/// `label:line: text` for each offence in a markdown page.
fn scan_markdown(label: &str, text: &str, verbs: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    let mut in_fence = false;
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if let Some(text) = offence(line, in_fence, verbs) {
            found.push(format!("{label}:{}: {text}", index + 1));
        }
    }
    found
}

/// The physical lines (1-based) of an action that are prompt text: a `prompt="..."` assignment,
/// closed by its first unescaped double quote, and the lines of a `claude -p` command, joined by
/// trailing backslashes.
fn prompt_lines(text: &str) -> Vec<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_start();
        if line.starts_with("prompt=\"") {
            let mut quotes = 0;
            loop {
                found.push(index + 1);
                quotes += unescaped_quotes(lines[index]);
                if quotes >= 2 || index + 1 >= lines.len() {
                    break;
                }
                index += 1;
            }
        } else if line.contains("claude -p") {
            while index < lines.len() {
                found.push(index + 1);
                if !lines[index].trim_end().ends_with('\\') {
                    break;
                }
                index += 1;
            }
        }
        index += 1;
    }
    found
}

fn unescaped_quotes(line: &str) -> usize {
    let mut count = 0;
    let mut escaped = false;
    for c in line.chars() {
        match c {
            '\\' if !escaped => {
                escaped = true;
                continue;
            }
            '"' if !escaped => count += 1,
            _ => {}
        }
        escaped = false;
    }
    count
}

/// `label:line: text` for each offence in an action's prompt strings. A prompt is written in a
/// shell string, so its code spans arrive as escaped backticks; every line of it is read as a
/// code line too.
fn scan_action(label: &str, text: &str, verbs: &[String]) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for number in prompt_lines(text) {
        let raw = lines[number - 1];
        let line = raw
            .replace("\\`", "`")
            .replace("\\\"", "\"")
            .trim_start()
            .trim_start_matches("prompt=\"")
            .trim_start_matches("CLAUDE_CONFIG_DIR=\"$ZAF_ROOT/.zaf/profiles/lean\" ")
            .trim_start_matches("claude -p ")
            .trim_start_matches('"')
            .to_owned();
        let hit = offence(&line, true, verbs).or_else(|| offence(&line, false, verbs));
        if let Some(hit) = hit {
            found.push(format!("{label}:{number}: {hit}"));
        }
    }
    found
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn no_prompt_tells_a_model_to_run_zaf_in_bash() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("..");
    if !root.join("plugins/teams").is_dir() {
        return;
    }
    let verbs = visible_verbs();
    let mut files = Vec::new();
    for folder in ["zaf/assets", "plugins/teams"] {
        walk(&root.join(folder), &mut files);
    }
    // The pack's README is the Engineer's manual: it tells them what to type in their own shell
    // and no model is handed it, so it is the one page not read.
    let manual = root.join("plugins/teams/README.md");
    let pages: Vec<_> = files
        .iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "md") && **path != manual)
        .collect();
    assert!(!pages.is_empty(), "no markdown page was found to read");
    let mut found = Vec::new();
    for path in pages {
        let label = path.strip_prefix(&root).unwrap_or(path).display();
        let text = std::fs::read_to_string(path).expect("the page reads");
        found.extend(scan_markdown(&label.to_string(), &text, &verbs));
    }
    let mut actions = Vec::new();
    for folder in ["plugins/teams/actions", ".zaf/actions"] {
        walk(&root.join(folder), &mut actions);
    }
    for path in actions {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let label = path.strip_prefix(&root).unwrap_or(&path).display();
        found.extend(scan_action(&label.to_string(), &text, &verbs));
    }
    assert!(
        found.is_empty(),
        "a prompt tells a model to type `zaf <verb>` in Bash; name the tool instead:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_matcher_fails_a_zaf_line_and_passes_the_rule_sentence() {
    let verbs = visible_verbs();
    assert!(verbs.iter().any(|verb| verb == "board"));

    let role = "# Programmer\n\nRun `zaf board ready`.\n";
    assert_eq!(
        scan_markdown("plugins/teams/agents/x.md", role, &verbs),
        ["plugins/teams/agents/x.md:3: zaf board ready"]
    );

    let fenced = "text\n```\n$ zaf check 12\n```\n";
    assert_eq!(
        scan_markdown("p.md", fenced, &verbs),
        ["p.md:3: $ zaf check 12"]
    );
    let table = "| `zaf do slice-pr <task>` | publish |\n";
    assert_eq!(scan_markdown("p.md", table, &verbs).len(), 1);

    assert!(scan_markdown("interface.md", MCP_RULE, &verbs).is_empty());
    // `<verb>` is a placeholder, not a verb: it is no violation on its own.
    assert!(scan_markdown("p.md", "Say `zaf <verb> <words>`.", &verbs).is_empty());
    // Not a verb, a bare `zaf`, prose and a tool call are all fine.
    let fine = "`zaf` is denied; `zaf --version`; zaf board is prose; call `board` with `args`.\n";
    assert!(scan_markdown("p.md", fine, &verbs).is_empty());

    let action = "#!/bin/sh\nzaf board claim \"$task\" || exit 2\nprompt=\"Read it, then run \\`zaf epic show 1\\`.\nMore.\"\nclaude -p \\\n    \"Task. Findings: \\`zaf check pr:1:2\\`\" \\\n    --model sonnet\nzaf brief 3\n";
    assert_eq!(
        scan_action("a/implement", action, &verbs),
        [
            "a/implement:3: zaf epic show 1",
            "a/implement:6: zaf check pr:1:2"
        ]
    );
}
