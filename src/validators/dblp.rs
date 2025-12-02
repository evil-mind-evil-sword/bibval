use super::{async_trait, Validator, ValidatorError};
use crate::entry::Entry;
use reqwest::Client;
use serde::Deserialize;

const DBLP_API_BASE: &str = "https://dblp.org/search/publ/api";

pub struct DblpClient {
    client: Client,
}

impl DblpClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("bibval/0.1.0 (https://github.com/femtomc/bibval)")
            .build()
            .expect("Failed to create HTTP client");
        Self { client }
    }
}

impl Default for DblpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct DblpResponse {
    result: DblpResult,
}

#[derive(Debug, Deserialize)]
struct DblpResult {
    hits: Option<DblpHits>,
}

#[derive(Debug, Deserialize)]
struct DblpHits {
    hit: Option<Vec<DblpHit>>,
}

#[derive(Debug, Deserialize)]
struct DblpHit {
    info: DblpInfo,
}

#[derive(Debug, Deserialize)]
struct DblpInfo {
    title: Option<String>,
    authors: Option<DblpAuthors>,
    year: Option<String>,
    venue: Option<String>,
    doi: Option<String>,
    #[serde(rename = "type")]
    pub_type: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DblpAuthors {
    author: DblpAuthorList,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DblpAuthorList {
    Single(DblpAuthor),
    Multiple(Vec<DblpAuthor>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum DblpAuthor {
    Simple(String),
    Complex { text: String },
}

impl DblpAuthor {
    fn name(&self) -> &str {
        match self {
            DblpAuthor::Simple(s) => s,
            DblpAuthor::Complex { text } => text,
        }
    }
}

impl DblpInfo {
    fn to_entry(&self) -> Entry {
        let mut entry = Entry::new(
            self.doi.clone().unwrap_or_else(|| {
                self.url
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string())
            }),
            self.pub_type.clone().unwrap_or_else(|| "article".to_string()),
        );

        entry.title = self.title.clone().map(|t| t.trim_end_matches('.').to_string());
        entry.doi = self.doi.clone();
        entry.year = self.year.as_ref().and_then(|y| y.parse().ok());
        entry.venue = self.venue.clone();

        if let Some(authors) = &self.authors {
            entry.authors = match &authors.author {
                DblpAuthorList::Single(a) => vec![a.name().to_string()],
                DblpAuthorList::Multiple(list) => list.iter().map(|a| a.name().to_string()).collect(),
            };
        }

        entry
    }
}

#[async_trait]
impl Validator for DblpClient {
    async fn search_by_doi(&self, doi: &str) -> Result<Option<Entry>, ValidatorError> {
        // DBLP doesn't have direct DOI lookup, so we search for it
        let results = self.search_by_title(doi).await?;
        Ok(results.into_iter().find(|e| e.doi.as_deref() == Some(doi)))
    }

    async fn search_by_title(&self, title: &str) -> Result<Vec<Entry>, ValidatorError> {
        let url = format!(
            "{}?q={}&format=json&h=5",
            DBLP_API_BASE,
            urlencoding::encode(title)
        );

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ValidatorError::RateLimited);
        }

        let response: DblpResponse = response.json().await.map_err(|e| {
            ValidatorError::ParseError(format!("Failed to parse DBLP response: {}", e))
        })?;

        let entries = response
            .result
            .hits
            .and_then(|h| h.hit)
            .map(|hits| hits.iter().map(|h| h.info.to_entry()).collect())
            .unwrap_or_default();

        Ok(entries)
    }

    fn name(&self) -> &'static str {
        "DBLP"
    }
}
