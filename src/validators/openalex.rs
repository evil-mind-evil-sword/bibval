use super::{async_trait, Validator, ValidatorError};
use crate::entry::Entry;
use reqwest::Client;
use serde::Deserialize;

const OPENALEX_API_BASE: &str = "https://api.openalex.org";

pub struct OpenAlexClient {
    client: Client,
}

impl OpenAlexClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("bibval/0.1.0 (https://github.com/femtomc/bibval)")
            .build()
            .expect("Failed to create HTTP client");
        Self { client }
    }
}

impl Default for OpenAlexClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    results: Vec<Work>,
}

#[derive(Debug, Deserialize)]
struct Work {
    id: Option<String>,
    title: Option<String>,
    #[serde(rename = "authorships")]
    authorships: Option<Vec<Authorship>>,
    #[serde(rename = "publication_year")]
    publication_year: Option<i32>,
    #[serde(rename = "primary_location")]
    primary_location: Option<Location>,
    doi: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Authorship {
    author: Option<Author>,
}

#[derive(Debug, Deserialize)]
struct Author {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Location {
    source: Option<Source>,
}

#[derive(Debug, Deserialize)]
struct Source {
    display_name: Option<String>,
}

impl Work {
    fn to_entry(&self) -> Entry {
        let mut entry = Entry::new(
            self.id.clone().unwrap_or_default(),
            "article".to_string(),
        );

        entry.title = self.title.clone();
        entry.year = self.publication_year;

        // Extract venue from primary location
        if let Some(loc) = &self.primary_location {
            if let Some(source) = &loc.source {
                entry.venue = source.display_name.clone();
            }
        }

        // Extract authors
        if let Some(authorships) = &self.authorships {
            entry.authors = authorships
                .iter()
                .filter_map(|a| a.author.as_ref()?.display_name.clone())
                .collect();
        }

        // Clean up DOI (OpenAlex returns full URL)
        if let Some(doi) = &self.doi {
            entry.doi = Some(doi.replace("https://doi.org/", ""));
        }

        entry
    }
}

#[async_trait]
impl Validator for OpenAlexClient {
    async fn search_by_doi(&self, doi: &str) -> Result<Option<Entry>, ValidatorError> {
        let url = format!("{}/works/doi:{}", OPENALEX_API_BASE, doi);

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ValidatorError::RateLimited);
        }

        if !response.status().is_success() {
            return Ok(None);
        }

        let work: Work = response.json().await.map_err(|e| {
            ValidatorError::ParseError(format!("Failed to parse OpenAlex response: {}", e))
        })?;

        Ok(Some(work.to_entry()))
    }

    async fn search_by_title(&self, title: &str) -> Result<Vec<Entry>, ValidatorError> {
        let url = format!(
            "{}/works?search={}&per_page=5",
            OPENALEX_API_BASE,
            urlencoding::encode(title)
        );

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ValidatorError::RateLimited);
        }

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        let response: SearchResponse = response.json().await.map_err(|e| {
            ValidatorError::ParseError(format!("Failed to parse OpenAlex response: {}", e))
        })?;

        let entries = response.results.iter().map(|w| w.to_entry()).collect();

        Ok(entries)
    }

    fn name(&self) -> &'static str {
        "OpenAlex"
    }
}
