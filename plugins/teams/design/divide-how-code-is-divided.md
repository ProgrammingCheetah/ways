# How code is divided (SOLID): who owns a file, a type or a module, where a function lives, changing a port method or signature, an interface's shape, fakes, what depends on what, how to cut tasks and areas, parsing and types, when not to build

## What this page is for

A planner, a challenger or a programmer often has to ask: who owns this, where does this live, may I change that? Most such questions are settled by the same few principles. This page states them, so the question is settled where it comes up.

**How to use it.** When a question is about ownership, placement or an interface's shape, find the principle below that decides it, apply it, and write which one in the ticket or the report. Ask the Engineer only when two principles pull apart, when the answer would change stored data or a tool's contract (a one-way door), or when no principle decides it. "Within reason": a principle is never forced onto a question it does not decide.

## The principles

### 1. One reason to change (single responsibility)

A task is one behaviour. A module is one thing. A ticket owns only the files it has a reason to change.

- A task never creates files for another task's work. Each task adds its own file and its own `mod` line.
- A type lives with the task that must define it first, and the others consume it.
- A function lives beside what it depends on most.
- A task does not do another role's job to prove itself.
- A second behaviour found inside a ticket becomes a second ticket, and the first waits on it.

### 2. Extend, do not modify (open/closed)

What others already depend on keeps its shape. New needs are met by adding.

- A port grows by a new method with a default, never by changing a signature its callers and adapters use.
- A rule that will differ by project is a list or a setting with a default, never a constant.
- A filed ticket is never rewritten by a later plan; a re-plan adds beside it.

### 3. A stand-in must behave like the real thing (substitution)

Anything that takes the place of another keeps its promises.

- A fake board, forge or repository answers as the real adapter would, including the fields a new behaviour reads. A test that passes only because the fake is kinder than the real thing proves nothing.
- A defaulted port method gives an answer every caller can live with ("it does not say"), never a wrong one.
- An interface task's signature and its contract test are the promise. The task that later writes the body must pass that test unchanged.

### 4. Small interfaces, shaped by their readers (interface segregation)

A reader depends only on what it uses.

- A method is added for the reader that needs it, not folded into a result every other reader must then carry.
- One plain data shape read by several renderers is fine, and better than one shape per renderer, when each reader uses most of it.
- A verb takes what it needs as an argument, never from hidden state.

### 5. Depend inward, on what is abstract (dependency inversion)

The domain imports no outer layer, and a test should enforce it. Outer layers depend on ports and on the app's own shapes.

- The command line and any display read the app's view, never the domain's graph directly.
- Something read from outside is reached through its port. A new place to read from is a second source behind that port, not a way round it.
- A limit that must hold is set where the project can test it, in its own code, not in a file another tool may or may not read.

## Four more, beyond SOLID

They decide what the five above do not.

### 6. What changes together lives together (common closure)

A change should land in one place. Cut tasks, modules and areas so that one reason to change touches one of them.

- A task that must edit many files other tasks of its round also edit is cut wrongly, or needs to go alone and last.
- When two tasks keep meeting in the same file, that file holds two things: split it, or give both tasks one area so the plan takes them in turns.

### 7. A module hides one decision (information hiding)

Divide by what is likely to change, not by the steps of a process. Each module keeps one decision to itself, and the rest of the code cannot tell how it was made.

- This is where an interface belongs: around a decision, so that changing it later touches one module.
- A marker's text, a comment's format, a file's layout: one module knows it, and everyone else calls that module.

### 8. Parse once at the edge, then trust the type (parse, don't validate)

Untrusted or unchecked input becomes a type exactly once. Inside, a value that should not exist cannot be built.

- If a function needs a checked value, its argument's type says so. A plan that was not normalised should not compile where a normalised one is needed.
- Fields that are present together or absent together are one optional value, never two.
- Text from a model, the board or a file is parsed at the adapter or the domain's parser, never deeper.

### 9. Not until it is needed (YAGNI): the brake on all of the above

Build for the need in front of you. A principle is a reason to choose between two ways of doing what is asked, never a reason to do more.

- No port, setting, type or split without a reader today.
- A new method for one reader beats a general mechanism for readers that may come.

## When to ask anyway

- The answer is a one-way door: a stored format, a marker, a verb's arguments, a tool's contract.
- Two principles disagree, and the page gives no order between them.
- The ticket, the design and the code say three different things.

Then raise the question, and say which principles you weighed and what you would pick.
