use super::{async_trait, Validator, ValidatorError};
use crate::entry::Entry;
use reqwest::Client;
use serde::Deserialize;

const SEMANTIC_SCHOLAR_API_BASE: &str = "https://api.semanticscholar.org/graph/v1";

pub struct SemanticScholarClient {
    client: Client,
}

impl SemanticScholarClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("bibval/0.1.0 (https://github.com/femtomc/bibval)")
            .build()
            .expect("Failed to create HTTP client");
        Self { client }
    }
}

impl Default for SemanticScholarClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Option<Vec<Paper>>,
}

#[derive(Debug, Deserialize)]
struct Paper {
    #[serde(rename = "paperId")]
    paper_id: Option<String>,
    title: Option<String>,
    authors: Option<Vec<Author>>,
    year: Option<i32>,
    venue: Option<String>,
    #[serde(rename = "externalIds")]
    external_ids: Option<ExternalIds>,
}

#[derive(Debug, Deserialize)]
struct Author {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExternalIds {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    #[serde(rename = "ArXiv")]
    arxiv: Option<String>,
}

impl Paper {
    fn to_entry(&self) -> Entry {
        let mut entry = Entry::new(
            self.paper_id.clone().unwrap_or_default(),
            "article".to_string(),
        );

        entry.title = self.title.clone();
        entry.year = self.year;
        entry.venue = self.venue.clone();

        if let Some(authors) = &self.authors {
            entry.authors = authors
                .iter()
                .filter_map(|a| a.name.clone())
                .collect();
        }

        if let Some(ids) = &self.external_ids {
            entry.doi = ids.doi.clone();
            entry.arxiv_id = ids.arxiv.clone();
        }

        entry
    }
}

#[async_trait]
impl Validator for SemanticScholarClient {
    async fn search_by_doi(&self, doi: &str) -> Result<Option<Entry>, ValidatorError> {
        let url = format!(
            "{}/paper/DOI:{}?fields=title,authors,year,venue,externalIds",
            SEMANTIC_SCHOLAR_API_BASE, doi
        );

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ValidatorError::RateLimited);
        }

        let paper: Paper = response.json().await.map_err(|e| {
            ValidatorError::ParseError(format!("Failed to parse Semantic Scholar response: {}", e))
        })?;

        Ok(Some(paper.to_entry()))
    }

    async fn search_by_title(&self, title: &str) -> Result<Vec<Entry>, ValidatorError> {
        let url = format!(
            "{}/paper/search?query={}&fields=title,authors,year,venue,externalIds&limit=5",
            SEMANTIC_SCHOLAR_API_BASE,
            urlencoding::encode(title)
        );

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ValidatorError::RateLimited);
        }

        let response: SearchResponse = response.json().await.map_err(|e| {
            ValidatorError::ParseError(format!("Failed to parse Semantic Scholar response: {}", e))
        })?;

        let entries = response
            .data
            .map(|papers| papers.iter().map(|p| p.to_entry()).collect())
            .unwrap_or_default();

        Ok(entries)
    }

    async fn search_by_arxiv_id(&self, arxiv_id: &str) -> Result<Option<Entry>, ValidatorError> {
        let url = format!(
            "{}/paper/ARXIV:{}?fields=title,authors,year,venue,externalIds",
            SEMANTIC_SCHOLAR_API_BASE, arxiv_id
        );

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(ValidatorError::RateLimited);
        }

        let paper: Paper = response.json().await.map_err(|e| {
            ValidatorError::ParseError(format!("Failed to parse Semantic Scholar response: {}", e))
        })?;

        Ok(Some(paper.to_entry()))
    }

    fn name(&self) -> &'static str {
        "Semantic Scholar"
    }
}
