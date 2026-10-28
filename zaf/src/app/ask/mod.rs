//! `zaf ask`: up to five design questions to the keeper, with cited answers the tool verifies.
//!
//! A question asked recently with the same task, kind and text is answered from
//! the ask log without a run. The rest go to the keeper as one ask, climbing the
//! manifest's ladder: a rung whose run fails, whose answer does not parse, does
//! not answer exactly the questions asked, cites a page that is not citable, or
//! carries a quote its pages do not hold, is discarded and the next rung asked.
//! When every rung fails, each question is `not settled`. Every question asked,
//! cached or not, appends one line to `.zaf/asklog.jsonl`.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::brief::{RUNS_DIR, ensure_runs_dir, named_pages};
use crate::domain::{
    Answer, AnsweredQuestion, Ask, AskError, IssueNumber, Profile, Question, QuestionId, RoleName,
    TaskTemplate, Verdict,
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
    /// `settled`, `not settled` or `conflict`.
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
    /// One entry per question, in the order asked.
    pub answer: Answer,
    /// One line per question, in the order asked, as appended to the log.
    pub lines: Vec<LogLine>,
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
        let (answered, rung, check) = climb(query, ports, &parsed, &prompt)?;
        let mut fresh = answered.into_iter();
        for slot in entries.iter_mut().filter(|slot| slot.is_none()) {
            *slot = fresh
                .next()
                .map(|entry| (entry, rung.clone(), check.clone()));
        }
    }

    let mut answer = Answer {
        questions: Vec::new(),
    };
    let mut lines = Vec::new();
    for (question, slot) in questions.iter().zip(entries) {
        let (entry, rung, check) = slot.unwrap_or_else(|| {
            let entry = not_settled(question.id, query.task);
            (entry, None, CheckResult::Failed(Vec::new()))
        });
        lines.push(log_line(query, question, &entry, rung, check, now));
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

/// Runs the ladder until a rung's answer passes; every rung failing yields `not settled` for each question.
fn climb(
    query: &AskQuery<'_>,
    ports: &AskPorts<'_>,
    ask: &Ask,
    prompt: &str,
) -> Result<(Vec<AnsweredQuestion>, Option<String>, CheckResult), AskFailure> {
    let mut failures = Vec::new();
    for (index, rung) in ports.ladder.iter().enumerate() {
        match try_rung(query, ports.design, ask, prompt, index, rung)? {
            Ok(answer) => {
                return Ok((
                    answer.questions,
                    Some(rung.name.to_string()),
                    CheckResult::Passed,
                ));
            }
            Err(reason) => failures.push(format!("{}: {reason}", rung.name)),
        }
    }
    let exhausted = ask
        .questions
        .iter()
        .map(|question| not_settled(question.id, query.task))
        .collect();
    Ok((exhausted, None, CheckResult::Failed(failures)))
}

/// One rung: `Ok(Ok(answer))` when it passes, `Ok(Err(reason))` when the rung fails.
fn try_rung(
    query: &AskQuery<'_>,
    design: Option<&dyn Design>,
    ask: &Ask,
    prompt: &str,
    index: usize,
    rung: &LadderRung<'_>,
) -> Result<Result<Answer, String>, AskFailure> {
    let runner = match &rung.runner {
        Ok(runner) => *runner,
        Err(why) => return Ok(Err(format!("no runner: {why}"))),
    };
    let dir = run_dir(query, index, &rung.name)?;
    let request = RunRequest {
        role: RoleName::from(KEEPER_ROLE),
        profile: Profile::Lean,
        prompt: prompt.to_owned(),
        dir,
    };
    let output = match runner.run(&request) {
        Ok(output) => output,
        Err(error) => return Ok(Err(format!("run failed: {error}"))),
    };
    if output.exit_code != 0 {
        return Ok(Err(format!("run exited {}", output.exit_code)));
    }
    let answer = match output.transcript.trim().parse::<Answer>() {
        Ok(answer) => answer,
        Err(error) => return Ok(Err(format!("malformed answer: {error}"))),
    };
    Ok(verify(&answer, ask, design).map(|()| answer))
}

/// The answer covers exactly the questions asked, in order, cites only citable pages, and every quote is found.
fn verify(answer: &Answer, ask: &Ask, design: Option<&dyn Design>) -> Result<(), String> {
    let asked: Vec<QuestionId> = ask.questions.iter().map(|question| question.id).collect();
    let answered: Vec<QuestionId> = answer.questions.iter().map(|entry| entry.id).collect();
    if asked != answered {
        let list = |ids: &[QuestionId]| {
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        };
        return Err(format!("answered {} for {}", list(&answered), list(&asked)));
    }
    let cited: BTreeSet<_> = answer
        .questions
        .iter()
        .flat_map(|entry| match &entry.verdict {
            Verdict::Settled { pages, .. } | Verdict::Conflict { pages, .. } => pages.as_slice(),
            Verdict::NotSettled { .. } => &[],
        })
        .map(|citation| citation.page.clone())
        .collect();
    for page in &cited {
        let status = design
            .ok_or_else(|| format!("page {page} cited, but there is no design source"))?
            .status(page)
            .map_err(|error| format!("page {page}: {error}"))?;
        if !status.is_citable() {
            return Err(format!("page {page} is {status} and cannot be cited"));
        }
    }
    answer
        .check_quotes(|page| {
            design
                .and_then(|design| design.page(page).ok())
                .map(|page| page.text)
        })
        .map_err(|error| error.to_string())
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
    let dir = runs.join(format!("ask-{stamp}-{index}-{name}"));
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

/// Every line of the log; a missing log is empty, and a line that does not parse is skipped.
pub fn read_log(path: &Path) -> Result<Vec<LogLine>, AskFailure> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(state_error(path, source)),
    };
    let lines = text
        .lines()
        .filter_map(|line| serde_json::from_str::<LogLine>(line).ok())
        .collect();
    Ok(lines)
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
