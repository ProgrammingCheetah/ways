//! `zaf`: one project-blind binary for the verb set of the AI framework.
//!
//! Layers, inner to outer:
//! - [`domain`]: the objects (epics, tasks, asks, answers, findings, instances,
//!   profiles) and the text formats they travel in, with their parsers.
//! - [`ports`]: narrow traits written from what the verbs need.
//! - [`app`]: one use case per verb.
//! - [`adapters`]: one implementation per port and backend kind.
//! - [`fakes`]: one in-memory implementation per port, the ports' first consumer.
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
