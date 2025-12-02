use super::{async_trait, Validator, ValidatorError};
use crate::entry::Entry;
use reqwest::Client;
use serde::Deserialize;

const OPENLIBRARY_API_BASE: &str = "https://openlibrary.org";

pub struct OpenLibraryClient {
    client: Client,
}

impl OpenLibraryClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("bibval/0.1.0 (https://github.com/femtomc/bibval)")
            .build()
            .expect("Failed to create HTTP client");
        Self { client }
    }

    /// Search by ISBN
    pub async fn search_by_isbn(&self, isbn: &str) -> Result<Option<Entry>, ValidatorError> {
        // Clean ISBN (remove hyphens)
        let clean_isbn: String = isbn.chars().filter(|c| c.is_alphanumeric()).collect();

        let url = format!(
            "{}/isbn/{}.json",
            OPENLIBRARY_API_BASE, clean_isbn
        );

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Ok(None);
        }

        let book: BookEdition = response.json().await.map_err(|e| {
            ValidatorError::ParseError(format!("Failed to parse Open Library response: {}", e))
        })?;

        // Get additional details from the works endpoint if available
        let mut entry = book.to_entry();

        if let Some(works) = &book.works {
            if let Some(work_ref) = works.first() {
                if let Ok(Some(work_entry)) = self.get_work_details(&work_ref.key).await {
                    // Merge work details (work has better title/author info sometimes)
                    if entry.title.is_none() {
                        entry.title = work_entry.title;
                    }
                    if entry.authors.is_empty() {
                        entry.authors = work_entry.authors;
                    }
                }
            }
        }

        Ok(Some(entry))
    }

    async fn get_work_details(&self, work_key: &str) -> Result<Option<Entry>, ValidatorError> {
        let url = format!("{}{}.json", OPENLIBRARY_API_BASE, work_key);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let work: Work = response.json().await.map_err(|e| {
            ValidatorError::ParseError(format!("Failed to parse Open Library work: {}", e))
        })?;

        Ok(Some(work.to_entry()))
    }
}

impl Default for OpenLibraryClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    docs: Vec<SearchDoc>,
}

#[derive(Debug, Deserialize)]
struct SearchDoc {
    key: Option<String>,
    title: Option<String>,
    author_name: Option<Vec<String>>,
    first_publish_year: Option<i32>,
    publisher: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct BookEdition {
    title: Option<String>,
    publish_date: Option<String>,
    publishers: Option<Vec<String>>,
    works: Option<Vec<WorkRef>>,
}

#[derive(Debug, Deserialize)]
struct WorkRef {
    key: String,
}

#[derive(Debug, Deserialize)]
struct Work {
    title: Option<String>,
}

impl SearchDoc {
    fn to_entry(&self) -> Entry {
        let mut entry = Entry::new(
            self.key.clone().unwrap_or_default(),
            "book".to_string(),
        );

        entry.title = self.title.clone();
        entry.year = self.first_publish_year;

        if let Some(authors) = &self.author_name {
            entry.authors = authors.clone();
        }

        if let Some(publishers) = &self.publisher {
            entry.venue = publishers.first().cloned();
        }

        entry
    }
}

impl BookEdition {
    fn to_entry(&self) -> Entry {
        let mut entry = Entry::new(String::new(), "book".to_string());

        entry.title = self.title.clone();

        // Parse year from publish_date (e.g., "1996", "January 1, 1996", etc.)
        if let Some(date) = &self.publish_date {
            entry.year = extract_year(date);
        }

        if let Some(publishers) = &self.publishers {
            entry.venue = publishers.first().cloned();
        }

        entry
    }
}

impl Work {
    fn to_entry(&self) -> Entry {
        let mut entry = Entry::new(String::new(), "book".to_string());
        entry.title = self.title.clone();
        entry
    }
}

/// Extract a 4-digit year from a date string
fn extract_year(date: &str) -> Option<i32> {
    // Look for a 4-digit number that looks like a year (1800-2099)
    let year_regex = regex_lite::Regex::new(r"\b(1[89]\d{2}|20\d{2})\b").ok()?;
    year_regex
        .find(date)
        .and_then(|m| m.as_str().parse().ok())
}

#[async_trait]
impl Validator for OpenLibraryClient {
    async fn search_by_doi(&self, _doi: &str) -> Result<Option<Entry>, ValidatorError> {
        // Open Library doesn't support DOI lookup
        Ok(None)
    }

    async fn search_by_title(&self, title: &str) -> Result<Vec<Entry>, ValidatorError> {
        let url = format!(
            "{}/search.json?q={}&limit=5&fields=key,title,author_name,first_publish_year,isbn,publisher",
            OPENLIBRARY_API_BASE,
            urlencoding::encode(title)
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        let response: SearchResponse = response.json().await.map_err(|e| {
            ValidatorError::ParseError(format!("Failed to parse Open Library response: {}", e))
        })?;

        let entries = response.docs.iter().map(|d| d.to_entry()).collect();

        Ok(entries)
    }

    fn name(&self) -> &'static str {
        "Open Library"
    }
}
