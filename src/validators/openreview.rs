use super::{async_trait, Validator, ValidatorError};
use crate::entry::Entry;
use reqwest::Client;
use serde::Deserialize;

const OPENREVIEW_API_BASE: &str = "https://api.openreview.net";

pub struct OpenReviewClient {
    client: Client,
}

impl OpenReviewClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("bibval/0.1.0 (https://github.com/femtomc/bibval)")
            .build()
            .expect("Failed to create HTTP client");
        Self { client }
    }
}

impl Default for OpenReviewClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct NotesResponse {
    notes: Option<Vec<Note>>,
}

#[derive(Debug, Deserialize)]
struct Note {
    id: Option<String>,
    content: Option<NoteContent>,
    #[serde(rename = "cdate")]
    creation_date: Option<i64>,
    venue: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NoteContent {
    title: Option<TitleField>,
    authors: Option<AuthorsField>,
    venue: Option<VenueField>,
}

// OpenReview API can return title as either a string or an object with "value"
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TitleField {
    Simple(String),
    WithValue { value: String },
}

impl TitleField {
    fn as_str(&self) -> &str {
        match self {
            TitleField::Simple(s) => s,
            TitleField::WithValue { value } => value,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AuthorsField {
    Simple(Vec<String>),
    WithValue { value: Vec<String> },
}

impl AuthorsField {
    fn as_vec(&self) -> &[String] {
        match self {
            AuthorsField::Simple(v) => v,
            AuthorsField::WithValue { value } => value,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum VenueField {
    Simple(String),
    WithValue { value: String },
}

impl VenueField {
    fn as_str(&self) -> &str {
        match self {
            VenueField::Simple(s) => s,
            VenueField::WithValue { value } => value,
        }
    }
}

impl Note {
    fn to_entry(&self) -> Entry {
        let mut entry = Entry::new(
            self.id.clone().unwrap_or_default(),
            "inproceedings".to_string(),
        );

        if let Some(content) = &self.content {
            if let Some(title) = &content.title {
                entry.title = Some(title.as_str().to_string());
            }

            if let Some(authors) = &content.authors {
                entry.authors = authors.as_vec().to_vec();
            }

            if let Some(venue) = &content.venue {
                entry.venue = Some(venue.as_str().to_string());
            }
        }

        // Use top-level venue if content venue is missing
        if entry.venue.is_none() {
            entry.venue = self.venue.clone();
        }

        // Extract year from creation date (milliseconds since epoch)
        if let Some(cdate) = self.creation_date {
            let seconds = cdate / 1000;
            // Simple year extraction: seconds since 1970
            let years_since_1970 = seconds / (365 * 24 * 60 * 60);
            entry.year = Some(1970 + years_since_1970 as i32);
        }

        entry
    }
}

#[async_trait]
impl Validator for OpenReviewClient {
    async fn search_by_doi(&self, _doi: &str) -> Result<Option<Entry>, ValidatorError> {
        // OpenReview doesn't support DOI lookup directly
        Ok(None)
    }

    async fn search_by_title(&self, title: &str) -> Result<Vec<Entry>, ValidatorError> {
        // Search using the notes endpoint with term parameter
        let url = format!(
            "{}/notes/search?query={}&limit=5&content=all",
            OPENREVIEW_API_BASE,
            urlencoding::encode(title)
        );

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ValidatorError::RateLimited);
        }

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        let response: NotesResponse = response.json().await.map_err(|e| {
            ValidatorError::ParseError(format!("Failed to parse OpenReview response: {}", e))
        })?;

        let entries = response
            .notes
            .map(|notes| notes.iter().map(|n| n.to_entry()).collect())
            .unwrap_or_default();

        Ok(entries)
    }

    fn name(&self) -> &'static str {
        "OpenReview"
    }
}
