//! In-memory fakes, one per port: the ports' first consumer and the ground for verb tests.
//!
//! Each fake keeps its state behind a lock, follows the port's contract
//! (idempotent repeats included), and adds a few setup and inspection methods
//! a test needs to arrange a scene and read the result.

mod board;
mod calls;
mod ci_filter;
mod design;
mod forge;
mod gate;
mod graph_epic;
mod releases;
mod runner;
mod vcs;
mod verb_runner;

/// A planned ticket body (I7) whose size is derived: two behaviours, each sized `S`, weigh 2 and
/// so make a ticket of size `M`; no section says `M`, and no line says `Blocked by`.
pub const PLANNED_M_BODY: &str = "Node: A+B\n\
    Behaviour: a\nGoal: first\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
    Acceptance:\n  - the first holds\nTests: first_holds\nSecurity trigger: no\n\
    Model: sonnet\nSize: S\n\
    Behaviour: b\nGoal: second\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
    Acceptance:\n  - the second holds\nTests: second_holds\nSecurity trigger: no\n\
    Model: sonnet\nSize: S\n";

pub use board::{FakeBoard, FakeComment, OWN_LOGIN, link as board_link};
pub use ci_filter::FakeCiFilter;
pub use design::FakeDesign;
pub use forge::{FakeForge, PrRequest};
pub use gate::FakeGate;
pub use graph_epic::add_fixture_12;
pub use releases::FakeReleases;
pub use runner::FakeRunner;
pub use vcs::FakeVcs;
pub use verb_runner::FakeVerbRunner;
