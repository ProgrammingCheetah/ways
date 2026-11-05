//! The ledger: what every run cost, one row per run, in an SQLite file on the machine, so
//! the numbers outlive the transcripts they were read from.
//!
//! This is the only code that touches SQLite. Ingesting reads the transcripts with no lock
//! held, skips any whose size and modified time match its row, and writes the rest in one
//! short transaction, so two processes at once take turns instead of failing. It only ever
//! writes rows, so a deleted transcript leaves its row. No row holds dollars: a price is
//! applied on reading, never stored.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::{Connection, Row, TransactionBehavior, params};

use super::prices::PriceTable;
use super::transcript::parse_run;
use super::{collect_jsonl, transcript_roots};

/// The ledger's file in the zaf home.
pub const LEDGER_FILE: &str = "ledger.db";
/// The schema this binary reads and writes, kept in `PRAGMA user_version`.
pub const SCHEMA_VERSION: i64 = 1;

/// How long a process waits for another one's transaction.
const BUSY_WAIT: Duration = Duration::from_secs(60);

const CREATE_RUNS: &str = "CREATE TABLE IF NOT EXISTS runs (
    run_id TEXT PRIMARY KEY,
    parent_run TEXT,
    project TEXT NOT NULL,
    role TEXT,
    epic INTEGER,
    tasks TEXT NOT NULL,
    model TEXT NOT NULL,
    first_time TEXT,
    last_time TEXT,
    turns INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    cache_write_tokens INTEGER NOT NULL,
    cache_read_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    context_first INTEGER NOT NULL,
    context_last INTEGER NOT NULL,
    first_prompt TEXT NOT NULL,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified_ns INTEGER NOT NULL
)";

const COLUMNS: &str = "run_id, parent_run, project, role, epic, tasks, model, first_time, \
     last_time, turns, input_tokens, cache_write_tokens, cache_read_tokens, output_tokens, \
     context_first, context_last, first_prompt, path, size, modified_ns";

/// Gives every subagent its parent's epic, whichever of the two was read first and whether
/// or not the parent's transcript is still there. A parent without an epic takes nothing away.
const LINK_SUBAGENTS: &str = "UPDATE runs SET epic = (
        SELECT parent.epic FROM runs AS parent WHERE parent.run_id = runs.parent_run
    )
    WHERE EXISTS (
        SELECT 1 FROM runs AS parent
        WHERE parent.run_id = runs.parent_run
          AND parent.epic IS NOT NULL
          AND parent.epic IS NOT runs.epic
    )";

/// Why the ledger could not be used; every message names the file.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// The file could not be opened or is not a ledger.
    #[error("cannot open the ledger {}: {reason}", path.display())]
    Open {
        /// The file.
        path: PathBuf,
        /// What SQLite or the file system said.
        reason: String,
    },
    /// The file was written by a newer binary.
    #[error(
        "the ledger {} has schema version {found}; this binary reads {SCHEMA_VERSION}",
        path.display()
    )]
    Schema {
        /// The file.
        path: PathBuf,
        /// The version it holds.
        found: i64,
    },
    /// A read or a write failed.
    #[error("the ledger {}: {source}", path.display())]
    Sql {
        /// The file.
        path: PathBuf,
        /// What SQLite said.
        source: rusqlite::Error,
    },
}

/// One folder of transcripts and the project they count for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Searched recursively for `*.jsonl`.
    pub root: PathBuf,
    /// The project; none names each transcript by Claude's folder for it under `root`.
    pub project: Option<String>,
}

/// One run in the ledger.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LedgerRow {
    /// The session id, `<session>/<agent>` for a subagent.
    pub run_id: String,
    /// The session a subagent ran under.
    pub parent_run: Option<String>,
    /// The project.
    pub project: String,
    /// The run's role.
    pub role: Option<String>,
    /// The run's epic.
    pub epic: Option<u64>,
    /// The tasks its prompt named.
    pub tasks: Vec<u64>,
    /// The model most of its turns ran on.
    pub model: String,
    /// The transcript's first timestamp.
    pub first_time: Option<String>,
    /// The transcript's last timestamp.
    pub last_time: Option<String>,
    /// Turns.
    pub turns: u64,
    /// Fresh input tokens.
    pub input: u64,
    /// Cache-write tokens.
    pub cache_write: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
    /// Output tokens.
    pub output: u64,
    /// The context at turn one.
    pub context_first: u64,
    /// The context at the last turn.
    pub context_last: u64,
    /// The first 120 characters of its first prompt, as `zaf report` shows them.
    pub first_prompt: String,
    /// The transcript this row was read from.
    pub path: String,
    /// The transcript's size when read.
    pub size: u64,
    /// The transcript's modified time when read, in nanoseconds since the epoch.
    pub modified_ns: i64,
}

/// The ledger's sources: the project's profiles under its name, and the default config dir
/// under Claude's folder names, since that dir holds every project's sessions. A config dir
/// that is one of the profiles, as it is inside a spawned run, counts once, as the profile.
pub fn sources(state_dir: &Path, project: &str, default_config_dir: Option<&Path>) -> Vec<Source> {
    let mut sources: Vec<Source> = transcript_roots(state_dir, None)
        .into_iter()
        .map(|root| Source {
            root,
            project: Some(project.to_owned()),
        })
        .collect();
    if let Some(dir) = default_config_dir {
        let root = dir.join("projects");
        if !sources.iter().any(|source| same_dir(&source.root, &root)) {
            sources.push(Source {
                root,
                project: None,
            });
        }
    }
    sources
}

/// Claude's folder name for a project root: every character that is not an ASCII letter or
/// digit becomes `-`, so `/home/zuri/code/ways` is `-home-zuri-code-ways`.
pub fn encoded_root(root: &Path) -> String {
    root.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Whether a row's project is this project: its name, which the profiles' runs carry, or its
/// encoded root, which the default config dir's runs carry, the Engineer's own sessions here.
pub fn is_this_project(row_project: &str, name: &str, root: &Path) -> bool {
    row_project == name
        || row_project == encoded_root(root)
        || fs::canonicalize(root).is_ok_and(|real| row_project == encoded_root(&real))
}

fn same_dir(one: &Path, other: &Path) -> bool {
    one == other
        || matches!(
            (fs::canonicalize(one), fs::canonicalize(other)),
            (Ok(one), Ok(other)) if one == other
        )
}

/// The open ledger.
#[derive(Debug)]
pub struct Ledger {
    path: PathBuf,
    connection: Connection,
}

impl Ledger {
    /// Opens the ledger at `path`, creating it with the current schema when absent.
    pub fn open(path: &Path) -> Result<Self, LedgerError> {
        let failed = |reason: String| LedgerError::Open {
            path: path.to_owned(),
            reason,
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| failed(error.to_string()))?;
        }
        let connection = Connection::open(path).map_err(|error| failed(error.to_string()))?;
        connection
            .busy_timeout(BUSY_WAIT)
            .map_err(|error| failed(error.to_string()))?;
        let mut ledger = Self {
            path: path.to_owned(),
            connection,
        };
        ledger
            .prepare_schema()
            .map_err(|error| failed(error.to_string()))?;
        match ledger.schema_version() {
            Ok(SCHEMA_VERSION) => Ok(ledger),
            Ok(found) => Err(LedgerError::Schema {
                path: path.to_owned(),
                found,
            }),
            Err(error) => Err(failed(error.to_string())),
        }
    }

    /// Reads every transcript under `sources` whose size or modified time differs from its
    /// row and writes its row; returns how many it wrote.
    pub fn ingest(&mut self, sources: &[Source]) -> Result<usize, LedgerError> {
        let known = self.stamps().map_err(|error| self.sql(error))?;
        let mut changed = Vec::new();
        for source in sources {
            let mut files = Vec::new();
            collect_jsonl(&source.root, &mut files);
            files.sort();
            for file in files {
                let Some((size, modified_ns)) = stamp_of(&file) else {
                    continue;
                };
                let (run_id, parent_run) = run_ids(&file);
                if known.get(&run_id) == Some(&(size, modified_ns)) {
                    continue;
                }
                // No prices: the row keeps tokens and the run's dollars are dropped here.
                let Some(run) = parse_run(&file, &PriceTable::default()) else {
                    continue;
                };
                let project = source
                    .project
                    .clone()
                    .unwrap_or_else(|| claude_folder(&source.root, &file));
                changed.push(LedgerRow {
                    run_id,
                    parent_run,
                    project,
                    role: run.attribution.role,
                    epic: run.attribution.epic,
                    tasks: run.attribution.tasks,
                    model: run.model,
                    first_time: run.first_time,
                    last_time: run.last_time,
                    turns: run.turns,
                    input: run.input,
                    cache_write: run.cache_write,
                    cache_read: run.cache_read,
                    output: run.output,
                    context_first: run.context.first().copied().unwrap_or(0),
                    context_last: run.context.last().copied().unwrap_or(0),
                    first_prompt: run.first_prompt,
                    path: file.to_string_lossy().into_owned(),
                    size,
                    modified_ns,
                });
            }
        }
        if changed.is_empty() {
            return Ok(0);
        }
        self.write(&changed).map_err(|error| self.sql(error))?;
        Ok(changed.len())
    }

    /// Every row, by run id.
    pub fn rows(&self) -> Result<Vec<LedgerRow>, LedgerError> {
        self.read_rows().map_err(|error| self.sql(error))
    }

    fn sql(&self, source: rusqlite::Error) -> LedgerError {
        LedgerError::Sql {
            path: self.path.clone(),
            source,
        }
    }

    fn schema_version(&self) -> rusqlite::Result<i64> {
        self.connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
    }

    /// Creates the table in a file at version zero; another process may be doing the same.
    fn prepare_schema(&mut self) -> rusqlite::Result<()> {
        if self.schema_version()? != 0 {
            return Ok(());
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let found: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if found == 0 {
            transaction.execute_batch(CREATE_RUNS)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        transaction.commit()
    }

    /// Each row's size and modified time.
    fn stamps(&self) -> rusqlite::Result<HashMap<String, (u64, i64)>> {
        let mut statement = self
            .connection
            .prepare("SELECT run_id, size, modified_ns FROM runs")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get(0)?, (count_of(row.get(1)?), row.get(2)?)))
        })?;
        rows.collect()
    }

    fn write(&mut self, rows: &[LedgerRow]) -> rusqlite::Result<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut upsert = transaction.prepare(&format!(
                "INSERT OR REPLACE INTO runs ({COLUMNS}) VALUES \
                 (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, \
                 ?18, ?19, ?20)"
            ))?;
            for row in rows {
                upsert.execute(params![
                    row.run_id,
                    row.parent_run,
                    row.project,
                    row.role,
                    row.epic.map(sql_count),
                    serde_json::json!(row.tasks).to_string(),
                    row.model,
                    row.first_time,
                    row.last_time,
                    sql_count(row.turns),
                    sql_count(row.input),
                    sql_count(row.cache_write),
                    sql_count(row.cache_read),
                    sql_count(row.output),
                    sql_count(row.context_first),
                    sql_count(row.context_last),
                    row.first_prompt,
                    row.path,
                    sql_count(row.size),
                    row.modified_ns,
                ])?;
            }
            transaction.execute(LINK_SUBAGENTS, [])?;
        }
        transaction.commit()
    }

    fn read_rows(&self) -> rusqlite::Result<Vec<LedgerRow>> {
        let mut statement = self
            .connection
            .prepare(&format!("SELECT {COLUMNS} FROM runs ORDER BY run_id"))?;
        let rows = statement.query_map([], row_of)?;
        rows.collect()
    }
}

fn row_of(row: &Row<'_>) -> rusqlite::Result<LedgerRow> {
    let tasks: String = row.get(5)?;
    let epic: Option<i64> = row.get(4)?;
    Ok(LedgerRow {
        run_id: row.get(0)?,
        parent_run: row.get(1)?,
        project: row.get(2)?,
        role: row.get(3)?,
        epic: epic.map(count_of),
        tasks: serde_json::from_str(&tasks).unwrap_or_default(),
        model: row.get(6)?,
        first_time: row.get(7)?,
        last_time: row.get(8)?,
        turns: count_of(row.get(9)?),
        input: count_of(row.get(10)?),
        cache_write: count_of(row.get(11)?),
        cache_read: count_of(row.get(12)?),
        output: count_of(row.get(13)?),
        context_first: count_of(row.get(14)?),
        context_last: count_of(row.get(15)?),
        first_prompt: row.get(16)?,
        path: row.get(17)?,
        size: count_of(row.get(18)?),
        modified_ns: row.get(19)?,
    })
}

/// SQLite's integers are signed; no count here comes near the limit.
fn sql_count(count: u64) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

fn count_of(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// A transcript's size and modified time, if it can be read.
fn stamp_of(file: &Path) -> Option<(u64, i64)> {
    let metadata = fs::metadata(file).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((metadata.len(), i64::try_from(modified.as_nanos()).ok()?))
}

/// The run id and parent run of a transcript path: `<session>.jsonl` is a session,
/// `<session>/subagents/<agent>.jsonl` a subagent of it.
fn run_ids(file: &Path) -> (String, Option<String>) {
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parts: Vec<_> = file.iter().map(|part| part.to_string_lossy()).collect();
    let parent = parts
        .iter()
        .rposition(|part| part == "subagents")
        .and_then(|index| index.checked_sub(1))
        .map(|index| parts[index].clone().into_owned());
    match parent {
        Some(parent) => (format!("{parent}/{stem}"), Some(parent)),
        None => (stem, None),
    }
}

/// The folder Claude keeps a transcript's project in, directly under `root`.
fn claude_folder(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .ok()
        .and_then(Path::parent)
        .and_then(|folder| folder.iter().next())
        .map(|folder| folder.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
