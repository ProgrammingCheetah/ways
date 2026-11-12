//! In-memory fakes, one per port: the ports' first consumer and the ground for verb tests.
//!
//! Each fake keeps its state behind a lock, follows the port's contract
//! (idempotent repeats included), and adds a few setup and inspection methods
//! a test needs to arrange a scene and read the result.

mod board;
mod design;
mod forge;
mod gate;
mod releases;
mod runner;
mod vcs;

pub use board::{FakeBoard, link as board_link};
pub use design::FakeDesign;
pub use forge::{FakeForge, PrRequest};
pub use gate::FakeGate;
pub use releases::FakeReleases;
pub use runner::FakeRunner;
pub use vcs::FakeVcs;
