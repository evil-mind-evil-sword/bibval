pub use async_trait::async_trait;

pub mod arxiv;
pub mod crossref;
pub mod dblp;
pub mod openalex;
pub mod openlibrary;
pub mod openreview;
pub mod semantic;

use crate::entry::Entry;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ValidatorError {
    #[error("HTTP request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("Failed to parse response: {0}")]
    ParseError(String),
    #[error("Rate limited, try again later")]
    RateLimited,
    #[error("No results found")]
    NotFound,
}

/// Trait for API validators
#[async_trait]
pub trait Validator: Send + Sync {
    /// Search for an entry by DOI
    async fn search_by_doi(&self, doi: &str) -> Result<Option<Entry>, ValidatorError>;

    /// Search for an entry by title
    async fn search_by_title(&self, title: &str) -> Result<Vec<Entry>, ValidatorError>;

    /// Search for an entry by arXiv ID
    async fn search_by_arxiv_id(&self, arxiv_id: &str) -> Result<Option<Entry>, ValidatorError> {
        // Default implementation returns None - override in ArXiv client
        let _ = arxiv_id;
        Ok(None)
    }

    /// Get the name of this validator
    fn name(&self) -> &'static str;
}
