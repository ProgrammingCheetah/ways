//! `zaf ask`: up to five design questions to the keeper, with cited answers the tool verifies.
//!
//! A question asked recently with the same task, kind and text is answered from
//! the ask log without a run. The rest go to the keeper as one ask, climbing the
//! manifest's ladder: a rung whose run fails, whose answer does not parse, does
//! not answer exactly the questions asked, cites a page that is not citable, or
//! carries a quote its pages do not hold, is discarded and the next rung asked.
//! When every rung fails, the ask fails and each question it carried is logged as
//! `failed`; when the answers that were checked failed only on cited pages the design
//! source could not read, the ask returns, each question it carried is logged as
//! `check failed` and the caller reports it (a fault of the design source, never a
//! `not settled`). Every question asked, cached or not, appends one line to
//! `.zaf/asklog.jsonl`: a keeper run that ends in an error leaves `failed` lines, and
//! one that is cut off (the caller's timeout kills `zaf ask` while the keeper still
//! runs) leaves the `interrupted` lines written before the run began, which a later
//! line for the same question supersedes.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::brief::{RUNS_DIR, ensure_runs_dir, named_pages};
use crate::domain::{
    Answer, AnsweredQuestion, Ask, AskError, Instance, IssueNumber, PageId, Profile, Question,
    QuestionId, QuoteCheckError, RoleName, TaskTemplate, Verdict,
};
use crate::manifest::Rung;
use crate::ports::{Board, Design, Page, PortError, RunRequest, Runner};

/// The default keeper prompt; `.zaf/roles/keeper.md` replaces it.
pub const DEFAULT_KEEPER_PROMPT: &str = include_str!("../../../assets/keeper.md");

/// The keeper prompt override, relative to the state folder.
pub const KEEPER_OVERRIDE: &str = "roles/keeper.md";

/// The ask log, relative to the state folder.
pub const ASK_LOG: &str = "asklog.jsonl";

/// How long an answer in the log stays fresh when the manifest says nothing.
pub const DEFAULT_CACHE_HOURS: u64 = 24;

/// The verdict a log line carries when no answer was recorded.
pub const FAILED: &str = "failed";

/// The verdict a log line carries when the ladder ended on a cited page the design source
/// could not read: the question is neither answered nor `not settled`.
pub const CHECK_FAILED: &str = "check failed";

/// What the printed line says about a misconfigured design source.
const MISCONFIGURED: &str = "design dir or index misconfigured";

/// The reason on a line written before a keeper run; it stays only when `zaf ask` never finished.
pub const INTERRUPTED: &str = "interrupted: the keeper was asked and no answer was recorded";

/// The role every rung runs.
pub const KEEPER_ROLE: &str = "keeper";

/// One rung of the ladder: its name and its runner, or why there is none.
pub struct LadderRung<'a> {
    /// The rung as the manifest names it.
    pub name: Rung,
    /// The runner, or why it could not be built; a missing runner fails only its rung.
    pub runner: Result<&'a dyn Runner, String>,
}

/// What is asked, by whom, and where the state lives.
#[derive(Debug, Clone, Copy)]
pub struct AskQuery<'a> {
    /// The task whose named pages go first.
    pub task: IssueNumber,
    /// Who asks, for the log.
    pub from: &'a str,
    /// Each question as `<kind>: <question>`.
    pub questions: &'a [String],
    /// The most questions one ask may carry, from the manifest.
    pub max: usize,
    /// The project root, where the keeper searches.
    pub root: &'a Path,
    /// The state folder, `.zaf`.
    pub state_dir: &'a Path,
    /// How long a logged answer is served without a run.
    pub cache_window: Duration,
    /// The time of the ask.
    pub now: SystemTime,
}

/// What the use case reads and runs through.
pub struct AskPorts<'a> {
    /// The board holding the task.
    pub board: &'a dyn Board,
    /// The design source quotes are checked against; `None` when the project has none.
    pub design: Option<&'a dyn Design>,
    /// The ladder, in order.
    pub ladder: Vec<LadderRung<'a>>,
}

/// How an answer was checked, as the log records it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckResult {
    /// The rung's answer parsed and every quote was found.
    Passed,
    /// Served from an earlier line of the log.
    Cached,
    /// Every rung failed; the reasons, one per rung.
    Failed(Vec<String>),
}

impl fmt::Display for CheckResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passed => formatter.write_str("passed"),
            Self::Cached => formatter.write_str("cached"),
            Self::Failed(reasons) => write!(formatter, "failed ({})", reasons.join("; ")),
        }
    }
}

/// One line of the ask log: one question and what came back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LogLine {
    /// When it was asked, in seconds since the Unix epoch.
    pub time: u64,
    /// The task it was asked for.
    pub task: u64,
    /// Who asked.
    pub from: String,
    /// The question's kind.
    pub kind: String,
    /// The question.
    pub question: String,
    /// `settled`, `not settled`, `conflict`, `failed` or `check failed`.
    pub verdict: String,
    /// The pages the answer cites.
    pub pages: Vec<String>,
    /// The answer entry, in the answer format.
    pub answer: String,
    /// The rung that answered; `None` when every rung failed.
    pub rung: Option<String>,
    /// How the answer was checked.
    pub check: CheckResult,
}

/// The verified answer and the log lines written for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskOutcome {
    /// One entry per question, in the order asked. A question whose line is
    /// [`CHECK_FAILED`] has a `not settled` placeholder here that is never to be shown:
    /// [`AskOutcome::render`] prints its line instead.
    pub answer: Answer,
    /// One line per question, in the order asked, as appended to the log.
    pub lines: Vec<LogLine>,
}

impl AskOutcome {
    /// Whether any question ended on a failed citation check, not on an answer.
    #[must_use]
    pub fn has_check_failure(&self) -> bool {
        self.lines.iter().any(|line| line.verdict == CHECK_FAILED)
    }

    /// The terse answer: each entry in the answer format, and for a question whose
    /// citation check failed, its `check failed` line in the entry's place.
    #[must_use]
    pub fn render(&self) -> String {
        self.answer
            .questions
            .iter()
            .zip(&self.lines)
            .map(|(entry, line)| {
                if line.verdict == CHECK_FAILED {
                    format!("{}\n", line.answer)
                } else {
                    Answer {
                        questions: vec![entry.clone()],
                    }
                    .to_string()
                }
            })
            .collect()
    }
}

/// Why no answer came back.
#[derive(Debug, thiserror::Error)]
pub enum AskFailure {
    /// More questions than the manifest allows.
    #[error("{given} questions; this project allows at most {max}")]
    TooMany {
        /// How many were given.
        given: usize,
        /// The manifest's limit.
        max: usize,
    },
    /// A question not written as `<kind>: <question>`.
    #[error("`{0}` is not `<kind>: <question>`")]
    Question(String),
    /// The questions do not make a valid ask.
    #[error(transparent)]
    Ask(#[from] AskError),
    /// Every rung failed; the reasons, one per rung.
    #[error("no rung gave a verified answer: {}", .0.join("; "))]
    Exhausted(Vec<String>),
    /// The board could not give the task.
    #[error("board: {0}")]
    Board(PortError),
    /// The design source failed while reading a named page.
    #[error("design: {0}")]
    Design(PortError),
    /// The log or a run folder could not be read or written.
    #[error("cannot use {path}: {source}")]
    State {
        /// The file or folder.
        path: PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
}

/// Asks the questions and returns the verified answer; the log has the new lines on return.
pub fn ask(query: &AskQuery<'_>, ports: &AskPorts<'_>) -> Result<AskOutcome, AskFailure> {
    let mut parsed = parse_ask(query)?;
    let questions = parsed.questions.clone();
    let log_path = query.state_dir.join(ASK_LOG);
    let earlier = read_log(&log_path)?;
    let now = seconds(query.now);

    let mut entries: Vec<Option<(AnsweredQuestion, Option<String>, CheckResult)>> = parsed
        .questions
        .iter()
        .map(|question| {
            cached(&earlier, query, question, now)
                .map(|(entry, rung)| (entry, rung, CheckResult::Cached))
        })
        .collect();

    let open: Vec<Question> = parsed
        .questions
        .iter()
        .zip(&entries)
        .filter(|(_, entry)| entry.is_none())
        .map(|(question, _)| question.clone())
        .collect();
    let mut unreadable: Vec<(QuestionId, PageId)> = Vec::new();
    if !open.is_empty() {
        let ticket = ports.board.task(query.task).map_err(AskFailure::Board)?;
        let pages = match (ticket.body.parse::<TaskTemplate>(), ports.design) {
            (Ok(template), Some(design)) => {
                named_pages(design, &template.design_pages).map_err(AskFailure::Design)?
            }
            _ => Vec::new(),
        };
        parsed.context = vec![ticket.title];
        parsed.questions = open;
        let prompt = keeper_prompt(query, &parsed, &pages)?;
        // A keeper run can outlive `zaf ask` (a caller's timeout kills the process, the
        // run goes on): the questions are in the log before it starts.
        append_log(
            &log_path,
            &failed_lines(query, &parsed.questions, INTERRUPTED, now),
        )?;
        match climb(query, ports, &parsed, &prompt) {
            Ok(Climb::Answered {
                answers,
                rung,
                check,
            }) => {
                let mut fresh = answers.into_iter();
                for slot in entries.iter_mut().filter(|slot| slot.is_none()) {
                    *slot = fresh
                        .next()
                        .map(|entry| (entry, rung.clone(), check.clone()));
                }
            }
            // Every open question stays without an entry: each ends on a failed check below.
            Ok(Climb::CheckFailed(pages)) => unreadable = pages,
            Err(error) => {
                let lines = failed_lines(query, &parsed.questions, &error.to_string(), now);
                // The ask's own error is the one to return; a log that cannot be written is not.
                let _ = append_log(&log_path, &lines);
                return Err(error);
            }
        }
    }

    let mut answer = Answer {
        questions: Vec::new(),
    };
    let mut lines = Vec::new();
    for (question, slot) in questions.iter().zip(entries) {
        let (entry, line) = match slot {
            Some((entry, rung, check)) => {
                let line = log_line(query, question, &entry, rung, check, now);
                (entry, line)
            }
            None => {
                let pages = unreadable_for(&unreadable, question.id);
                let line = check_failed_line(query, question, &pages, now);
                (not_settled(question.id, query.task), line)
            }
        };
        lines.push(line);
        answer.questions.push(entry);
    }
    append_log(&log_path, &lines)?;
    Ok(AskOutcome { answer, lines })
}

/// The ask as given, with an empty context; the ticket's title fills it before a run.
fn parse_ask(query: &AskQuery<'_>) -> Result<Ask, AskFailure> {
    let given = query.questions.len();
    if given > query.max {
        return Err(AskFailure::TooMany {
            given,
            max: query.max,
        });
    }
    let mut text = format!(
        "from: {}\nfor: {}\ncontext:\nquestions:\n",
        query.from, query.task
    );
    for (index, question) in query.questions.iter().enumerate() {
        let (kind, body) = question
            .split_once(':')
            .ok_or_else(|| AskFailure::Question(question.clone()))?;
        let line = format!("  q{}: {} | {}\n", index + 1, kind.trim(), body.trim());
        text.push_str(&line);
    }
    Ok(text.parse::<Ask>()?)
}

/// The freshest passed answer to the same task, kind and text inside the window, re-keyed to `question`.
fn cached(
    earlier: &[LogLine],
    query: &AskQuery<'_>,
    question: &Question,
    now: u64,
) -> Option<(AnsweredQuestion, Option<String>)> {
    let kind: &'static str = question.kind.into();
    let window = query.cache_window.as_secs();
    earlier
        .iter()
        .rev()
        .filter(|line| {
            line.check == CheckResult::Passed
                && line.task == query.task.get()
                && line.kind == kind
                && line.question == question.text
                && now.saturating_sub(line.time) <= window
        })
        .find_map(|line| {
            let mut entry = line.answer.parse::<Answer>().ok()?.questions.pop()?;
            entry.id = question.id;
            Some((entry, line.rung.clone()))
        })
}

/// The keeper prompt: the role text, then the ask, then the task's named pages in full.
fn keeper_prompt(query: &AskQuery<'_>, ask: &Ask, pages: &[Page]) -> Result<String, AskFailure> {
    let override_path = query.state_dir.join(KEEPER_OVERRIDE);
    let role = match fs::read_to_string(&override_path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => DEFAULT_KEEPER_PROMPT.to_owned(),
        Err(source) => return Err(state_error(&override_path, source)),
    };
    let mut sections = vec![
        role.trim_end().to_owned(),
        format!("## The ask\n\n{}", ask.to_string().trim_end()),
        "## The task's pages".to_owned(),
    ];
    sections.extend(pages.iter().map(|page| {
        let heading = format!("### {} {} ({})", page.id, page.title, page.status);
        format!("{heading}\n\n{}", page.text.trim_end())
    }));
    let root = format!(
        "## Everything else\n\nSearch the design source under the project root, {}.",
        query.root.display()
    );
    sections.push(root);
    Ok(sections.join("\n\n") + "\n")
}

/// How the ladder ended.
enum Climb {
    /// A rung's answer passed.
    Answered {
        answers: Vec<AnsweredQuestion>,
        rung: Option<String>,
        check: CheckResult,
    },
    /// No answer passed, and the only rungs that got as far as a check failed it on cited
    /// pages the design source could not read: the question and the page, per citation.
    CheckFailed(Vec<(QuestionId, PageId)>),
}

/// Why one rung gave no verified answer.
enum RungFailure {
    /// There was no answer to check: no runner, a failed run, a malformed answer.
    NoAnswer(String),
    /// The answer cites pages the design source could not read.
    Unreadable {
        reason: String,
        pages: Vec<(QuestionId, PageId)>,
    },
    /// The answer was checked and rejected on its own account.
    Rejected(String),
}

impl RungFailure {
    fn reason(&self) -> &str {
        match self {
            Self::NoAnswer(reason) | Self::Rejected(reason) | Self::Unreadable { reason, .. } => {
                reason
            }
        }
    }
}

/// Runs the ladder until a rung's answer passes.
///
/// Every rung failing is [`AskFailure::Exhausted`], unless the answers that were checked
/// failed only on unreadable pages: that is [`Climb::CheckFailed`], a fault of the design
/// source, not of the keeper.
fn climb(
    query: &AskQuery<'_>,
    ports: &AskPorts<'_>,
    ask: &Ask,
    prompt: &str,
) -> Result<Climb, AskFailure> {
    let mut failures = Vec::new();
    let mut rejected = false;
    let mut unreadable: Vec<(QuestionId, PageId)> = Vec::new();
    for (index, rung) in ports.ladder.iter().enumerate() {
        match try_rung(query, ports.design, ask, prompt, index, rung)? {
            Ok(answer) => {
                return Ok(Climb::Answered {
                    answers: answer.questions,
                    rung: Some(rung.name.to_string()),
                    check: CheckResult::Passed,
                });
            }
            Err(failure) => {
                failures.push(format!("{}: {}", rung.name, failure.reason()));
                match failure {
                    RungFailure::NoAnswer(_) => {}
                    RungFailure::Rejected(_) => rejected = true,
                    RungFailure::Unreadable { pages, .. } => unreadable.extend(pages),
                }
            }
        }
    }
    if rejected || unreadable.is_empty() {
        Err(AskFailure::Exhausted(failures))
    } else {
        Ok(Climb::CheckFailed(unreadable))
    }
}

/// One rung: `Ok(Ok(answer))` when it passes, `Ok(Err(failure))` when the rung fails.
fn try_rung(
    query: &AskQuery<'_>,
    design: Option<&dyn Design>,
    ask: &Ask,
    prompt: &str,
    index: usize,
    rung: &LadderRung<'_>,
) -> Result<Result<Answer, RungFailure>, AskFailure> {
    let no_answer = |reason: String| Ok(Err(RungFailure::NoAnswer(reason)));
    let runner = match &rung.runner {
        Ok(runner) => *runner,
        Err(why) => return no_answer(format!("no runner: {why}")),
    };
    let dir = run_dir(query, index, &rung.name)?;
    let request = RunRequest {
        role: RoleName::from(KEEPER_ROLE),
        profile: Profile::Lean,
        prompt: prompt.to_owned(),
        dir,
        env: std::collections::BTreeMap::new(),
    };
    let output = match runner.run(&request) {
        Ok(output) => output,
        Err(error) => return no_answer(format!("run failed: {error}")),
    };
    if output.exit_code != 0 {
        return no_answer(format!("run exited {}", output.exit_code));
    }
    let answer = match output.transcript.trim().parse::<Answer>() {
        Ok(answer) => answer,
        Err(error) => return no_answer(format!("malformed answer: {error}")),
    };
    Ok(verify(&answer, ask, design).map(|()| answer))
}

/// The answer covers exactly the questions asked, in order, cites only citable pages, and every quote is found.
///
/// A cited page the design source cannot read is [`RungFailure::Unreadable`]: its index
/// entry (the design source failed, not found) or its text. A page the index does not know
/// is a wrong citation, not a fault.
fn verify(answer: &Answer, ask: &Ask, design: Option<&dyn Design>) -> Result<(), RungFailure> {
    let rejected = |reason: String| Err(RungFailure::Rejected(reason));
    let asked: Vec<QuestionId> = ask.questions.iter().map(|question| question.id).collect();
    let answered: Vec<QuestionId> = answer.questions.iter().map(|entry| entry.id).collect();
    if asked != answered {
        let list = |ids: &[QuestionId]| {
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        };
        return rejected(format!("answered {} for {}", list(&answered), list(&asked)));
    }
    let cited: BTreeSet<(QuestionId, PageId)> = answer
        .questions
        .iter()
        .flat_map(|entry| {
            let pages = match &entry.verdict {
                Verdict::Settled { pages, .. } | Verdict::Conflict { pages, .. } => {
                    pages.as_slice()
                }
                Verdict::NotSettled { .. } => &[],
            };
            pages
                .iter()
                .map(|citation| (entry.id, citation.page.clone()))
        })
        .collect();
    let mut unreadable = Vec::new();
    let mut reasons = Vec::new();
    for (id, page) in cited {
        let Some(design) = design else {
            return rejected(format!("page {page} cited, but there is no design source"));
        };
        match design.status(&page) {
            Ok(status) if !status.is_citable() => {
                return rejected(format!("page {page} is {status} and cannot be cited"));
            }
            Ok(_) => {}
            Err(error @ PortError::NotFound(_)) => {
                return rejected(format!("page {page}: {error}"));
            }
            Err(error) => {
                reasons.push(format!("page {page}: {error}"));
                unreadable.push((id, page));
            }
        }
    }
    if !unreadable.is_empty() {
        let reason = reasons.join("; ");
        return Err(RungFailure::Unreadable {
            reason,
            pages: unreadable,
        });
    }
    answer
        .check_quotes(|page| {
            design
                .and_then(|design| design.page(page).ok())
                .map(|page| page.text)
        })
        .map_err(|error| {
            let reason = error.to_string();
            match error {
                QuoteCheckError::PageUnavailable { question, page } => RungFailure::Unreadable {
                    reason,
                    pages: vec![(question, page)],
                },
                QuoteCheckError::QuoteNotFound { .. } => RungFailure::Rejected(reason),
            }
        })
}

fn run_dir(query: &AskQuery<'_>, index: usize, rung: &Rung) -> Result<PathBuf, AskFailure> {
    let runs = query.state_dir.join(RUNS_DIR);
    ensure_runs_dir(query.state_dir).map_err(|source| state_error(&runs, source))?;
    let stamp = query
        .now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let name: String = rung
        .as_ref()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    let dir = runs
        .join(Instance::slug_of(query.from))
        .join(format!("ask-{stamp}-{index}-{name}"));
    fs::create_dir_all(&dir).map_err(|source| state_error(&dir, source))?;
    Ok(dir)
}

fn not_settled(id: QuestionId, task: IssueNumber) -> AnsweredQuestion {
    AnsweredQuestion {
        id,
        verdict: Verdict::NotSettled {
            next: format!("question ticket on {task} (door: no)"),
        },
    }
}

/// The pages to name for a question that ended on a failed check: the unreadable pages it
/// cited, or, when its own citations were fine and its rung was discarded with the others,
/// every unreadable page of the ask.
fn unreadable_for(unreadable: &[(QuestionId, PageId)], id: QuestionId) -> Vec<PageId> {
    let own: BTreeSet<PageId> = unreadable
        .iter()
        .filter(|(question, _)| *question == id)
        .map(|(_, page)| page.clone())
        .collect();
    if own.is_empty() {
        let all: BTreeSet<PageId> = unreadable.iter().map(|(_, page)| page.clone()).collect();
        all.into_iter().collect()
    } else {
        own.into_iter().collect()
    }
}

/// The log line of a question that ended on pages the design source could not read.
///
/// Its `answer` is the line the verb prints, its check the failure per page.
fn check_failed_line(
    query: &AskQuery<'_>,
    question: &Question,
    pages: &[PageId],
    now: u64,
) -> LogLine {
    let kind: &'static str = question.kind.into();
    let failures: Vec<String> = pages
        .iter()
        .map(|page| format!("page {page} could not be read"))
        .collect();
    let printed = if pages.is_empty() {
        format!(
            "{}: check failed — no verified answer ({MISCONFIGURED})",
            question.id
        )
    } else {
        format!(
            "{}: check failed — {} ({MISCONFIGURED})",
            question.id,
            failures.join(", ")
        )
    };
    LogLine {
        time: now,
        task: query.task.get(),
        from: query.from.to_owned(),
        kind: kind.to_owned(),
        question: question.text.clone(),
        verdict: CHECK_FAILED.to_owned(),
        pages: Vec::new(),
        answer: printed,
        rung: None,
        check: CheckResult::Failed(failures),
    }
}

fn log_line(
    query: &AskQuery<'_>,
    question: &Question,
    entry: &AnsweredQuestion,
    rung: Option<String>,
    check: CheckResult,
    now: u64,
) -> LogLine {
    let (verdict, pages) = match &entry.verdict {
        Verdict::Settled { pages, .. } => ("settled", pages.as_slice()),
        Verdict::NotSettled { .. } => ("not settled", &[][..]),
        Verdict::Conflict { pages, .. } => ("conflict", pages.as_slice()),
    };
    let kind: &'static str = question.kind.into();
    let single = Answer {
        questions: vec![entry.clone()],
    };
    LogLine {
        time: now,
        task: query.task.get(),
        from: query.from.to_owned(),
        kind: kind.to_owned(),
        question: question.text.clone(),
        verdict: verdict.to_owned(),
        pages: pages
            .iter()
            .map(|citation| citation.page.to_string())
            .collect(),
        answer: single.to_string(),
        rung,
        check,
    }
}

/// One `failed` line per question, carrying `reason` as its answer and its only check reason.
fn failed_lines(
    query: &AskQuery<'_>,
    questions: &[Question],
    reason: &str,
    now: u64,
) -> Vec<LogLine> {
    questions
        .iter()
        .map(|question| {
            let kind: &'static str = question.kind.into();
            LogLine {
                time: now,
                task: query.task.get(),
                from: query.from.to_owned(),
                kind: kind.to_owned(),
                question: question.text.clone(),
                verdict: FAILED.to_owned(),
                pages: Vec::new(),
                answer: reason.to_owned(),
                rung: None,
                check: CheckResult::Failed(vec![reason.to_owned()]),
            }
        })
        .collect()
}

/// Whether `lines[index]` is an interrupted marker that a later line for the same question replaces.
fn superseded(lines: &[LogLine], index: usize) -> bool {
    let line = &lines[index];
    let same = |other: &LogLine| {
        (
            other.time,
            &other.from,
            other.task,
            &other.kind,
            &other.question,
        ) == (line.time, &line.from, line.task, &line.kind, &line.question)
    };
    line.verdict == FAILED && line.answer == INTERRUPTED && lines[index + 1..].iter().any(same)
}

/// Every line of the log; a missing log is empty, and a line that does not parse is skipped.
/// An interrupted marker is dropped once a later line answers or fails the same question.
pub fn read_log(path: &Path) -> Result<Vec<LogLine>, AskFailure> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(state_error(path, source)),
    };
    let lines: Vec<LogLine> = text
        .lines()
        .filter_map(|line| serde_json::from_str::<LogLine>(line).ok())
        .collect();
    let kept = (0..lines.len())
        .filter(|&index| !superseded(&lines, index))
        .map(|index| lines[index].clone())
        .collect();
    Ok(kept)
}

fn append_log(path: &Path, lines: &[LogLine]) -> Result<(), AskFailure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| state_error(parent, source))?;
    }
    let mut text = String::new();
    for line in lines {
        let json = serde_json::to_string(line).map_err(|error| state_error(path, error.into()))?;
        text.push_str(&json);
        text.push('\n');
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| state_error(path, source))?;
    file.write_all(text.as_bytes())
        .map_err(|source| state_error(path, source))
}

fn state_error(path: &Path, source: io::Error) -> AskFailure {
    AskFailure::State {
        path: path.to_owned(),
        source,
    }
}

fn seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests;
