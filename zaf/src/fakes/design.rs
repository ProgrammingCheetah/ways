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

    /// Matches pages whose title or text holds every word of the query, ignoring case.
    fn search(&self, query: &str) -> Result<Vec<PageId>, PortError> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let matches = self
            .pages()
            .values()
            .filter(|page| {
                let haystack = format!("{} {}", page.title, page.text).to_lowercase();
                words.iter().all(|word| haystack.contains(word.as_str()))
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
