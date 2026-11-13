# What is This Document?

This document is Zuri's vision and understanding of what zaf should be doing altogether. It may not represent the current state of what `zaf` is, but it is trying to be according to himself.

# What is Zaf?

Zaf was a way for models to talk to each other and coordinate using computer short-speak. LLMs are chatty—they like to speak constantly and may sometimes do this by using a lot of tokens.
Zaf was a way to solve this by having some predefined actions defined by the program that every model would use. It would be a refinement binary, since a project would have to figure out exactly what actions repeat a lot to be able to leverage them to actions.
While Zaf used to be an action runner, it slowly became a deterministic orchestrator.

## Why an Orchestrator?

Models are great! LLMs are amazing at checking what is next and getting to the next point going from A to B. However, a thing that is oftentimes forgotten is that computers were made out of deterministic processes. While a model can go from A to B, that doesn't mean that everything should be taken through the path that the model took. In pathfinding, let's imagine a grid of `nxm`:

| A   |     |     |     |     |
| --- | --- | --- | --- | --- |
|     |     |     |     |     |
|     |     |     |     |     |
|     |     |     |     | B   |

It seems limited in this one, but just follow me in here. If you were to tell a model to go from A to B, it might write a pathfinding program and get there; or it may try to solve it itself; or it may give up and tell you it cannot be solved; or it may tell you a completely wrong answer. My point is that the process is not deterministic. However, if you provide the model with the means to get from point A to point B using a pathfinding program—no matter how many times you ask it—it'll probably arrive to the answer in the least amount of tokens and with high accuracy.

This is because models, like humans, have a way of thinking that is non-deterministic. If you were to ask for something multiple times, nobody can assure you that the same answer may be given. In this same vein, I had previously created an **Orchestrator**, which was basically just a model that handled models. At first, it was a good idea—or so it seemed. The model was spawning models to orchestrate and handle complex tasks in parallel, but it hardly even handled the tokens in the way I expected. The context grew exponentially and became erratic with every compaction, so it was hard to control. Even with a knowledge base, I would get problems with things that should've been handled differently.

## How Does Zaf Help?

Zaf evolved into becoming a **Scheduler**, specifically, a Task Scheduler like the ones we find in computers. This means that spawning, despawning, working on tickets and planning on them is a _pseudo-deterministic_ operation. Models STILL design, work, implement, etc. but they do _NOT_ handle their own lifecycle like before.

# The Principle

**Authority goes down. Facts come up.**

Like React passes props down and bubbles events up, and like Rust passes ownership down and returns results up:

- **Down goes permission to act:** a turn, a slot, a briefing. The level above decides who may do what, and when.
- **Up comes what happened:** a report, stated as facts. Never a verdict on itself.
- **Each level changes only what it owns.** The lane owns positions, a ticket owns its phases, a phase owns its work.

Three tests for any design or code question:

1. **Nobody moves themselves.** A phase does not move its ticket, a ticket does not place itself in the lane, and a model does not manage its own lifecycle.
2. **A report is a fact, not a verdict.** "I raised a question" is a fact. "I'm done" is a verdict. What a fact means is decided one level above where it happened.
3. **What no level can decide keeps going up.** It ends at the Engineer: questions come up, rulings and signatures go down.

# Zaf Strategies

Zaf works through ticket management. Given a multitude of tickets, we enter a queue:

```
[1, 2, 3, 4, 5, 6, 7, 8]
```

Like any other queue, we can grab the ticket in the front until we run out of tickets:

```
[2, 3, 4, 5, 6, 7, 8]
[3, 4, 5, 6, 7, 8]
[4, 5, 6, 7, 8]
[5, 6, 7, 8]
[6, 7, 8]
[7, 8]
[8]
[]
```

At least, in an ideal world. In reality, tickets may be blocked or postponed. This creates the pattern of pushing them back:

```
[2, 3, 4, 5, 6, 7, 8, 1]
[3, 4, 5, 6, 7, 8, 1]
[4, 5, 6, 7, 8, 1, 3]
[5, 6, 7, 8, 1, 3]
```

And so on and so forth. Likewise, tickets may have phases, always one after the other. Given two tickets `[A, B]` with the phases `A1, A2, A3` and `B1, B2`, the queue holds the tickets, each at its current phase:

```
[A1, B1]
[B1, A2]
[A2]
[A3]
[A3, B1]
```

`A1` ended, so `A` went to the back at `A2`. Then `B1` asked a question, so `B` left the queue to wait and held nothing up. Once the question was answered, `B` came back, still at `B1`.

And so forth. This is because some phases require IO, such as questions, reviews, tests, etc. This makes the queue a _living structure_. Add weights to it and you're set, like a real computer scheduler.

The best part? **This is all deterministic.** A ticket has a lifecycle, a turn at our processors in different lanes and a dependency graph, increasing our parallelizing ability. This is all handled by `zaf`, a binary and not a model that creates the lifecycles, the roles, the workflows, etc. Automatically, without a model whose tokens we have to spend or thinking we have to wait on.

# Domain

zaf is a process that stays up. One scheduler runs for as long as the machine works, and everything else talks to it. The verbs are its clients: a ruling tells the scheduler, which wakes the ticket that waited on it. When no scheduler is running, a verb still works by itself, and the scheduler picks the change up from the facts when it next starts.

There is only ever one scheduler, and it is never stale:

- It holds a lock for its whole life. A second one says where the first is and exits.
- Each tick it compares itself with the installed binary, and starts again as the new one when nothing is being launched.
- Across machines, the claim on the board is still the guard.

## Memory is a cache

The queue lives in memory, in the running scheduler, and nowhere else: never in a file, never as a label on a ticket. But memory is only a cache. Everything the scheduler holds names the fact it is rebuilt from:

| What the scheduler holds | The fact behind it |
|---|---|
| Where a ticket sits | The placement facts below |
| A ticket's current phase | The first of its phases whose end fact does not hold at the pull request's head |
| How a run ended | Its run record: the exit code, any question it raised, the head it left |
| How often a phase got nowhere | The run records |
| The order of the lane | Each ticket's `ready_since` and `priority` |

So a restart, an upgrade or a crash loses nothing, and a phase that ends while no scheduler runs is still read afterwards from its record.

## Where a ticket sits

Tickets are placed from facts, not from state labels. A ticket sits in exactly one place: the first of these that holds.

1. **Running:** a phase process of it is alive (its recorded pid and start time). It leaves only when its end is recorded.
2. **Waiting on its epic:** the epic is not signed, or not challenged yet.
3. **Waiting on a question:** a question is open on it or on its epic.
4. **Waiting on a ticket:** one of its dependencies is not done. Done means closed as completed.
5. **Waiting on a check:** the checks required at its head have not all ended. No check at the head yet counts as not ended.
6. **In the lane:** none of the above.

Running comes first, so a ticket whose model is still working keeps its slot even when a question appears on it.

The board's state labels are output only: zaf writes them so the Engineer can see them, and never reads them back. What the Engineer sets on a ticket (its priority, its kind) is not a state label: it is the Engineer's input, and zaf reads it.

## The tick

I define a tick as—not time—but rather as when the next snapshot of the program exists. THIS is a tick.

- **Events only ask for a tick.** A phase ending, a ruling, a check finishing: each asks, and none moves a ticket. The tick alone moves tickets, so a ticket can never be put in two places.
- **A timer asks too,** every few minutes, for what no event announces: an answer typed straight into Forgejo, a check with no webhook.
- **A tick that cannot read the facts** (the forge is down) keeps the last placement and starts nothing.
- **Hooks stay:** `pre-tick`, `post-tick` and `post-phase`. They cost nothing and may reveal more complex patterns.

## Ticket

These are scope-defined units, each created from an external ticket of a project / epic (an issue on its board). They have a beginning and an end that's well defined. If they need to be expanded—they are not. Rather, a new ticket is added as a continuation to this one and handled based on urgency and relationship to another `ticket`.

```rust
struct Ticket {
  pub id: TicketId,
  pub title: TicketTitle, // <- Just making it easier for cache
  pub external_ticket_id: ExternalTicketId, // <- The issue on the board
  pub epic: Option<EpicId>, // <- The epic it belongs to; it waits while that epic is unsigned or unchallenged
  pub priority: Option<Priority>, // <- Optional. Set by the Engineer on the ticket; zaf never changes it
  pub size: Size, // <- What it costs of the budget while it runs
  pub phases: Vec<Phase>, // <- Always one after the other
  pub current_phase: PhaseId, // <- Derived each tick: the first phase whose end fact does not hold at the head
  pub ready_since: Utc, // <- Derived each tick: when the ticket last became ready. The longer ago, the more weight.
  pub dependencies: Vec<TicketId>, // <- Other tickets this one waits on, completely
  pub pre_run_actions: Vec<ActionId>, // Runs BEFORE the UOW starts.
  pub post_run_actions: Vec<ActionId> // Runs AFTER the UOW finishes as a whole
}

impl Ticket {
  pub async fn run(&mut self) -> Report {} // Runs `current_phase` and hands its `Report` up. The ticket knows which phase it is on; the queue does not.
}

```

The ticket is what gets queued, and the lane does the queuing: a ticket never places itself. It has a `current_phase`; the queue only knows tickets. Because the current phase is derived at the head, a push after the review opens the review again.

Before-run and after-run actions follow three rules, so a ticket is never left with no place:

- Each leaves a run record, like a phase.
- Each is safe to run twice.
- One that fails makes the ticket wait on a question.

A ticket's weight decides its turn. It is computed each tick and never stored: how long the ticket has been ready (`ready_since`), plus its `priority` when one is set. A priority is worth a fixed amount of age, so age always wins in the end and nothing starves.

A ticket is a unit of work (UOW): what we would recognize in the previous examples as `A`, `B`, etc.

## Phase

The unit of work of the unit of work: one step of a ticket. These are equivalent to `A1`, `A2`, etc.

A phase is never queued by itself. Phases run one after the other inside their ticket, so holding one back holds back the ones after it, and only the ticket moves.

```rust
struct Phase {
  pub id: PhaseId,
  pub title: PhaseTitle,
  pub ends: EndFact, // <- The fact that says this phase is over, at the pull request's head. Read, never set
  pub on_fail: Option<PhaseId>, // <- The model phase to run when this one fails (a red check runs `fix`)
  pub phase_briefing_id: Option<PhaseBriefingId>, // <- None for a phase with no model
  pub pre_run_actions: Vec<ActionId>,
  pub post_run_actions: Vec<ActionId>,
}

// What a run leaves behind: facts only, written down as its run record.
struct Report {
  pub exit: ExitCode,
  pub question: Option<QuestionId>, // <- A question this run raised
  pub head: Option<Commit>,         // <- The head it left
}

impl Phase {
  pub async fn run(&mut self) -> Report {} // Starts running the phase. Runs pre_run_actions, does its thing and then post_run_actions. It reports what happened and judges nothing.
}
```

A phase reports facts and never its own verdict. "I exited 0" is a fact; "I'm done" is a verdict, and that is the lane's to give.

## Lane

Fancy name for a queue of tickets. A lane is the event loop: it only holds tickets that can run.

```rust
// What the lane works out from a run's `Report` and the facts. Never handed up by the phase.
enum Outcome {
  DONE,               // <- To the back of the lane for its next phase; out for good when none is left
  WAIT_ON(WaitingOn), // <- Out of the lane, into `Waiting`, under what it waits on
  FAILED(Failure),    // <- Its `on_fail` phase runs next, or it tries again
}

struct Lane {
  pub lane_id: LaneId,
  pub tickets: VecDeque<Ticket>, // <- Ordered. The front gets the next free slot
}

impl Lane {
  pub fn enqueue(&mut self, ticket: Ticket) {} // The lane places a ticket; a ticket already in a place is not placed again.
  pub async fn tick(&mut self) {} // Gives each free slot to the ticket in front, by weight and within the budget, and runs its `current_phase`. From the `Report` and the facts it works out the `Outcome`, which decides where the ticket goes.
  pub async fn progress_tick(&self) {} // Smaller tick that just informs `zaf hub` of the current progress
}
```

The main thing about tickets is that they may keep appearing over and over again when asking stuff and, like it or not, these take precious ticks away. This means that a paused ticket (on a question) may accidentally hold the queue if nothing moves it out. Because of this, waiting is not a lane at all.

How the lane works out the outcome of a run, in this order:

1. **A question this run raised is still open:** wait on it. An exit code of 0 changes nothing.
2. **The phase's end fact holds at the current head:** done. For implement that means a change that is not empty, on a head that moved.
3. **The phase started something outside that has not ended:** wait on the check.
4. **Anything else:** failed.

When a phase fails:

- **A failed check runs its `on_fail` phase,** a model that fixes, and then the check applies again at the new head.
- **Getting nowhere is what counts,** not failing: the same phase ending at the same head with the same failure. Three times, and the ticket waits on a question. Done, failed, done, failed at one head is getting nowhere.
- **A failure of the machinery never counts:** an action that never ran, a forge that could not be read. The tick skips and tries later.

A phase with no model awaits. CI is running, but not on our slots: its phase starts the check and the ticket waits on it, so its slot is free. When the check ends, the next tick reads the fact and the ticket moves on or fails. There is no kind of phase to label.

Slots are not free of cost. The budget stays as it is built today: a ticket's size costs units, the machine has a budget, each project has its own slots, and a big ticket that has been passed over for too long holds the line until it fits.

## Running

Its own place, not part of the lane and not waiting.

```rust
struct Running {
  pub runs: HashMap<TicketId, Run>, // <- One per slot in use
}

struct Run {
  pub phase: PhaseId,
  pub process: Holder,      // <- The pid and the start time the OS gave it
  pub head: Option<Commit>, // <- The head when it started
}
```

A ticket whose phase a model is working on sits here and holds its slot until the end of that process is recorded, so the tick never starts it twice and never counts its slot as free.

## Waiting

Not a queue. A ticket that waits has no turn, so there is no order to keep. It sits in an unordered collection, keyed by what it waits on.

```rust
// Why a ticket waits. The reason is where the ticket sits, not a label on it.
enum WaitingOn {
  EPIC(EpicId),            // <- Its epic is not signed, or not challenged yet
  QUESTION(QuestionId),    // <- The Engineer's ruling
  TICKET(TicketId),        // <- Another ticket, completely (a dependency)
  CHECK(TicketId, Commit), // <- The checks required at this ticket's head: CI, a review
}

struct Waiting {
  pub tickets: HashMap<WaitingOn, HashSet<TicketId>>, // <- Unordered. Nothing in here takes a tick
}
```

A ticket may wait on several things at once (two other tickets, say). It is listed under each, and returns to the lane only when it is listed under none. Nothing in `Waiting` moves by itself: when a thing arrives it asks for a tick, and the tick places the ticket again from the facts.

No wait may last forever without the Engineer hearing of it:

- **Every wait has a deadline.** Past it, a question is raised on the waiting ticket: a check that never registered, a runner that is down.
- **A dependency that can never complete** (closed as not planned, a cycle, a number that is not a ticket) raises a question at once.

A completed ticket is in none of these places: it is done on the board, and that is all a tick needs.
