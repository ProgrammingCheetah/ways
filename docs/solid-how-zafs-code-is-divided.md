# How zaf's code is divided

v1, 2026-11-23. Written by Claude on the Engineer's word ("Draft the page") and approved by the Engineer the same day ("Approved"). A line in quotes is the Engineer's. Each example is a ruling of 0.6.0's build, named by its question's number.

## What this page is for

A planner, a challenger or a programmer often has to ask: who owns this, where does this live, may I change that? On 0.6.0's build about a third of the questions to the Engineer were of that kind, and each was settled by the same few principles. The Engineer: "recommendations should be cemented in good architecture, such as SOLID, within reason."

This page states those principles in zaf's own terms, so the question is settled where it comes up.

**How to use it.** When a question is about ownership, placement or an interface's shape, find the principle below that decides it, apply it, and write which one in the ticket or the report. Ask the Engineer only when two principles pull apart, when the answer would change stored data or a tool's contract (a one-way door), or when no principle decides it. "Within reason": a principle is never forced onto a question it does not decide. A slip in a ticket is a slip, and a flaky test is a flaky test.

## The principles

### 1. One reason to change (single responsibility)

A task is one behaviour. A module is one thing. A ticket owns only the files it has a reason to change.

- A task never creates files for another task's work. Each task adds its own file and its own `mod` line (#1277).
- A type lives with the task that must define it first, and the others consume it. `Measures` is defined by the task that loads the fixtures, and `measure` consumes it (#1237).
- A function lives beside what it depends on most. `PlanGraph::before` parses a plan, so it lives beside the plan parser (#1178).
- A task does not do another role's job to prove itself. A programmer does not drive the spawner; that check belongs to a run that really spawns (#1294).
- A second behaviour found inside a ticket becomes a second ticket, and the first waits on it (#1312, #1319).

### 2. Extend, do not modify (open/closed)

What others already depend on keeps its shape. New needs are met by adding.

- A port grows by a new method with a default, never by changing a signature its callers and adapters use. The report needed a question's opened time: `Board::opened_at` was added beside `standing_at`, and `questions_on` kept its shape (#1319).
- A rule that will differ by project is a list or a setting with a default, never a constant. Records are read from the board's trusted logins, defaulting to zaf's own, because a teammate's zaf posts under another login (#1312).
- A filed ticket is never rewritten by a later plan; a re-plan adds beside it (the design's D29).

### 3. A stand-in must behave like the real thing (substitution)

Anything that takes the place of another keeps its promises.

- A fake board, forge or repository answers as the real adapter would, including the fields a new behaviour reads (authors on comments, opened times). A test that passes only because the fake is kinder than the real thing proves nothing.
- A defaulted port method gives an answer every caller can live with ("the board does not say"), never a wrong one.
- An interface task's signature and its contract test are the promise. The task that later writes the body must pass that test unchanged.

### 4. Small interfaces, shaped by their readers (interface segregation)

A reader depends only on what it uses.

- A method is added for the reader that needs it, not folded into a result every other reader must then carry (#1319).
- One plain data shape read by several renderers is fine, and better than one shape per renderer, when each reader uses most of it. `PlanView` is data only, and each renderer lives with its reader (#1275).
- A verb takes what it needs as an argument, never from hidden state. `zaf plan check <epic>` takes the epic; it does not read it from the environment (#1210).

### 5. Depend inward, on what is abstract (dependency inversion)

The domain imports no outer layer; `tests/architecture.rs` enforces it. Outer layers depend on ports and on the app's own shapes.

- The command line and the chair read the app's view (`PlanView`), never the domain's graph directly (#1275).
- Something read from outside is reached through its port. The keeper and the brief read design pages through the design port; a new place for pages is a second source behind that port, not a way round it (#1276).
- A limit that must hold is set where zaf can test it, in zaf's own code, not in a file another tool may or may not read. The challenger's tool list is set in zaf's launch, with a test on the arguments (#1269).

## Four more, beyond SOLID

Added on the Engineer's word (2026-11-23: "I like those principles"). They decide what the five above do not.

### 6. What changes together lives together (common closure)

A change should land in one place. Cut tasks, modules and areas so that one reason to change touches one of them.

- A task that must edit many files other tasks of its round also edit is cut wrongly, or needs to go alone and last. #1195 moved every reader of a task body and conflicted with four sibling tasks that had merged first.
- When two tasks keep meeting in the same file, that file holds two things: split it, or give both tasks one area so the plan takes them in turns.

### 7. A module hides one decision (information hiding)

Divide by what is likely to change, not by the steps of a process. Each module keeps one decision to itself, and the rest of the code cannot tell how it was made.

- This is where an interface belongs: around a decision, so that changing it later touches one module.
- A marker's text, a comment's format, a file's layout: one module knows it, and everyone else calls that module (the plan record and the filing mark, I8).

### 8. Parse once at the edge, then trust the type (parse, don't validate)

Untrusted or unchecked input becomes a type exactly once. Inside, a value that should not exist cannot be built.

- If a function needs a checked value, its argument's type says so. `refuse` reads a normalised plan; a plan that was not normalised reached it twice on 0.6.0's build (#1333, and `optimise`), because both were the same type. A type for the normalised plan would not have compiled.
- Fields that are present together or absent together are one optional value, never two (#1350).
- Text from a model, the board or a file is parsed at the adapter or the domain's parser, never deeper.

### 9. Not until it is needed (YAGNI): the brake on all of the above

Build for the need in front of you. A principle is a reason to choose between two ways of doing what is asked, never a reason to do more.

- No port, setting, type or split without a reader today. "Unless small enough" is part of the Engineer's rule for ports and adapters.
- A new method for one reader beats a general mechanism for readers that may come.

## What goes with them in zaf

These are the Engineer's standing rulings, of the same family:

- **Ports and adapters by default**, unless the thing is small enough not to need them.
- **Domain shapes live in the domain**, which imports no outer layer.
- **Newtypes over bare strings and numbers** (a login is a `Login`, not a `String`).
- **State is derived on read.** Nothing is stored that can be calculated; a cache is allowed only if it resists corruption and heals itself.
- **Deterministic over non-deterministic.** zaf decides between steps; a model works inside one and is checked, never trusted.

## When to ask anyway

- The answer is a one-way door: a stored format, a marker, a verb's arguments, a tool's contract.
- Two principles disagree, and the page gives no order between them.
- The ticket, the design and the code say three different things.

Then raise the question, and say which principles you weighed and what you would pick.
