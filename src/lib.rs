pub mod cache;
pub mod entry;
pub mod matcher;
pub mod parser;
pub mod report;
pub mod validators;

use cache::Cache;
use entry::{ApiSource, Entry, ValidationResult};
use matcher::{compare_entries, find_best_match};
use report::{EntryReport, EntryStatus, Report};
use validators::{
    arxiv::ArxivClient, crossref::CrossRefClient, dblp::DblpClient,
    openalex::OpenAlexClient, semantic::SemanticScholarClient, Validator, ValidatorError,
};

use futures::{stream, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Configuration for the validator
pub struct ValidatorConfig {
    pub use_crossref: bool,
    pub use_dblp: bool,
    pub use_arxiv: bool,
    pub use_semantic: bool,
    pub use_openalex: bool,
    pub cache_enabled: bool,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            use_crossref: true,
            use_dblp: true,
            use_arxiv: true,
            use_semantic: true,
            use_openalex: true,
            cache_enabled: true,
        }
    }
}

/// Main validator that coordinates all API clients
pub struct BibValidator {
    crossref: Option<CrossRefClient>,
    dblp: Option<DblpClient>,
    arxiv: Option<ArxivClient>,
    semantic: Option<SemanticScholarClient>,
    openalex: Option<OpenAlexClient>,
    cache: Cache,
}

impl BibValidator {
    pub fn new(config: ValidatorConfig) -> Result<Self, cache::CacheError> {
        let cache = Cache::new(config.cache_enabled)?;

        Ok(Self {
            crossref: if config.use_crossref {
                Some(CrossRefClient::new())
            } else {
                None
            },
            dblp: if config.use_dblp {
                Some(DblpClient::new())
            } else {
                None
            },
            arxiv: if config.use_arxiv {
                Some(ArxivClient::new())
            } else {
                None
            },
            semantic: if config.use_semantic {
                Some(SemanticScholarClient::new())
            } else {
                None
            },
            openalex: if config.use_openalex {
                Some(OpenAlexClient::new())
            } else {
                None
            },
            cache,
        })
    }

    /// Validate a list of entries and return a report
    pub async fn validate(&self, entries: Vec<Entry>) -> Report {
        const CONCURRENCY_LIMIT: usize = 20;

        let total = entries.len() as u64;
        let progress = Arc::new(AtomicU64::new(0));

        let pb = ProgressBar::new(total);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );

        let results: Vec<EntryReport> = stream::iter(entries)
            .map(|entry| {
                let progress = Arc::clone(&progress);
                async move {
                    let report = self.validate_entry(&entry).await;
                    progress.fetch_add(1, Ordering::Relaxed);
                    report
                }
            })
            .buffer_unordered(CONCURRENCY_LIMIT)
            .inspect(|_| pb.inc(1))
            .collect()
            .await;

        pb.finish_with_message("Done!");

        let mut report = Report::new();
        for entry_report in results {
            report.add(entry_report);
        }
        report
    }

    /// Validate a single entry against all configured APIs
    async fn validate_entry(&self, entry: &Entry) -> EntryReport {
        let mut validation_results = Vec::new();

        // Try DOI-based lookup first (most reliable)
        if let Some(doi) = &entry.doi {
            if let Some(ref client) = self.crossref {
                match self.try_doi_lookup(client, doi).await {
                    Ok(Some(result)) => {
                        let discrepancies = compare_entries(entry, &result);
                        let confidence = if discrepancies.is_empty() { 1.0 } else { 0.8 };
                        validation_results.push(ValidationResult {
                            source: ApiSource::CrossRef,
                            matched_entry: Some(result),
                            confidence,
                            discrepancies,
                        });
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }
        }

        // Try arXiv ID lookup
        if let Some(arxiv_id) = &entry.arxiv_id {
            if let Some(ref client) = self.arxiv {
                match client.search_by_arxiv_id(arxiv_id).await {
                    Ok(Some(result)) => {
                        let discrepancies = compare_entries(entry, &result);
                        validation_results.push(ValidationResult {
                            source: ApiSource::ArXiv,
                            matched_entry: Some(result),
                            confidence: 0.95,
                            discrepancies,
                        });
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }

            // Also try Semantic Scholar with arXiv ID
            if let Some(ref client) = self.semantic {
                match client.search_by_arxiv_id(arxiv_id).await {
                    Ok(Some(result)) => {
                        let discrepancies = compare_entries(entry, &result);
                        validation_results.push(ValidationResult {
                            source: ApiSource::SemanticScholar,
                            matched_entry: Some(result),
                            confidence: 0.9,
                            discrepancies,
                        });
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }
        }

        // If no exact matches, try title search
        if validation_results.is_empty() {
            if let Some(title) = &entry.title {
                // Try DBLP
                if let Some(ref client) = self.dblp {
                    if let Ok(results) = client.search_by_title(title).await {
                        if let Some((matched, confidence)) = find_best_match(entry, &results) {
                            let discrepancies = compare_entries(entry, matched);
                            validation_results.push(ValidationResult {
                                source: ApiSource::Dblp,
                                matched_entry: Some(matched.clone()),
                                confidence,
                                discrepancies,
                            });
                        }
                    }
                }

                // Try Semantic Scholar
                if let Some(ref client) = self.semantic {
                    if let Ok(results) = client.search_by_title(title).await {
                        if let Some((matched, confidence)) = find_best_match(entry, &results) {
                            let discrepancies = compare_entries(entry, matched);
                            validation_results.push(ValidationResult {
                                source: ApiSource::SemanticScholar,
                                matched_entry: Some(matched.clone()),
                                confidence,
                                discrepancies,
                            });
                        }
                    }
                }

                // Try OpenAlex
                if let Some(ref client) = self.openalex {
                    if let Ok(results) = client.search_by_title(title).await {
                        if let Some((matched, confidence)) = find_best_match(entry, &results) {
                            let discrepancies = compare_entries(entry, matched);
                            validation_results.push(ValidationResult {
                                source: ApiSource::OpenAlex,
                                matched_entry: Some(matched.clone()),
                                confidence,
                                discrepancies,
                            });
                        }
                    }
                }
            }
        }

        // Determine overall status
        let status = determine_status(&validation_results);

        EntryReport {
            entry: entry.clone(),
            status,
            validation_results,
        }
    }

    async fn try_doi_lookup(
        &self,
        client: &CrossRefClient,
        doi: &str,
    ) -> Result<Option<Entry>, ValidatorError> {
        // Check cache first
        if let Some(cached) = self.cache.get::<Entry>("crossref_doi", doi) {
            return Ok(Some(cached));
        }

        let result = client.search_by_doi(doi).await?;

        // Cache the result
        if let Some(ref entry) = result {
            let _ = self.cache.set("crossref_doi", doi, entry);
        }

        Ok(result)
    }
}

fn determine_status(results: &[ValidationResult]) -> EntryStatus {
    if results.is_empty() {
        return EntryStatus::NotFound;
    }

    // Find the result with highest confidence
    let best = results
        .iter()
        .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap());

    if let Some(result) = best {
        let has_errors = result
            .discrepancies
            .iter()
            .any(|d| d.severity == entry::Severity::Error);
        let has_warnings = result
            .discrepancies
            .iter()
            .any(|d| d.severity == entry::Severity::Warning);

        if has_errors {
            EntryStatus::Error
        } else if has_warnings {
            EntryStatus::Warning
        } else {
            EntryStatus::Ok(result.source)
        }
    } else {
        EntryStatus::NotFound
    }
}
