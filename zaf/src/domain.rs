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
mod profile;
mod quote;
mod task;
mod text;

pub use answer::{
    ANSWER_WORD_LIMIT, Answer, AnswerError, AnsweredQuestion, Citation, QuoteCheckError, Verdict,
};
pub use ask::{
    Ask, AskError, AskKind, Asker, MAX_CONTEXT_LINES, MAX_QUESTIONS, QUESTION_WORD_LIMIT, Question,
    QuestionId,
};
pub use epic::Epic;
pub use finding::{Finding, FindingSource, Location, Severity};
pub use ids::{IdError, IssueNumber, PageId};
pub use instance::{Instance, InstanceNumber, RoleName};
pub use labels::{PageStatus, TaskLabel};
pub use profile::Profile;
pub use quote::Quote;
pub use task::{SecurityTrigger, TaskField, TaskModel, TaskSize, TaskTemplate, TaskTemplateError};
