//! The objects of the framework and the text formats they travel in.
//!
//! Nothing here knows a backend: the board, the VCS and the design source are
//! reached only through [`crate::ports`].

mod answer;
mod ask;
mod epic;
mod finding;
mod ids;
mod instance;
mod labels;
mod memory;
mod profile;
mod quote;
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
pub use epic::{
    BodyHash, CHALLENGER_ROLE, Epic, EpicStage, EpicTask, GroupName, Signature, SignatureError,
};
pub use finding::{Finding, FindingSource, Location, Severity};
pub use ids::{IdError, PageId, TicketId};
pub use instance::{Instance, InstanceNumber, RoleName};
pub use labels::{PageStatus, TicketState};
pub use memory::{MEBIBYTES_PER_UNIT, Mebibytes, Units};
pub use profile::Profile;
pub use quote::Quote;
pub use slot::{SLOTS_DIR, SlotRecord};
pub use task::{
    SecurityTrigger, TaskField, TaskModel, TaskProblem, TaskSize, TaskTemplate, TaskTemplateError,
};
pub use ticket::{
    DEFAULT_TASK_TYPE, NewTask, Standing, Ticket, TicketBody, TicketSummary, TicketTitle,
    TicketType, TicketUrl,
};
