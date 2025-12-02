use serde::{Deserialize, Serialize};

/// Normalized bibliography entry for comparison across different sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Citation key from the bib file
    pub key: String,
    /// Entry type (article, inproceedings, book, etc.)
    pub entry_type: String,
    /// Paper title
    pub title: Option<String>,
    /// List of authors
    pub authors: Vec<String>,
    /// Publication year
    pub year: Option<i32>,
    /// Journal or conference venue
    pub venue: Option<String>,
    /// DOI identifier
    pub doi: Option<String>,
    /// ArXiv identifier (e.g., "2301.12345")
    pub arxiv_id: Option<String>,
    /// URL
    pub url: Option<String>,
}

impl Entry {
    pub fn new(key: String, entry_type: String) -> Self {
        Self {
            key,
            entry_type,
            title: None,
            authors: Vec::new(),
            year: None,
            venue: None,
            doi: None,
            arxiv_id: None,
            url: None,
        }
    }

    /// Normalize title for comparison (lowercase, remove extra whitespace)
    pub fn normalized_title(&self) -> Option<String> {
        self.title.as_ref().map(|t| normalize_string(t))
    }

    /// Normalize authors for comparison
    pub fn normalized_authors(&self) -> Vec<String> {
        self.authors.iter().map(|a| normalize_string(a)).collect()
    }
}

/// Normalize a string for comparison: lowercase, collapse whitespace, remove punctuation
pub fn normalize_string(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Result from an external API validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Which API this result came from
    pub source: ApiSource,
    /// The matched entry from the API
    pub matched_entry: Option<Entry>,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,
    /// List of discrepancies found
    pub discrepancies: Vec<Discrepancy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiSource {
    CrossRef,
    Dblp,
    ArXiv,
    SemanticScholar,
    OpenAlex,
    OpenLibrary,
}

impl std::fmt::Display for ApiSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiSource::CrossRef => write!(f, "CrossRef"),
            ApiSource::Dblp => write!(f, "DBLP"),
            ApiSource::ArXiv => write!(f, "ArXiv"),
            ApiSource::SemanticScholar => write!(f, "Semantic Scholar"),
            ApiSource::OpenAlex => write!(f, "OpenAlex"),
            ApiSource::OpenLibrary => write!(f, "Open Library"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Discrepancy {
    pub field: DiscrepancyField,
    pub severity: Severity,
    pub local_value: String,
    pub remote_value: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscrepancyField {
    Title,
    Authors,
    Year,
    Venue,
    Doi,
}

impl std::fmt::Display for DiscrepancyField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscrepancyField::Title => write!(f, "Title"),
            DiscrepancyField::Authors => write!(f, "Authors"),
            DiscrepancyField::Year => write!(f, "Year"),
            DiscrepancyField::Venue => write!(f, "Venue"),
            DiscrepancyField::Doi => write!(f, "DOI"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "INFO"),
            Severity::Warning => write!(f, "WARNING"),
            Severity::Error => write!(f, "ERROR"),
        }
    }
}
