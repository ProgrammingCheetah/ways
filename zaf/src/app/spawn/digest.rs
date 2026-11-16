//! What a person reads of one tick: [`Block`], the short text a real tick prints
//! (docs/zaf-actions.md §5.1), so a terminal left open shows that zaf works and what it works on.
//!
//! In order, each line left out when it has nothing to say: the header `tick 14:32`; the plan's
//! slots; each ticket that moved since the trace's last line (`#7 lane(implement) ->
//! running(implement)`), or with `--more` every ticket's place, then the count line (`26 tickets:
//! 1 running, 3 in the lane, 8 waiting, 14 done`), which [`crate::app::schedule::trace`] words;
//! what the tick did (`lost`, `released`, `#26 unblocked: #25 is done`, `ran`, `started`, `wait`,
//! `question on`, `launching`, `launched`); what
//! runs now, `running  challenger instance 2 on #4, 6 min`; what the tick passed over, worded as
//! the dry run words it; what waits on the Engineer, `waiting on you: 3 questions (#29 #30 #31)`
//! and `waiting on you: epic #1 is not signed`; and, when none of that but the places and the
//! slots has a line, `nothing to do`. `zaf spawn --watch` ends each tick with `next tick 14:37` ([`next_tick_line`]), and says
//! `tick 14:32 skipped: another tick holds spawn.lock` for a tick that found the lock held
//! ([`skipped_line`]).
//!
//! The block is pure and records nothing: time arrives as a `SystemTime` and a UTC offset in
//! seconds, so no test reads the clock. A piece of state that cannot be read becomes the line
//! `could not read <what>: <why>` and never fails the tick.

use std::time::{Duration, SystemTime};

use super::SpawnState;
use crate::app::actions::LivePhase;
use crate::domain::{Epic, EpicStage, Instance, TicketId};

/// The local time of day as `14:32`, `at` shifted by `utc_offset` seconds. A time before the
/// epoch reads as the epoch.
pub fn clock(at: SystemTime, utc_offset: i64) -> String {
    let seconds = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX / 2)
        });
    let day = (seconds + utc_offset).rem_euclid(86_400);
    format!("{:02}:{:02}", day / 3600, day % 3600 / 60)
}

/// How long something has run, in words: `under 1 min`, `6 min`, `1 h 5 min`.
pub fn age(elapsed: Duration) -> String {
    let minutes = elapsed.as_secs() / 60;
    match minutes {
        0 => "under 1 min".to_owned(),
        1..=59 => format!("{minutes} min"),
        _ => format!("{} h {} min", minutes / 60, minutes % 60),
    }
}

/// The line of a tick that found `spawn.lock` held by another: `tick 14:32 skipped: another tick
/// holds spawn.lock`.
pub fn skipped_line(at: SystemTime, utc_offset: i64) -> String {
    format!(
        "tick {} skipped: another tick holds spawn.lock",
        clock(at, utc_offset)
    )
}

/// The last line of a `--watch` tick: `next tick 14:37`, `interval` after `now`.
pub fn next_tick_line(now: SystemTime, interval: Duration, utc_offset: i64) -> String {
    format!("next tick {}", clock(now + interval, utc_offset))
}

/// The live instances, one line each with how long they have run, read as `zaf status` reads
/// them: the distinct instances of the derived state, with the tasks each carries. An instance
/// whose run folder records no start time shows no duration.
pub fn running_lines(state: &SpawnState, now: SystemTime) -> Vec<String> {
    state
        .instances()
        .into_iter()
        .map(|name| {
            let tasks: Vec<String> = name
                .parse::<Instance>()
                .map(|instance| state.tasks_of(&instance))
                .unwrap_or_default()
                .iter()
                .map(|task| format!("#{task}"))
                .collect();
            let since = state
                .launches
                .iter()
                .filter(|launch| launch.instance == name)
                .filter_map(|launch| launch.started_at.as_deref())
                .filter_map(|text| humantime::parse_rfc3339(text).ok())
                .min();
            let mut line = format!("running  {name}");
            if !tasks.is_empty() {
                line.push_str(&format!(" ({})", tasks.join(" ")));
            }
            if let Some(since) = since {
                line.push_str(&format!(
                    ", {}",
                    age(now.duration_since(since).unwrap_or_default())
                ));
            }
            line
        })
        .collect()
}

/// The live task phases, one line each with how long they have run.
pub fn phase_lines(phases: &[LivePhase], now: SystemTime) -> Vec<String> {
    phases
        .iter()
        .map(|phase| {
            let epic = phase
                .epic()
                .map_or_else(String::new, |epic| format!(" (epic {epic})"));
            format!(
                "running  {} {}{epic}, {}",
                phase.action,
                phase.task,
                age(now.duration_since(phase.started_at).unwrap_or_default())
            )
        })
        .collect()
}

/// What waits on the Engineer: `waiting on you: 3 questions (#29 #30 #31)`, then one line for each
/// epic the Engineer has not signed.
pub fn waiting_lines(questions: &[TicketId], unsigned: &[TicketId]) -> Vec<String> {
    let mut lines = Vec::new();
    if !questions.is_empty() {
        let numbers: Vec<String> = questions.iter().map(ToString::to_string).collect();
        let noun = if questions.len() == 1 {
            "question"
        } else {
            "questions"
        };
        lines.push(format!(
            "waiting on you: {} {noun} ({})",
            questions.len(),
            numbers.join(" ")
        ));
    }
    lines.extend(
        unsigned
            .iter()
            .map(|epic| format!("waiting on you: epic {epic} is not signed")),
    );
    lines
}

/// The open epics the Engineer has not signed, or has edited since.
pub fn unsigned_epics(epics: &[Epic]) -> Vec<TicketId> {
    epics
        .iter()
        .filter(|epic| epic.stage() == EpicStage::Unsigned)
        .map(|epic| epic.id)
        .collect()
}

/// One tick's block, filled in as the tick goes: print [`Block::head`] when the tick has decided,
/// the launches run, and print [`Block::tail`] when they have ended.
#[derive(Debug, Clone)]
pub struct Block {
    at: SystemTime,
    utc_offset: i64,
    /// Who asked for the tick, when the scheduler runs it: `asked by: phase-end implement #7,
    /// ruling #45`, the line right under the tick's own.
    pub asked: Option<String>,
    /// What the tick did before its launches ran: `lost`, `released` and `unblocked` lines, then
    /// the phase steps' and the launches'.
    pub did: Vec<String>,
    /// The launches that ended: `launched <instance>: exit <n> (<report>)`.
    pub launched: Vec<String>,
    running: Vec<String>,
    /// The plan's slots line (`slots 1 of 4 (guard green), 1 running, 3 free`), when it has one.
    pub slots: Option<String>,
    /// Where the tickets sit: each move since the trace's last line, or every ticket with
    /// `--more`, then the count line ([`crate::app::schedule::trace::place_lines`]). They are no
    /// act, so they never stand in for `nothing to do`.
    pub places: Vec<String>,
    /// What the tick passed over.
    pub skipped: Vec<String>,
    waiting: Vec<String>,
    unreadable: Vec<String>,
}

impl Block {
    /// An empty block for a tick that began at `at`, `utc_offset` seconds from UTC.
    pub fn new(at: SystemTime, utc_offset: i64) -> Self {
        Self {
            at,
            utc_offset,
            asked: None,
            did: Vec::new(),
            launched: Vec::new(),
            running: Vec::new(),
            slots: None,
            places: Vec::new(),
            skipped: Vec::new(),
            waiting: Vec::new(),
            unreadable: Vec::new(),
        }
    }

    /// The live instances, read at `now`; a read that failed is the line `could not read …`.
    pub fn read_running(&mut self, state: Result<&SpawnState, String>, now: SystemTime) {
        match state {
            Ok(state) => self.running = running_lines(state, now),
            Err(why) => self.cannot_read("running instances", &why),
        }
    }

    /// What waits on the Engineer: the open questions and the unsigned epics, each a read that
    /// may have failed.
    pub fn read_waiting(
        &mut self,
        questions: Result<&[TicketId], String>,
        unsigned: Result<&[TicketId], String>,
    ) {
        let questions = match questions {
            Ok(questions) => questions,
            Err(why) => {
                self.cannot_read("questions", &why);
                &[]
            }
        };
        let unsigned = match unsigned {
            Ok(unsigned) => unsigned,
            Err(why) => {
                self.cannot_read("epics", &why);
                &[]
            }
        };
        self.waiting = waiting_lines(questions, unsigned);
    }

    /// The task phases that run now, one `running` line each after the instances':
    /// `running  implement #3 (epic #1), 4 min`.
    pub fn read_phases(&mut self, phases: Result<&[LivePhase], String>, now: SystemTime) {
        match phases {
            Ok(phases) => self.running.extend(phase_lines(phases, now)),
            Err(why) => self.cannot_read("running phases", &why),
        }
    }

    fn cannot_read(&mut self, what: &str, why: &str) {
        self.unreadable
            .push(format!("could not read {what}: {why}"));
    }

    /// The header, who asked, the slots, where the tickets sit and what the tick did before its
    /// launches ran.
    pub fn head(&self) -> String {
        let mut lines = vec![format!("tick {}", clock(self.at, self.utc_offset))];
        lines.extend(self.asked.iter().cloned());
        lines.extend(self.slots.iter().cloned());
        lines.extend(self.places.iter().cloned());
        lines.extend(self.did.iter().cloned());
        join(lines)
    }

    /// The rest: the launches that ended, what runs now, what was skipped, what waits, and
    /// `nothing to do` when none of those, and nothing the tick did, has a line.
    pub fn tail(&self) -> String {
        let mut lines = Vec::new();
        lines.extend(self.launched.iter().cloned());
        lines.extend(self.running.iter().cloned());
        lines.extend(self.skipped.iter().cloned());
        lines.extend(self.waiting.iter().cloned());
        lines.extend(self.unreadable.iter().cloned());
        let nothing = self.did.is_empty()
            && self.launched.is_empty()
            && self.running.is_empty()
            && self.waiting.is_empty()
            && self.unreadable.is_empty();
        if nothing {
            lines.push("nothing to do".to_owned());
        }
        join(lines)
    }

    /// The whole block: [`Self::head`] then [`Self::tail`].
    pub fn render_text(&self) -> String {
        format!("{}{}", self.head(), self.tail())
    }
}

/// The lines as text, one each; no lines, no text.
fn join(lines: Vec<String>) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

#[cfg(test)]
mod tests;
