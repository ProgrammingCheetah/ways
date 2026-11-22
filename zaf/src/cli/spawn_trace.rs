//! `zaf spawn trace [<ticket>] [--since <when>]`: where the tickets moved, tick by tick, from the
//! trace every real tick leaves in `.zaf/ticks.jsonl` (docs/zaf-actions.md §5.1, R24).
//!
//! It prints one line per move, oldest first, `17:15 #7 lane(implement) -> running(implement)`,
//! with a date line where the listing crosses a day; a ticket keeps that ticket's moves, `--since`
//! the ticks from a time on (an RFC 3339 time, or a span back from now such as `2h`), and `--more`
//! adds what each tick did under its moves ([`crate::app::schedule::trace::listing`]). `--json`
//! prints `{"moves": [...]}`. It only reads, takes no lock and needs no scheduler running. No
//! trace yet, or no move kept, is said and exits `0`; a trace that cannot be read exits `2`.

use std::time::SystemTime;

use super::Cli;
use super::exit::{self, BACKEND, OK};
use crate::adapters::clock;
use crate::app::schedule::ticks::{self, TICKS_FILE};
use crate::app::schedule::trace::listing::{self, Filter};
use crate::composition::{Composition, STATE_DIR};
use crate::domain::TicketId;

/// Prints the trace's moves the filters keep.
pub fn run(cli: &Cli, ticket: Option<TicketId>, since: Option<SystemTime>) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let recorded = match ticks::read(&composition.state_dir()) {
        Ok(recorded) => recorded.unwrap_or_default(),
        Err(why) => {
            crate::eprintln_zaf!("zaf {path}: {why}");
            return BACKEND;
        }
    };
    let filter = Filter { ticket, since };
    if cli.json {
        println!("{}", listing::to_json(&recorded, &filter, cli.more));
        return OK;
    }
    if recorded.is_empty() {
        println!("no trace yet: no tick has written {STATE_DIR}/{TICKS_FILE}");
        return OK;
    }
    let now = SystemTime::now();
    let lines = listing::lines(&recorded, &filter, cli.more, clock::utc_offset(now));
    if lines.is_empty() {
        let mut line = "no moves".to_owned();
        if let Some(ticket) = ticket {
            line.push_str(&format!(" of {ticket}"));
        }
        if let Some(since) = since {
            line.push_str(&format!(
                " since {}",
                humantime::format_rfc3339_seconds(since)
            ));
        }
        println!("{line}");
    }
    for line in lines {
        println!("{line}");
    }
    OK
}
