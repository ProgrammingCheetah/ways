//! `zaf`: one project-blind binary for the verb set of the AI framework.
//!
//! Layers, inner to outer:
//! - [`domain`]: the objects (epics, tasks, asks, answers, findings, instances,
//!   profiles) and the text formats they travel in, with their parsers.
//! - [`ports`]: narrow traits written from what the verbs need: [`ports::Board`],
//!   [`ports::Vcs`], [`ports::Forge`], [`ports::Design`], [`ports::Gate`] and
//!   [`ports::Runner`].
//! - [`app`]: one use case per verb, seeing only ports.
//! - [`adapters`]: the live implementations: a directory design source
//!   ([`adapters::design`]), a shell gate ([`adapters::gate`]), the board and the
//!   forge on GitHub through `gh` ([`adapters::github`]), a VCS on `jj`
//!   ([`adapters::jj`]) and a process runner ([`adapters::runner`]).
//! - [`fakes`]: one in-memory implementation per port, the ports' first consumer
//!   and the ground for verb tests.
//! - [`manifest`]: `ZAF.toml`, everything zaf knows about a project.
//! - [`composition`]: reads the manifest and picks the adapters, the only place they are named.
//! - [`cli`]: the command line, every verb listed.

pub mod adapters;
pub mod app;
pub mod cli;
pub mod composition;
pub mod domain;
pub mod fakes;
pub mod manifest;
pub mod ports;

mod newtype;
