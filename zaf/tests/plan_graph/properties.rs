//! What always holds, on generated plans. The generator: [`random_plan`] makes a plan of one of
//! the reference's seven kinds from a seed, the same plan for the same seed on every machine. It
//! ports `random_plan` and its kinds from `test_reference.py`. Then the twelve properties of D8,
//! one test each (`property_*`), over [`SEEDS`] seeds in the four option sets; a plan the
//! optimiser refuses is skipped, as the reference skips it. A failure names the seed and the
//! option set.
//!
//! The randomness is [`Rng`], SplitMix64 written here (`rand` is banned). Its sequence is not
//! Python's: only the plans' shapes and odds are the reference's.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use zaf::domain::plan::graph::Dag;
use zaf::domain::plan::measure::measure;
use zaf::domain::plan::names::normalise;
use zaf::domain::plan::optimise::optimise;
use zaf::domain::plan::{
    Area, Behaviour, EpicKey, Filed, FiledWith, Interfaces, Lift, Name, NewNames, Node, NodeId,
    NodeKey, NodeKind, Optimised, Options, PlanInput, Size, Turn, Vocabulary, Weight,
};

/// SplitMix64: a small seeded generator. The same seed gives the same sequence anywhere, since
/// it is integer arithmetic alone.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..bound`. The bias of a plain modulo is nil at the sizes used here.
    pub fn below(&mut self, bound: usize) -> usize {
        assert!(bound > 0, "below needs a bound");
        (self.next_u64() % bound as u64) as usize
    }

    /// A number in `low..=high`.
    pub fn between(&mut self, low: usize, high: usize) -> usize {
        low + self.below(high - low + 1)
    }

    /// One of the items; an item listed twice is twice as likely.
    pub fn choice<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }

    /// A number in `[0, 1)`.
    pub fn random(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Whether a draw falls under `odds`.
    pub fn chance(&mut self, odds: f64) -> bool {
        self.random() < odds
    }

    /// `count` of the items, each once, in a random order; fewer when there are fewer.
    pub fn sample<T: Clone>(&mut self, items: &[T], count: usize) -> Vec<T> {
        let mut pool = items.to_vec();
        let count = count.min(pool.len());
        for at in 0..count {
            let pick = at + self.below(pool.len() - at);
            pool.swap(at, pick);
        }
        pool.truncate(count);
        pool
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for at in (1..items.len()).rev() {
            let pick = self.below(at + 1);
            items.swap(at, pick);
        }
    }
}

/// The seven kinds of plan the reference makes, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// The plan as `random_plan` makes it, its lists written in the plan.
    Plain,
    /// The lists moved into a vocabulary, with a few names the plan adds.
    Vocabulary,
    /// Some needs are other epics' names.
    Outside,
    /// Every node of one epic.
    OneEpic,
    /// One epic's plan with names other epics need.
    Shared,
    /// A plan filed before 0.6.0, with `Blocked by` lines.
    BlockedBy,
    /// A second plan beside the tickets the first one filed.
    Replan,
}

pub const KINDS: [Kind; 7] = [
    Kind::Plain,
    Kind::Vocabulary,
    Kind::Outside,
    Kind::OneEpic,
    Kind::Shared,
    Kind::BlockedBy,
    Kind::Replan,
];

/// The four option sets: lift all or none, interfaces one or each.
pub const OPTION_SETS: [Options; 4] = [
    Options {
        lift: Lift::All,
        interfaces: Interfaces::Each,
    },
    Options {
        lift: Lift::All,
        interfaces: Interfaces::One,
    },
    Options {
        lift: Lift::None,
        interfaces: Interfaces::Each,
    },
    Options {
        lift: Lift::None,
        interfaces: Interfaces::One,
    },
];

/// A generated plan and the kind it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub kind: Kind,
    pub plan: PlanInput,
}

/// The plan a seed gives: its kind goes round the seven by seed, as the reference's rounds do,
/// and the plan is made from the seed alone. `options` shape only a re-plan, whose first plan
/// is optimised with them before it is filed.
pub fn random_plan(seed: u64, options: Options) -> Generated {
    let kind = KINDS[(seed % KINDS.len() as u64) as usize];
    Generated {
        kind,
        plan: plan_of_kind(kind, seed, options),
    }
}

/// The plan of one kind a seed gives.
pub fn plan_of_kind(kind: Kind, seed: u64, options: Options) -> PlanInput {
    let mut rng = Rng::new(seed);
    // The kind's own draws come from a second stream, so a kind never moves the base plan.
    let mut own = Rng::new(seed ^ 0xD1B5_4A32_D192_ED03);
    match kind {
        Kind::Plain => base_plan(&mut rng),
        Kind::Vocabulary => with_vocabulary(&mut own, &base_plan(&mut rng)),
        Kind::Outside => with_outside(&mut own, &base_plan(&mut rng)),
        Kind::OneEpic => in_one_epic(&base_plan(&mut rng)),
        Kind::Shared => with_shared(&mut own, &in_one_epic(&base_plan(&mut rng))),
        Kind::BlockedBy => filed_with_blocked_by(&mut own),
        Kind::Replan => replanned(&mut own, &base_plan(&mut rng), options),
    }
}

fn name(text: impl AsRef<str>) -> Name {
    Name::from(text.as_ref())
}

fn weight(rng: &mut Rng) -> Size {
    Size::Weight(Weight(rng.choice(&[1, 1, 2])))
}

fn names_set(names: &[&str]) -> BTreeSet<Name> {
    names.iter().map(|n| Name::from(*n)).collect()
}

/// `random_plan`: a plan that is never broken: a node only needs names from nodes made before it.
fn base_plan(rng: &mut Rng) -> PlanInput {
    let count = rng.between(1, 14);
    let areas: Vec<Area> = (0..rng.between(1, 6))
        .map(|i| Area::from(format!("area{i}")))
        .collect();
    let mut provided: Vec<Name> = Vec::new();
    let mut nodes = Vec::new();
    for index in 0..count {
        let mut behaviours = Vec::new();
        for part in 0..rng.choice(&[1, 1, 1, 2]) {
            let provides = name(format!("n{index}.{part}"));
            let take = rng.choice(&[0, 0, 1, 1, 2, 3]);
            let mut needs: BTreeSet<Name> = rng.sample(&provided, take).into_iter().collect();
            let size = weight(rng);
            let write_count = if areas.len() > 1 {
                rng.choice(&[0, 1, 1, 2])
            } else {
                1
            };
            let writes = rng.sample(&areas, write_count).into_iter().collect();
            if rng.chance(0.2) {
                needs.insert(name("old"));
            }
            behaviours.push(Behaviour {
                text: format!("behaviour {index}.{part}"),
                size,
                writes,
                provides: BTreeSet::from([provides]),
                needs,
            });
        }
        provided.extend(behaviours.iter().flat_map(|b| b.provides.iter().cloned()));
        nodes.push(node(format!("{}", 100 + index), behaviours));
    }
    rng.shuffle(&mut nodes);
    let lanes = rng.choice(&[2, 4]);
    let no_signature: BTreeSet<Name> = provided
        .iter()
        .filter(|_| rng.chance(0.25))
        .cloned()
        .collect();
    let mut plan = PlanInput::of(nodes);
    plan.limit = Weight(2);
    plan.lanes = lanes;
    plan.existing = Some(names_set(&["old"]));
    plan.no_signature = Some(no_signature);
    plan
}

fn node(id: String, behaviours: Vec<Behaviour>) -> Node {
    Node {
        id: NodeId::from(id.as_str()),
        kind: NodeKind::Body,
        filed: None,
        turns: Vec::new(),
        blocked_by: Vec::new(),
        epic: None,
        behaviours,
    }
}

/// Every name the plan's nodes provide.
fn provided_names(plan: &PlanInput) -> BTreeSet<Name> {
    plan.nodes
        .iter()
        .flat_map(|n| &n.behaviours)
        .flat_map(|b| b.provides.iter().cloned())
        .collect()
}

/// The lists move into a vocabulary; the plan adds a few names of its own.
fn with_vocabulary(rng: &mut Rng, plan: &PlanInput) -> PlanInput {
    let mut plan = plan.clone();
    let no_signature = plan.no_signature.take().unwrap_or_default();
    let existing = plan.existing.take().unwrap_or_default();
    let names = provided_names(&plan);
    let added: BTreeSet<Name> = names.iter().filter(|_| rng.chance(0.2)).cloned().collect();
    let areas: BTreeSet<Area> = plan
        .nodes
        .iter()
        .flat_map(|n| &n.behaviours)
        .flat_map(|b| b.writes.iter().cloned())
        .collect();
    let pick = |signed: bool, kept: bool| -> BTreeSet<Name> {
        names
            .iter()
            .filter(|n| no_signature.contains(*n) != signed && (added.contains(*n) != kept))
            .cloned()
            .collect()
    };
    plan.vocabulary = Some(Vocabulary {
        areas,
        signature: pick(false, false),
        no_signature: pick(true, false),
        existing,
        epic: Default::default(),
    });
    plan.new_names = Some(NewNames {
        signature: pick(false, true),
        no_signature: pick(true, true),
        ..NewNames::default()
    });
    plan
}

/// Some needs are other epics' names: one with a signature sits in that epic's interface ticket
/// (D13), one with none in its task.
fn with_outside(rng: &mut Rng, plan: &PlanInput) -> PlanInput {
    let mut plan = plan.clone();
    plan.outside = [
        (name("ext.sig"), NodeId::from("I@9.1")),
        (name("ext.plain"), NodeId::from("#901")),
    ]
    .into();
    plan.no_signature
        .get_or_insert_with(BTreeSet::new)
        .insert(name("ext.plain"));
    for node in &mut plan.nodes {
        for behaviour in &mut node.behaviours {
            if rng.chance(0.3) {
                let outside = rng.choice(&["ext.sig", "ext.plain"]);
                behaviour.needs.insert(name(outside));
            }
        }
    }
    plan
}

fn in_one_epic(plan: &PlanInput) -> PlanInput {
    let mut plan = plan.clone();
    for node in &mut plan.nodes {
        node.epic = Some(EpicKey::from("7"));
    }
    plan
}

fn with_shared(rng: &mut Rng, plan: &PlanInput) -> PlanInput {
    let mut plan = plan.clone();
    plan.shared = provided_names(&plan)
        .into_iter()
        .filter(|_| rng.chance(0.3))
        .collect();
    plan
}

/// A plan filed before 0.6.0: tasks, `Blocked by` lines, one of them naming an epic.
fn filed_with_blocked_by(rng: &mut Rng) -> PlanInput {
    let count = rng.between(1, 12);
    let mut nodes = Vec::new();
    for index in 0..count {
        let earlier: Vec<NodeKey> = (0..index)
            .map(|i| NodeKey::from(format!("{}", 100 + i)))
            .collect();
        let take = rng.choice(&[0, 1, 1, 2]);
        let mut blockers = rng.sample(&earlier, take);
        if rng.chance(0.2) {
            blockers.push(NodeKey::from("E9"));
        }
        let size = weight(rng);
        let mut task = node(
            format!("{}", 100 + index),
            vec![Behaviour {
                text: format!("task {index}"),
                size,
                writes: BTreeSet::new(),
                provides: BTreeSet::new(),
                needs: BTreeSet::new(),
            }],
        );
        task.blocked_by = blockers;
        nodes.push(task);
    }
    rng.shuffle(&mut nodes);
    let mut plan = PlanInput::of(nodes);
    plan.filed_with = Some(FiledWith::BlockedBy);
    plan.epics = [(
        NodeKey::from("E9"),
        vec![NodeId::from("900"), NodeId::from("901")],
    )]
    .into();
    plan
}

/// The epic's first plan, optimised and filed (each ticket open or done), then a second plan
/// beside it (D29). New tasks need new names, and filed names only where no interface is
/// missing: an interface's, a name with no signature, or a done task's.
fn replanned(rng: &mut Rng, plan: &PlanInput, options: Options) -> PlanInput {
    let first_input = in_one_epic(plan);
    let first = optimise(&first_input, options).expect("the first plan is never broken");
    let mut done: BTreeSet<NodeId> = first
        .nodes
        .iter()
        .filter(|_| rng.chance(0.3))
        .map(|n| n.id.clone())
        .collect();
    // A task is done only once all it waits on is done.
    loop {
        let before = done.len();
        for order in &first.orders {
            if done.contains(&order.to) {
                done.insert(order.from.clone());
            }
        }
        if done.len() == before {
            break;
        }
    }
    let no_signature = first_input.no_signature.clone().unwrap_or_default();
    let filed: Vec<Node> = first
        .nodes
        .iter()
        .map(|n| {
            let mut n = n.clone();
            n.filed = Some(if done.contains(&n.id) {
                Filed::Done
            } else {
                Filed::Open
            });
            n.turns = first
                .orders
                .iter()
                .filter(|o| o.to == n.id)
                .filter_map(|o| {
                    let area = o.needs.as_str().strip_prefix("turn:")?;
                    Some(Turn(o.from.clone(), Area::from(area)))
                })
                .collect();
            n
        })
        .collect();
    let usable: BTreeSet<Name> = filed
        .iter()
        .flat_map(|n| {
            n.behaviours
                .iter()
                .flat_map(|b| b.provides.iter().cloned())
                .map(move |name| (n, name))
        })
        .filter(|(n, name)| {
            n.kind == NodeKind::Interface
                || no_signature.contains(name)
                || n.filed == Some(Filed::Done)
        })
        .map(|(_, name)| name)
        .collect();
    let mut areas: Vec<Area> = filed
        .iter()
        .flat_map(|n| &n.behaviours)
        .flat_map(|b| b.writes.iter())
        .filter(|a| !a.as_str().starts_with("sig:"))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if areas.is_empty() {
        areas.push(Area::from("area0"));
    }
    let mut new = Vec::new();
    let mut made: Vec<Name> = Vec::new();
    for index in 0..rng.between(1, 6) {
        let provides = name(format!("r{index}"));
        let pool: Vec<Name> = usable.iter().cloned().chain(made.iter().cloned()).collect();
        let size = weight(rng);
        let writes = rng.sample(&areas, 1).into_iter().collect();
        let take = rng.choice(&[0, 1, 1, 2]);
        let needs = rng.sample(&pool, take).into_iter().collect();
        let mut task = node(
            format!("R{index}"),
            vec![Behaviour {
                text: format!("re-plan {index}"),
                size,
                writes,
                provides: BTreeSet::from([provides.clone()]),
                needs,
            }],
        );
        task.epic = Some(EpicKey::from("7"));
        new.push(task);
        made.push(provides);
    }
    let mut no_signature = no_signature;
    no_signature.extend(made.into_iter().filter(|_| rng.chance(0.25)));
    let mut nodes = filed;
    nodes.extend(new);
    let mut plan = PlanInput::of(nodes);
    plan.limit = Weight(2);
    plan.lanes = first_input.lanes;
    plan.record = 2;
    plan.existing = first_input.existing;
    plan.no_signature = Some(no_signature);
    plan
}

const SEEDS: u64 = 400;

#[test]
fn generator_is_seeded() {
    for seed in 0..SEEDS {
        for options in OPTION_SETS {
            let first = random_plan(seed, options);
            let again = random_plan(seed, options);
            assert_eq!(first, again, "seed {seed} gives one plan");
        }
    }
    // The seed decides: some two seeds of one kind give different plans.
    let plans: BTreeSet<String> = (0..SEEDS)
        .map(|seed| serde_json::to_string(&random_plan(seed, OPTION_SETS[0]).plan).unwrap())
        .collect();
    assert!(
        plans.len() > 300,
        "seeds give many plans, got {}",
        plans.len()
    );
    // A fixed value, so the sequence is the same on every machine.
    let mut rng = Rng::new(0);
    assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
}

#[test]
fn generator_covers_kinds() {
    let mut seen: Vec<Kind> = Vec::new();
    for seed in 0..SEEDS {
        for options in OPTION_SETS {
            let generated = random_plan(seed, options);
            assert!(!generated.plan.nodes.is_empty(), "seed {seed} has nodes");
            // The plan loads as a plan reads: through its own JSON.
            let text = serde_json::to_string(&generated.plan).expect("a plan writes");
            let loaded: PlanInput = serde_json::from_str(&text)
                .unwrap_or_else(|error| panic!("seed {seed} does not load: {error}"));
            assert_eq!(loaded, generated.plan, "seed {seed} loads as it was made");
            seen.push(generated.kind);
        }
    }
    for kind in KINDS {
        assert!(seen.contains(&kind), "{kind:?} appears over {SEEDS} seeds");
    }
    // Each kind has what makes it that kind.
    for seed in 0..SEEDS {
        let plan = random_plan(seed, OPTION_SETS[0]);
        match plan.kind {
            Kind::Plain => assert!(plan.plan.vocabulary.is_none() && plan.plan.epic.is_none()),
            Kind::Vocabulary => assert!(plan.plan.vocabulary.is_some()),
            Kind::Outside => assert_eq!(plan.plan.outside.len(), 2),
            Kind::OneEpic | Kind::Shared => {
                assert!(plan.plan.nodes.iter().all(|n| n.epic.is_some()));
            }
            Kind::BlockedBy => assert_eq!(plan.plan.filed_with, Some(FiledWith::BlockedBy)),
            Kind::Replan => assert!(plan.plan.nodes.iter().any(|n| n.filed.is_some())),
        }
    }
}

/// One generated plan, run through the optimiser.
struct Case {
    seed: u64,
    options: Options,
    kind: Kind,
    plan: PlanInput,
    /// What the optimiser gave; a case it refused is not kept.
    out: Optimised,
}

impl Case {
    /// What a failure names: the seed, the option set and the kind of plan.
    fn label(&self) -> String {
        format!(
            "seed {}, lift {:?}, interfaces {:?}, {:?} plan",
            self.seed, self.options.lift, self.options.interfaces, self.kind
        )
    }

    /// The plan's `no_signature` as the optimiser reads it: the vocabulary's and the plan's,
    /// and the names a `Blocked by` reading makes.
    fn no_signature(&self) -> BTreeSet<Name> {
        let plan = normalise(&self.plan).unwrap_or_else(|_| panic!("{}: normalises", self.label()));
        plan.no_signature.unwrap_or_default()
    }
}

/// The work on each item, over as many threads as the machine has: the optimiser is slow in a
/// debug build, and 1600 plans a property would run past the light gate's time. The results come
/// back in the items' order; a check on them is made by the caller, on its own thread, so a
/// failure names its seed.
fn in_parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let size = items.len().div_ceil(threads * 4).max(1);
    let work = &work;
    std::thread::scope(|scope| {
        let workers: Vec<_> = items
            .chunks(size)
            .map(|chunk| scope.spawn(move || chunk.iter().map(work).collect::<Vec<R>>()))
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("a worker finishes"))
            .collect()
    })
}

/// Every seed in every option set, each plan optimised once for all the tests. A refused plan is
/// skipped.
fn cases() -> &'static [Case] {
    static CASES: OnceLock<Vec<Case>> = OnceLock::new();
    CASES.get_or_init(|| {
        let seeds: Vec<u64> = (0..SEEDS).collect();
        let of_seed = |&seed: &u64| -> Vec<Case> {
            OPTION_SETS
                .iter()
                .filter_map(|&options| {
                    let Generated { kind, plan } = random_plan(seed, options);
                    let out = optimise(&plan, options).ok()?;
                    Some(Case {
                        seed,
                        options,
                        kind,
                        plan,
                        out,
                    })
                })
                .collect()
        };
        in_parallel(&seeds, of_seed).into_iter().flatten().collect()
    })
}

/// What a node weighs: the sizes of its behaviours.
fn size(node: &Node) -> u32 {
    node.behaviours
        .iter()
        .map(|b| b.size.weight().expect("a generated size is a weight").0)
        .sum()
}

fn writes(node: &Node) -> BTreeSet<&Area> {
    node.behaviours.iter().flat_map(|b| &b.writes).collect()
}

/// What a plan builds: the text of every behaviour of a body node, sorted.
fn meaning<'a>(nodes: impl IntoIterator<Item = &'a Node>) -> Vec<&'a str> {
    let mut texts: Vec<&str> = nodes
        .into_iter()
        .filter(|n| n.kind == NodeKind::Body)
        .flat_map(|n| n.behaviours.iter().map(|b| b.text.as_str()))
        .collect();
    texts.sort_unstable();
    texts
}

/// The waits as pairs, each pair once.
fn pairs(out: &Optimised) -> BTreeSet<(NodeId, NodeId)> {
    out.orders
        .iter()
        .map(|o| (o.from.clone(), o.to.clone()))
        .collect()
}

/// Every node reached from `start` along `edges`.
fn reached(start: &NodeId, edges: &[(NodeId, NodeId)]) -> BTreeSet<NodeId> {
    let mut next: BTreeMap<&NodeId, Vec<&NodeId>> = BTreeMap::new();
    for (from, to) in edges {
        next.entry(from).or_default().push(to);
    }
    let mut seen = BTreeSet::new();
    let mut stack = vec![start];
    while let Some(id) = stack.pop() {
        for &to in next.get(id).into_iter().flatten() {
            if seen.insert(to.clone()) {
                stack.push(to);
            }
        }
    }
    seen
}

#[test]
fn property_plans_run() {
    // The properties are not vacuous: most plans are let through, and every kind is among them.
    let total = SEEDS as usize * OPTION_SETS.len();
    let run = cases();
    assert!(
        run.len() * 10 >= total * 9,
        "{} of {total} plans were let through",
        run.len()
    );
    for kind in KINDS {
        assert!(run.iter().any(|c| c.kind == kind), "{kind:?} was run");
    }
}

#[test]
fn property_1_the_meaning_is_kept() {
    for case in cases() {
        let live = case
            .plan
            .nodes
            .iter()
            .filter(|n| n.filed != Some(Filed::Done));
        assert_eq!(
            meaning(&case.out.nodes),
            meaning(live),
            "{}: what the plan builds changed",
            case.label()
        );
    }
}

#[test]
fn property_2_no_cycle_both_ends_ids_unique() {
    for case in cases() {
        let label = case.label();
        let ids: Vec<NodeId> = case.out.nodes.iter().map(|n| n.id.clone()).collect();
        let unique: BTreeSet<&NodeId> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "{label}: ids are not unique");
        for order in &case.out.orders {
            assert!(
                unique.contains(&order.from),
                "{label}: no node {}",
                order.from
            );
            assert!(unique.contains(&order.to), "{label}: no node {}", order.to);
        }
        let waits: Vec<(NodeId, NodeId)> = pairs(&case.out).into_iter().collect();
        assert_eq!(Dag::new(&ids, &waits).cycle(), None, "{label}: a cycle");
        let width = measure(&case.out, case.plan.lanes).width;
        assert_eq!(
            width.iter().sum::<usize>(),
            ids.len(),
            "{label}: a node has no layer"
        );
    }
}

#[test]
fn property_3_no_body_node_passes_the_limit() {
    for case in cases() {
        for node in case.out.nodes.iter().filter(|n| n.kind == NodeKind::Body) {
            assert!(
                size(node) <= case.plan.limit.0,
                "{}: {} weighs {}, over {}",
                case.label(),
                node.id,
                size(node),
                case.plan.limit
            );
        }
    }
}

#[test]
fn property_4_listing_order_makes_no_difference() {
    let again = in_parallel(cases(), |case| {
        let mut shuffled = case.plan.clone();
        Rng::new(case.seed ^ 0x7).shuffle(&mut shuffled.nodes);
        optimise(&shuffled, case.options)
    });
    for (case, again) in cases().iter().zip(again) {
        assert_eq!(
            again,
            Ok(case.out.clone()),
            "{}: the plan listed another way",
            case.label()
        );
    }
}

#[test]
fn property_5_no_two_writers_of_one_area_are_unordered() {
    for case in cases() {
        let edges: Vec<(NodeId, NodeId)> = pairs(&case.out).into_iter().collect();
        let reach: BTreeMap<&NodeId, BTreeSet<NodeId>> = case
            .out
            .nodes
            .iter()
            .map(|n| (&n.id, reached(&n.id, &edges)))
            .collect();
        let nodes = &case.out.nodes;
        for (at, a) in nodes.iter().enumerate() {
            for b in &nodes[at + 1..] {
                if writes(a).is_disjoint(&writes(b)) {
                    continue;
                }
                assert!(
                    reach[&a.id].contains(&b.id) || reach[&b.id].contains(&a.id),
                    "{}: {} and {} write one area and wait on neither",
                    case.label(),
                    a.id,
                    b.id
                );
            }
        }
    }
}

#[test]
fn property_6_no_wait_is_one_another_path_implies() {
    for case in cases() {
        let waits: Vec<(NodeId, NodeId)> = case
            .out
            .orders
            .iter()
            .map(|o| (o.from.clone(), o.to.clone()))
            .collect();
        for (at, (from, to)) in waits.iter().enumerate() {
            let mut rest = waits.clone();
            rest.remove(at);
            assert!(
                !reached(from, &rest).contains(to),
                "{}: the wait {from} -> {to} is implied by another path",
                case.label()
            );
        }
    }
}

#[test]
fn property_7_after_lifting_a_body_waits_on_a_body_only_without_a_signature() {
    let mut checked = 0;
    for case in cases().iter().filter(|c| c.options.lift == Lift::All) {
        let no_signature = case.no_signature();
        let kind: BTreeMap<&NodeId, NodeKind> =
            case.out.nodes.iter().map(|n| (&n.id, n.kind)).collect();
        for order in &case.out.orders {
            if kind[&order.from] == NodeKind::Body {
                assert!(
                    order.needs.as_str().starts_with("turn:")
                        || no_signature.contains(&order.needs),
                    "{}: {} waits on body {} for {}, which has a signature",
                    case.label(),
                    order.to,
                    order.from,
                    order.needs
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "some body waits on a body");
}

#[test]
fn property_8_a_rerun_changes_nothing_ids_included() {
    // The output as the next run's input (D9): its nodes and the plan's lists, no waits.
    let again = in_parallel(cases(), |case| {
        let normal = normalise(&case.plan).expect("a plan that was run normalises");
        let mut input = PlanInput::of(case.out.nodes.clone());
        input.limit = normal.limit;
        input.lanes = normal.lanes;
        input.record = normal.record;
        input.shared = normal.shared;
        input.outside = normal.outside;
        input.no_signature = normal.no_signature;
        input.existing = normal.existing;
        optimise(&input, case.options)
    });
    for (case, second) in cases().iter().zip(again) {
        let second =
            second.unwrap_or_else(|r| panic!("{}: the rerun is refused: {r:?}", case.label()));
        assert_eq!(
            second,
            case.out,
            "{}: a rerun changed the plan",
            case.label()
        );
        assert_eq!(
            measure(&second, case.plan.lanes),
            measure(&case.out, case.plan.lanes),
            "{}: a rerun changed the measures",
            case.label()
        );
    }
}

#[test]
fn property_9_the_critical_path_is_a_path_of_the_critical_length() {
    for case in cases() {
        let label = case.label();
        let measures = measure(&case.out, case.plan.lanes);
        let path = &measures.critical_path;
        assert!(!path.is_empty(), "{label}: no critical path");
        let by_id: BTreeMap<&NodeId, &Node> = case.out.nodes.iter().map(|n| (&n.id, n)).collect();
        let total: u32 = path.iter().map(|id| size(by_id[id])).sum();
        assert_eq!(total, measures.critical, "{label}: the sizes do not add up");
        let waits = pairs(&case.out);
        for step in path.windows(2) {
            assert!(
                waits.contains(&(step[0].clone(), step[1].clone())),
                "{label}: no wait {} -> {}",
                step[0],
                step[1]
            );
        }
        assert!(
            waits.iter().all(|(_, to)| *to != path[0]),
            "{label}: the path starts at a node that waits"
        );
    }
}

#[test]
fn property_10_lifting_alone_gives_a_chain_of_two_or_less() {
    // Lifting alone: no conflicts (no writes) and no name without a signature.
    let mut checked = 0;
    for case in cases().iter().filter(|c| c.kind == Kind::Plain) {
        if case.options.lift != Lift::All || !case.no_signature().is_empty() {
            continue;
        }
        let mut quiet = case.plan.clone();
        for behaviour in quiet.nodes.iter_mut().flat_map(|n| &mut n.behaviours) {
            behaviour.writes.clear();
        }
        let out = optimise(&quiet, case.options)
            .unwrap_or_else(|r| panic!("{}: refused when quiet: {r:?}", case.label()));
        let chain = measure(&out, quiet.lanes).chain;
        assert!(chain <= 2, "{}: a chain of {chain}", case.label());
        checked += 1;
    }
    assert!(checked > 0, "some plan lifts alone");
}

#[test]
fn property_11_a_kept_wait_on_a_signature_comes_from_an_interface() {
    let mut checked = 0;
    for case in cases() {
        let no_signature = case.no_signature();
        for wait in &case.out.kept {
            if !no_signature.contains(&wait.needs) {
                assert!(
                    wait.from.as_str().starts_with('I'),
                    "{}: {} -> {} on {} has a signature",
                    case.label(),
                    wait.from,
                    wait.to,
                    wait.needs
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "some kept wait is on a name with a signature");
}

#[test]
fn property_12_a_replan_never_changes_a_filed_ticket() {
    let mut checked = 0;
    for case in cases().iter().filter(|c| c.kind == Kind::Replan) {
        let by_id: BTreeMap<&NodeId, &Node> = case.out.nodes.iter().map(|n| (&n.id, n)).collect();
        for node in case
            .plan
            .nodes
            .iter()
            .filter(|n| n.filed == Some(Filed::Open))
        {
            let kept = by_id
                .get(&node.id)
                .unwrap_or_else(|| panic!("{}: {} is gone", case.label(), node.id));
            assert_eq!(
                kept.behaviours,
                node.behaviours,
                "{}: {} changed",
                case.label(),
                node.id
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "some re-plan holds an open ticket");
}
