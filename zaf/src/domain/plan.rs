//! The plan as a graph of tasks: its types, and the passes that rewrite it.
//!
//! A plan is a list of nodes (tasks). Each node holds behaviours, and each behaviour says by
//! name what it writes, provides and needs. Nobody writes an edge: the provider of a name comes
//! before every node that needs it. The types here hold any plan the reference optimiser
//! (`.teams/releases/0.6.0/plan-graph/reference/optimize.py`) reads, so every fixture's plan
//! loads into a [`PlanInput`] with `serde`; the JSON keys are the fixtures' own.
//!
//! The module is pure and uses ordered collections only, so the same plan gives the same result
//! whatever order its nodes are listed in: `tests/architecture.rs` fails on a `HashMap` or a
//! `HashSet` under `domain/plan`. Each pass has its own file, a stand-in until its task fills it:
//! [`graph`], [`names`], [`refuse`], [`optimise`] (and, under it, `derive`, `split`, `lift`,
//! `settle` and `thin`) and [`measure`]. [`shape`] checks an epic's graph read from its tickets
//! against O3's structural rules. The text form of a planned ticket is in [`body`];
//! [`text`] reads a plan as a planner writes it into a [`PlanInput`].

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::newtype::text_newtype;

pub mod body;
pub mod graph;
pub mod measure;
pub mod names;
pub mod optimise;
pub mod read;
pub mod record;
pub mod refuse;
pub mod shape;
pub mod text;
pub mod vocabulary;

pub use body::{PlannedTicket, Section, Signature, Wait, WaitOn};

text_newtype! {
    /// The planner's name for a task: letters, digits, `-` and `_`. It is also how a
    /// `Blocked by` line or an epic table names a task or an epic before it is an id.
    NodeKey
}

text_newtype! {
    /// A name a behaviour provides or needs. A wait in an [`Order`] or a [`KeptWait`] may carry
    /// a `turn:<area>` in its place, which is zaf's own and no behaviour's.
    Name
}

impl Name {
    /// The name a filed task has by its number, `#<n>` (D4): what a plan needs to wait on that
    /// task (D13). Such a name never has a signature.
    pub fn of_task(task: crate::domain::TicketId) -> Self {
        Self(task.to_string())
    }

    /// The filed task the name stands for, when it is a `#<n>`.
    pub fn task(&self) -> Option<crate::domain::TicketId> {
        let digits = self.0.strip_prefix('#')?;
        digits
            .parse::<u64>()
            .ok()
            .map(crate::domain::TicketId::from)
    }
}

text_newtype! {
    /// A part of the system in words, never a file: what a behaviour writes.
    Area
}

text_newtype! {
    /// An epic's number, as the plan writes it.
    EpicKey
}

/// The id of a node in the graph: the planner's key, `A+B` for a merge, `A.a` for a part, or
/// `I@<epic>.<k>` for the interface node of the epic's k-th plan.
///
/// Ids order naturally, as the reference's `nat()` does: a run of digits compares as a number,
/// so `A2` comes before `A10`, `I@1004.1` before `I@1004.2` and `#9` before `#10`. Two ids that
/// read the same (`A7` and `A07`) fall back to their text, so the order agrees with `Eq`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    derive_more::Display,
    derive_more::From,
    derive_more::AsRef,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
#[as_ref(str)]
pub struct NodeId(String);

impl From<&str> for NodeId {
    fn from(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl From<&NodeKey> for NodeId {
    fn from(key: &NodeKey) -> Self {
        Self(key.as_str().to_owned())
    }
}

impl NodeId {
    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Ord for NodeId {
    fn cmp(&self, other: &Self) -> Ordering {
        natural(&self.0, &other.0).then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for NodeId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// One piece of a natural-order key: the text between digit runs, or a digit run.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Piece<'a> {
    Text(&'a str),
    /// A digit run without its leading zeros, ordered by length first: a number.
    Number(usize, &'a str),
}

/// The pieces of a text as `re.split(r"(\d+)")` cuts it: text, number, text, ... starting and
/// ending with a text (empty when the text starts or ends with a number), so two keys always
/// compare a text with a text and a number with a number.
fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let digits_at = rest.find(|c: char| c.is_ascii_digit());
        let Some(start) = digits_at else {
            out.push(Piece::Text(rest));
            return out;
        };
        out.push(Piece::Text(&rest[..start]));
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(tail.len());
        let number = tail[..end].trim_start_matches('0');
        out.push(Piece::Number(number.len(), number));
        rest = &tail[end..];
    }
}

fn natural(left: &str, right: &str) -> Ordering {
    pieces(left).cmp(&pieces(right))
}

/// What a behaviour weighs: 1 is small, 2 medium (D10). An interface node weighs 1 a signature.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    derive_more::Display,
    derive_more::From,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
pub struct Weight(pub u32);

/// A behaviour's size as a fixture writes it: an integer reads as a [`Weight`]; anything else
/// (a string, a fraction, a negative number) is kept as written, so the refusal pass can name
/// the node as `bad-size`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Size {
    /// An integer.
    Weight(Weight),
    /// Whatever else the plan wrote.
    Other(serde_json::Value),
}

impl Size {
    /// The weight, when the size is an integer.
    pub fn weight(&self) -> Option<Weight> {
        match self {
            Size::Weight(weight) => Some(*weight),
            Size::Other(_) => None,
        }
    }
}

impl Default for Size {
    /// A behaviour with no size is 1 (the reference's `setdefault`).
    fn default() -> Self {
        Size::Weight(Weight(1))
    }
}

impl serde::Serialize for Size {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Size::Weight(weight) => weight.serialize(serializer),
            Size::Other(value) => value.serialize(serializer),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Size {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let weight = value.as_u64().and_then(|n| u32::try_from(n).ok());
        Ok(match weight {
            Some(n) => Size::Weight(Weight(n)),
            None => Size::Other(value),
        })
    }
}

/// One thing a task builds: what it writes, provides and needs, each by name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Behaviour {
    /// What the behaviour does, in a sentence. It is what the plan means, so it is never empty.
    #[serde(default)]
    pub text: String,
    /// Its weight.
    #[serde(default)]
    pub size: Size,
    /// The areas it writes: two tasks that write one need an order or a merge.
    #[serde(default)]
    pub writes: BTreeSet<Area>,
    /// The names it gives to the tasks that come after.
    #[serde(default)]
    pub provides: BTreeSet<Name>,
    /// The names it waits for.
    #[serde(default)]
    pub needs: BTreeSet<Name>,
}

/// Whether a node does work or only holds signatures.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A task that builds behaviours. It has a limit.
    #[default]
    Body,
    /// A task that writes the signatures its users wait on. It has no limit.
    Interface,
}

/// A ticket an earlier plan of the epic filed (D29).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Filed {
    /// Still open: fixed, never split, merged or lifted.
    Open,
    /// Done: code by now, so what it provides is existing, and it leaves the plan.
    Done,
}

/// A wait a filed ticket kept: it comes after `after`, on the area `area` they both write.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Turn(pub NodeId, pub Area);

/// A task of the plan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Its id, unique in the plan.
    pub id: NodeId,
    /// Body or interface; a body when the plan says nothing.
    #[serde(default)]
    pub kind: NodeKind,
    /// Set on a ticket a re-plan carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filed: Option<Filed>,
    /// The waits a filed ticket already holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turns: Vec<Turn>,
    /// A plan filed with `Blocked by` names what each task waits on; it reads as needs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<NodeKey>,
    /// The epic whose task this is; it names the interface node an epic's lifts land in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epic: Option<EpicKey>,
    /// What it builds. A task of the plan holds at least one.
    #[serde(default)]
    pub behaviours: Vec<Behaviour>,
}

/// The names a release's design gives, which a plan may use (D5).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vocabulary {
    /// Every area a task may write.
    #[serde(default)]
    pub areas: BTreeSet<Area>,
    /// The names that have a signature.
    #[serde(default)]
    pub signature: BTreeSet<Name>,
    /// The names that have none: a prompt, a shell action, a doc.
    #[serde(default)]
    pub no_signature: BTreeSet<Name>,
    /// The names the code already provides.
    #[serde(default)]
    pub existing: BTreeSet<Name>,
    /// The epic each name is given to, for the names whose entry says `epic` (D13).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub epic: BTreeMap<Name, crate::domain::TicketId>,
}

/// The names a plan adds to the vocabulary (D6). A list under any other key is the planner's
/// attempt to add what only the vocabulary gives; it is kept in `other` so the refusal can name
/// it as `planner-list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NewNames {
    /// Added areas.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub areas: BTreeSet<Area>,
    /// Added names with a signature.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub signature: BTreeSet<Name>,
    /// Added names with none.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub no_signature: BTreeSet<Name>,
    /// Any other list, by its key.
    #[serde(flatten)]
    pub other: BTreeMap<String, BTreeSet<Name>>,
}

/// How an old plan was filed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FiledWith {
    /// With `Blocked by` lines (D4).
    BlockedBy,
}

const DEFAULT_LIMIT: Weight = Weight(2);
const DEFAULT_LANES: u32 = 4;
const DEFAULT_RECORD: u32 = 1;

fn default_limit() -> Weight {
    DEFAULT_LIMIT
}

fn default_lanes() -> u32 {
    DEFAULT_LANES
}

fn default_record() -> u32 {
    DEFAULT_RECORD
}

/// Everything the optimiser reads: the nodes and the lists around them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanInput {
    /// The tasks, in the order they were listed; the order carries no meaning. A list, not a
    /// map, so two nodes of one id stay there for the refusal to find.
    pub nodes: Vec<Node>,
    /// The largest weight a body node may reach.
    #[serde(default = "default_limit")]
    pub limit: Weight,
    /// How many tasks run side by side.
    #[serde(default = "default_lanes")]
    pub lanes: u32,
    /// Which plan of its epic this is: the interface node is `I@<epic>.<record>`.
    #[serde(default = "default_record")]
    pub record: u32,
    /// The epic the plan is of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epic: Option<EpicKey>,
    /// Names other epics need, lifted though nothing here needs them (D13).
    #[serde(default)]
    pub shared: BTreeSet<Name>,
    /// Names another epic provides, each with the node of that epic that gives it.
    #[serde(default)]
    pub outside: BTreeMap<Name, NodeId>,
    /// Names the vocabulary gives this epic that a filed ticket of another epic waits on it for
    /// (D13, Q10): a plan that leaves one out is `unprovided`. The caller fills it; a plan does
    /// not write it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub owed: BTreeSet<Name>,
    /// Names with no signature. `None` when the plan did not write the list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_signature: Option<BTreeSet<Name>>,
    /// Names the code already provides. `None` when the plan did not write the list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing: Option<BTreeSet<Name>>,
    /// The names the done filed tickets provided: code by now, and already in `existing`. Set
    /// by `names::normalise`; a plan does not write it.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub done_names: BTreeSet<Name>,
    /// The release's vocabulary, when the design has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocabulary: Option<Vocabulary>,
    /// What the plan adds to the vocabulary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_names: Option<NewNames>,
    /// How an old plan was filed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filed_with: Option<FiledWith>,
    /// Each epic a `Blocked by` line may name, with its tasks.
    #[serde(default)]
    pub epics: BTreeMap<NodeKey, Vec<NodeId>>,
}

impl PlanInput {
    /// A plan of these nodes with the defaults of every other key.
    pub fn of(nodes: Vec<Node>) -> Self {
        Self {
            nodes,
            limit: DEFAULT_LIMIT,
            lanes: DEFAULT_LANES,
            record: DEFAULT_RECORD,
            epic: None,
            shared: BTreeSet::new(),
            outside: BTreeMap::new(),
            owed: BTreeSet::new(),
            no_signature: None,
            existing: None,
            done_names: BTreeSet::new(),
            vocabulary: None,
            new_names: None,
            filed_with: None,
            epics: BTreeMap::new(),
        }
    }
}

/// Whether to lift the order edges that have a signature into interfaces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lift {
    /// Lift every liftable name (production).
    #[default]
    All,
    /// Lift none.
    None,
}

/// Whether each lifted name gets its own interface node or the epic's share one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interfaces {
    /// An interface node a name.
    #[default]
    Each,
    /// One interface node an epic and a record (production).
    One,
}

/// The options a fixture runs with; the reference's defaults are `all` and `each`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// Which names are lifted.
    #[serde(default)]
    pub lift: Lift,
    /// How the interface nodes are cut.
    #[serde(default)]
    pub interfaces: Interfaces,
}

/// A wait inside the plan: `to` comes after `from` because of `needs`.
///
/// The fields sort as the reference's `edge_key` does: natural ids first, then the name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Order {
    /// The node that comes first.
    pub from: NodeId,
    /// The node that waits.
    pub to: NodeId,
    /// The name, or `turn:<area>`, the wait is on.
    pub needs: Name,
}

/// A wait on another epic's task, kept as it is and never optimised (D13).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct KeptWait {
    /// The other epic's node, as `outside` names it.
    pub from: NodeId,
    /// The node of the plan that waits.
    pub to: NodeId,
    /// The name the wait is on.
    pub needs: Name,
}

/// Why a plan is refused. The code is the fixtures' `refused`, in kebab case.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::Display,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum RefusalCode {
    /// Two nodes with one id.
    DuplicateId,
    /// A node with no behaviour, or one whose text is empty.
    NoBehaviour,
    /// An id with `+` that does not hold one behaviour per part.
    BadId,
    /// A size that is not a whole number of at least 1.
    BadSize,
    /// A body behaviour heavier than the limit.
    TooLarge,
    /// A behaviour that needs what it provides.
    NeedsItself,
    /// A name neither the vocabulary's nor the plan's.
    UnknownName,
    /// A plan that writes `no_signature` or `existing` itself, or adds a name the code provides.
    PlannerList,
    /// A name provided twice, or one another epic provides.
    TwoProviders,
    /// A need nobody provides and the code does not hold.
    Unprovided,
    /// A new task waiting on an open filed ticket for a name with a signature.
    FixedProvider,
    /// Waits that go round.
    Cycle,
}

/// A plan the optimiser will not rewrite: the code and the detail that names what is wrong. It
/// never repairs one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {detail}")]
pub struct Refusal {
    /// What kind of break.
    pub code: RefusalCode,
    /// The node, name or nodes it is about.
    pub detail: String,
}

impl Refusal {
    /// A refusal of this kind, naming this.
    pub fn new(code: RefusalCode, detail: impl fmt::Display) -> Self {
        Self {
            code,
            detail: detail.to_string(),
        }
    }
}

/// The plan as the optimiser leaves it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Optimised {
    /// The nodes, by natural id.
    pub nodes: Vec<Node>,
    /// The waits between them, thinned.
    pub orders: BTreeSet<Order>,
    /// The waits on other epics' tasks.
    pub kept: BTreeSet<KeptWait>,
}

/// What a plan measures (I5): the keys the fixtures' `before` and `after` carry.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measures {
    /// How many nodes.
    pub nodes: usize,
    /// How many of them are interfaces.
    pub interfaces: usize,
    /// The number of layers: the longest chain of waits.
    pub chain: usize,
    /// How many nodes sit in each layer.
    pub width: Vec<usize>,
    /// The layers with fewer nodes than there are lanes.
    pub narrow_layers: Vec<usize>,
    /// The weight of every node together.
    pub work: u32,
    /// The weight of the heaviest chain.
    pub critical: u32,
    /// The heaviest chain's nodes, first to last (D15).
    pub critical_path: Vec<NodeId>,
}

#[cfg(test)]
#[test]
fn natural_id_order() {
    let id = NodeId::from;
    assert!(id("A2") < id("A10"));
    assert!(id("I@1004.1") < id("I@1004.2"));
    assert!(id("I@1004.2") > id("I@1004.1"));
    assert!(id("#9") < id("#10"));
    // A number sorts by value, not by text, however long it runs.
    assert!(id("9") < id("10"));
    assert!(id("A9") < id("A10"));
    assert!(id("A1") < id("A2"));
    assert!(id("A9") < id("A100"));
    // Text before a number decides first; a shorter key comes before its extension.
    assert!(id("A") < id("A1"));
    assert!(id("A10") < id("B1"));
    assert!(id("A+B") < id("A.a"));
    assert!(id("A.a") < id("A.b"));
    // Equal as numbers falls back to the text, so the order agrees with equality.
    assert!(id("A7") > id("A07"));
    assert_eq!(id("A7").cmp(&id("A7")), Ordering::Equal);

    let mut ids = vec![id("#10"), id("#9"), id("#100"), id("#1")];
    ids.sort();
    assert_eq!(ids, [id("#1"), id("#9"), id("#10"), id("#100")]);
}
