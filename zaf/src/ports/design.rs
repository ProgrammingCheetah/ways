//! The design source: pages by id, search and index status.
//!
//! A project may declare no design source; the board's epics and rulings are then the design.

use super::PortError;
use crate::domain::{PageId, PageStatus};

/// A design page as the source holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// Its id.
    pub id: PageId,
    /// Its title.
    pub title: String,
    /// Its index status.
    pub status: PageStatus,
    /// Its text, the ground every quote is checked against.
    pub text: String,
}

/// The project's design source.
pub trait Design {
    /// One page by id.
    fn page(&self, id: &PageId) -> Result<Page, PortError>;

    /// The ids of pages matching a query, in id order.
    fn search(&self, query: &str) -> Result<Vec<PageId>, PortError>;

    /// A page's index status, without its text.
    fn status(&self, id: &PageId) -> Result<PageStatus, PortError>;
}

#[cfg(test)]
mod tests;
