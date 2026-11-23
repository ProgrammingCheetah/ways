//! The tool catalog: every visible top-level verb of clap's command but `help`, as a tool.
//!
//! Nothing lists verb names here: a verb is a tool because clap shows it, so a new verb needs
//! no change, and a hidden one (`mcp`, say) is left out because it is hidden.

use serde_json::json;

use clap::FromArgMatches;

use crate::app::mcp::LineChecker;
use crate::cli::Cli;
use crate::domain::{CommandPath, ToolSpec};

/// clap's own name for its generated help verb.
const HELP: &str = "help";

/// The catalog over one clap command; it checks the lines built for its tools.
#[derive(Debug, Clone)]
pub struct Catalog {
    command: clap::Command,
}

impl Catalog {
    /// A checker over `command`, the command whose verbs [`Catalog::from_command`] listed.
    pub fn new(command: clap::Command) -> Self {
        Self { command }
    }

    /// One tool for each visible top-level subcommand of `command` but `help`, in clap's order.
    ///
    /// A description is the verb's long help, then each subcommand's usage line as clap
    /// renders it.
    pub fn from_command(command: &clap::Command) -> Vec<ToolSpec> {
        let mut built = command.clone();
        built.build();
        built
            .get_subcommands()
            .filter(|verb| !verb.is_hide_set() && verb.get_name() != HELP)
            .map(spec_of)
            .collect()
    }
}

impl LineChecker for Catalog {
    fn check(&self, argv: &[String]) -> Result<CommandPath, String> {
        let matches = self
            .command
            .clone()
            .try_get_matches_from(argv)
            .map_err(|error| error.to_string())?;
        Cli::from_arg_matches(&matches)
            .map(|cli| cli.verb.command_path())
            .map_err(|error| error.to_string())
    }
}

fn spec_of(verb: &clap::Command) -> ToolSpec {
    let mut description = verb
        .get_long_about()
        .or_else(|| verb.get_about())
        .map(ToString::to_string)
        .unwrap_or_default();
    for sub in verb
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && sub.get_name() != HELP)
    {
        description.push('\n');
        description.push_str(sub.clone().render_usage().to_string().trim_end());
    }
    ToolSpec {
        name: verb.get_name().to_owned(),
        description,
        input_schema: input_schema(),
    }
}

/// The input every tool takes; none of it is required.
fn input_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "args": {
                "type": "array",
                "items": { "type": "string" },
                "description": "The words after the verb, one element each."
            },
            "stdin": {
                "type": "string",
                "description": "What the verb reads on its standard input."
            },
            "json": {
                "type": "boolean",
                "description": "Run with --json: structured output."
            },
            "more": {
                "type": "boolean",
                "description": "Run with --more: more depth than the terse default."
            }
        },
        "required": []
    })
}

#[cfg(test)]
mod tests;
