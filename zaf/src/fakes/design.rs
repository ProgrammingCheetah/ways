//! The in-memory design source.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::domain::{PageId, PageStatus};
use crate::ports::{Design, Page, PortError};

/// A design source held in memory. Seed it with [`FakeDesign::add_page`].
#[derive(Debug, Default)]
pub struct FakeDesign {
    pages: Mutex<BTreeMap<PageId, Page>>,
}

impl FakeDesign {
    /// Adds a page, replacing one with the same id.
    pub fn add_page(&self, page: Page) {
        self.pages().insert(page.id.clone(), page);
    }

    fn pages(&self) -> MutexGuard<'_, BTreeMap<PageId, Page>> {
        self.pages.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn not_found(id: &PageId) -> PortError {
    PortError::NotFound(format!("page {id}"))
}

impl Design for FakeDesign {
    fn page(&self, id: &PageId) -> Result<Page, PortError> {
        self.pages().get(id).cloned().ok_or_else(|| not_found(id))
    }

    /// Matches what [`DirDesign`](crate::adapters::design::DirDesign) matches: pages with a line
    /// of text that holds the whole query, ignoring case, in id order. An empty query is rejected.
    fn search(&self, query: &str) -> Result<Vec<PageId>, PortError> {
        if query.trim().is_empty() {
            return Err(PortError::Rejected("an empty search query".to_owned()));
        }
        let needle = query.to_lowercase();
        let matches = self
            .pages()
            .values()
            .filter(|page| {
                page.text
                    .lines()
                    .any(|line| line.to_lowercase().contains(&needle))
            })
            .map(|page| page.id.clone())
            .collect();
        Ok(matches)
    }

    fn status(&self, id: &PageId) -> Result<PageStatus, PortError> {
        self.pages()
            .get(id)
            .map(|page| page.status)
            .ok_or_else(|| not_found(id))
    }
}

#[cfg(test)]
mod tests;
