//! The scheduler's edge around the pure core ([`crate::domain::schedule`]): what reads the
//! ports into the core's facts, the one place that reads and writes phase run folders, the tick's
//! phase steps over the core's placement, and the trace of where everything was.
//!
//! [`record`] owns the run folder under `.zaf/phases/`: its files, who writes each and in
//! what order, and how a folder reads back as a [`crate::domain::schedule::RunRecord`].
//! [`gather`] reads the board, the forge and the run folders into one
//! [`crate::domain::schedule::TicketFacts`] per open ticket with a phase list, for
//! [`crate::domain::schedule::place`] to place. [`tick`] holds the one gate every phase start
//! passes ([`tick::may_start`], for the tick and `zaf spawn phase` alike) and runs the tick's
//! phase steps over the placement. [`shadow`] places the tickets and writes the placement as
//! JSON, deciding nothing. [`labels`] sets each open ticket's state label from its place, the
//! labels being output only (R5). [`trace`] makes each real tick's line of `.zaf/ticks.jsonl`
//! (where every ticket sits, what the tick did, what moved) and [`ticks`] owns that file (R24).
//!
//! [`ask`] holds every question the scheduler raises, each with a marker it reads back, so no
//! cause has two open questions and a ruling resets what it should.
//!
//! The process that stays up (`zaf spawn --watch`) is [`scheduler`]'s loop: it holds
//! [`lock`]'s `.zaf/scheduler.lock` for its life, ticks when an event asks through [`request`]'s
//! `.zaf/tick.request` or its timer does, and starts again as a new binary ([`binary`]) between
//! two ticks.
//!
//! The core decides where each ticket sits; nothing here keeps a second opinion. `zaf spawn
//! --once --dry-run --json` shows the placement beside the plan, and every real tick writes it to
//! the trace, which nothing reads back to decide. The scheduler keeps the last placement between
//! ticks only as a cache, which every tick replaces with what it gathers.

pub mod ask;
pub mod binary;
pub mod gather;
pub mod labels;
pub mod lock;
pub mod record;
pub mod request;
pub mod scheduler;
pub mod shadow;
pub mod tick;
pub mod ticks;
pub mod trace;
