//! The objects of the framework and the text formats they travel in.
//!
//! Nothing here knows a backend: the board, the VCS and the design source are
//! reached only through [`crate::ports`]. [`schedule`] is the scheduling core: where
//! each ticket sits, worked out from facts alone.

mod answer;
mod ask;
mod budget;
mod document_signature;
mod epic;
mod finding;
mod ids;
mod instance;
mod labels;
mod memory;
mod profile;
mod quote;
mod ruling;
pub mod schedule;
mod slot;
mod task;
mod text;
mod ticket;

pub use answer::{
    ANSWER_WORD_LIMIT, Answer, AnswerError, AnsweredQuestion, Citation, QuoteCheckError, Verdict,
};
pub use ask::{
    Ask, AskError, AskKind, Asker, MAX_CONTEXT_LINES, MAX_QUESTIONS, QUESTION_WORD_LIMIT, Question,
    QuestionId,
};
pub use budget::{Budget, DEFAULT_RESERVE, Fit, SizeTable};
pub use document_signature::{DocumentSignature, SignatureState};
pub use epic::{
    BodyHash, CHALLENGER_ROLE, Epic, EpicStage, EpicTask, GroupName, PLANNING_ROLE, RELEASE_PREFIX,
    Signature, SignatureError, of_version, release_of, with_release,
};
pub use finding::{Finding, FindingSource, Location, Severity};
pub use ids::{CommitId, IdError, PageId, TicketId};
pub use instance::{Instance, InstanceNumber, RoleName};
pub use labels::{PageStatus, TicketState};
pub use memory::{MEBIBYTES_PER_UNIT, Mebibytes, Units};
pub use profile::Profile;
pub use quote::Quote;
pub use ruling::{RulingGap, TypedRulingError, check_typed_ruling, first_gap};
pub use schedule::{ActionName, CheckName, PhaseEnd};
pub use slot::{SLOTS_DIR, SlotRecord};
pub use task::{
    SecurityTrigger, TaskField, TaskModel, TaskProblem, TaskSize, TaskTemplate, TaskTemplateError,
};
pub use ticket::{
    DEFAULT_TASK_TYPE, NewEpic, NewTask, OpenTicket, Standing, StandingAt, Ticket, TicketBody,
    TicketSummary, TicketTitle, TicketType, TicketUrl,
};
