use super::{async_trait, Validator, ValidatorError};
use crate::entry::Entry;
use reqwest::Client;
use serde::Deserialize;

const CROSSREF_API_BASE: &str = "https://api.crossref.org/works";
const USER_AGENT: &str = "bibval/0.1.0 (https://github.com/femtomc/bibval; mailto:user@example.com)";

pub struct CrossRefClient {
    client: Client,
}

impl CrossRefClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .expect("Failed to create HTTP client");
        Self { client }
    }
}

impl Default for CrossRefClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct CrossRefResponse {
    status: String,
    message: CrossRefMessage,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CrossRefMessage {
    Single(CrossRefWork),
    Search(CrossRefSearchResult),
}

#[derive(Debug, Deserialize)]
struct CrossRefSearchResult {
    items: Vec<CrossRefWork>,
}

#[derive(Debug, Deserialize)]
struct CrossRefWork {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    title: Option<Vec<String>>,
    author: Option<Vec<CrossRefAuthor>>,
    #[serde(rename = "container-title")]
    container_title: Option<Vec<String>>,
    published: Option<CrossRefDate>,
    #[serde(rename = "published-print")]
    published_print: Option<CrossRefDate>,
    #[serde(rename = "published-online")]
    published_online: Option<CrossRefDate>,
    #[serde(rename = "type")]
    work_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrossRefAuthor {
    given: Option<String>,
    family: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrossRefDate {
    #[serde(rename = "date-parts")]
    date_parts: Option<Vec<Vec<i32>>>,
}

impl CrossRefWork {
    fn to_entry(&self) -> Entry {
        let mut entry = Entry::new(
            self.doi.clone().unwrap_or_default(),
            self.work_type.clone().unwrap_or_else(|| "article".to_string()),
        );

        entry.title = self.title.as_ref().and_then(|t| t.first().cloned());
        entry.doi = self.doi.clone();

        if let Some(authors) = &self.author {
            entry.authors = authors
                .iter()
                .map(|a| {
                    if let Some(name) = &a.name {
                        name.clone()
                    } else {
                        let given = a.given.as_deref().unwrap_or("");
                        let family = a.family.as_deref().unwrap_or("");
                        format!("{} {}", given, family).trim().to_string()
                    }
                })
                .collect();
        }

        entry.venue = self
            .container_title
            .as_ref()
            .and_then(|t| t.first().cloned());

        // Try different date fields
        let date = self
            .published
            .as_ref()
            .or(self.published_print.as_ref())
            .or(self.published_online.as_ref());

        if let Some(d) = date {
            if let Some(parts) = &d.date_parts {
                if let Some(first) = parts.first() {
                    if let Some(&year) = first.first() {
                        entry.year = Some(year);
                    }
                }
            }
        }

        entry
    }
}

#[async_trait]
impl Validator for CrossRefClient {
    async fn search_by_doi(&self, doi: &str) -> Result<Option<Entry>, ValidatorError> {
        let url = format!("{}/{}", CROSSREF_API_BASE, doi);

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ValidatorError::RateLimited);
        }

        let response: CrossRefResponse = response.json().await?;

        if response.status != "ok" {
            return Ok(None);
        }

        match response.message {
            CrossRefMessage::Single(work) => Ok(Some(work.to_entry())),
            CrossRefMessage::Search(_) => Ok(None),
        }
    }

    async fn search_by_title(&self, title: &str) -> Result<Vec<Entry>, ValidatorError> {
        let url = format!(
            "{}?query.title={}&rows=5",
            CROSSREF_API_BASE,
            urlencoding::encode(title)
        );

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ValidatorError::RateLimited);
        }

        let response: CrossRefResponse = response.json().await?;

        if response.status != "ok" {
            return Ok(Vec::new());
        }

        match response.message {
            CrossRefMessage::Search(result) => {
                Ok(result.items.iter().map(|w| w.to_entry()).collect())
            }
            CrossRefMessage::Single(work) => Ok(vec![work.to_entry()]),
        }
    }

    fn name(&self) -> &'static str {
        "CrossRef"
    }
}
