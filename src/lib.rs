pub mod cache;
pub mod entry;
pub mod fusion;
pub mod matcher;
pub mod parser;
pub mod report;
pub mod validators;

use cache::Cache;
use entry::{ApiSource, Entry, Severity, ValidationResult};
use fusion::fuse_results;
use matcher::{compare_entries, find_best_match, title_similarity, years_compatible};
use report::{EntryReport, EntryStatus, Report};

/// Minimum title similarity to trust a DOI/arXiv ID lookup result
const MIN_TITLE_SIMILARITY_FOR_ID_LOOKUP: f64 = 0.75;
use validators::{
    arxiv::ArxivClient, crossref::CrossRefClient, dblp::DblpClient, openalex::OpenAlexClient,
    openlibrary::OpenLibraryClient, openreview::OpenReviewClient, semantic::SemanticScholarClient,
    Validator, ValidatorError,
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
    pub use_openlibrary: bool,
    pub use_openreview: bool,
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
            use_openlibrary: true,
            use_openreview: true,
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
    openlibrary: Option<OpenLibraryClient>,
    openreview: Option<OpenReviewClient>,
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
            openlibrary: if config.use_openlibrary {
                Some(OpenLibraryClient::new())
            } else {
                None
            },
            openreview: if config.use_openreview {
                Some(OpenReviewClient::new())
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
                        // Validate that the returned paper actually matches
                        if is_valid_id_match(entry, &result) {
                            let discrepancies = compare_entries(entry, &result);
                            let confidence = if discrepancies.is_empty() { 1.0 } else { 0.8 };
                            validation_results.push(ValidationResult {
                                source: ApiSource::CrossRef,
                                matched_entry: Some(result),
                                confidence,
                                discrepancies,
                            });
                        }
                        // If invalid match, silently skip - DOI might be wrong
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
                        if is_valid_id_match(entry, &result) {
                            let discrepancies = compare_entries(entry, &result);
                            validation_results.push(ValidationResult {
                                source: ApiSource::ArXiv,
                                matched_entry: Some(result),
                                confidence: 0.95,
                                discrepancies,
                            });
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }

            // Also try Semantic Scholar with arXiv ID
            if let Some(ref client) = self.semantic {
                match client.search_by_arxiv_id(arxiv_id).await {
                    Ok(Some(result)) => {
                        if is_valid_id_match(entry, &result) {
                            let discrepancies = compare_entries(entry, &result);
                            validation_results.push(ValidationResult {
                                source: ApiSource::SemanticScholar,
                                matched_entry: Some(result),
                                confidence: 0.9,
                                discrepancies,
                            });
                        }
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

                // Try Open Library (good for older books)
                if let Some(ref client) = self.openlibrary {
                    if let Ok(results) = client.search_by_title(title).await {
                        if let Some((matched, confidence)) = find_best_match(entry, &results) {
                            let discrepancies = compare_entries(entry, matched);
                            validation_results.push(ValidationResult {
                                source: ApiSource::OpenLibrary,
                                matched_entry: Some(matched.clone()),
                                confidence,
                                discrepancies,
                            });
                        }
                    }
                }

                // Try OpenReview (good for ML conference papers)
                if let Some(ref client) = self.openreview {
                    if let Ok(results) = client.search_by_title(title).await {
                        if let Some((matched, confidence)) = find_best_match(entry, &results) {
                            let discrepancies = compare_entries(entry, matched);
                            validation_results.push(ValidationResult {
                                source: ApiSource::OpenReview,
                                matched_entry: Some(matched.clone()),
                                confidence,
                                discrepancies,
                            });
                        }
                    }
                }
            }
        }

        // Fuse results from all validators to find consensus
        let fused = fuse_results(entry, &validation_results);

        // Determine overall status based on fused results
        let status = determine_status(&fused);

        // Create a synthetic validation result with fused discrepancies for reporting
        let fused_result = if fused.has_matches {
            vec![ValidationResult {
                source: *fused.sources.first().unwrap_or(&ApiSource::CrossRef),
                matched_entry: None,
                confidence: 1.0,
                discrepancies: fused.discrepancies,
            }]
        } else {
            validation_results
        };

        EntryReport {
            entry: entry.clone(),
            status,
            validation_results: fused_result,
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

/// Check if a matched entry from ID lookup is valid (title similar enough, year compatible)
fn is_valid_id_match(local: &Entry, remote: &Entry) -> bool {
    let title_sim = title_similarity(local, remote);
    let years_ok = years_compatible(local, remote);

    // For ID-based lookups (DOI, arXiv), we're more lenient on title
    // but still require some similarity and year compatibility
    title_sim >= MIN_TITLE_SIMILARITY_FOR_ID_LOOKUP && years_ok
}

fn determine_status(fused: &fusion::FusedResult) -> EntryStatus {
    if !fused.has_matches {
        return EntryStatus::NotFound;
    }

    let has_errors = fused
        .discrepancies
        .iter()
        .any(|d| d.severity == Severity::Error);
    let has_warnings = fused
        .discrepancies
        .iter()
        .any(|d| d.severity == Severity::Warning);

    if has_errors {
        EntryStatus::Error
    } else if has_warnings {
        EntryStatus::Warning
    } else {
        // Report which sources validated this entry
        let source = fused.sources.first().copied().unwrap_or(ApiSource::CrossRef);
        EntryStatus::Ok(source)
    }
}
