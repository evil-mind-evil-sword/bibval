use crate::entry::{ApiSource, Discrepancy, DiscrepancyField, Entry, Severity, ValidationResult};
use std::collections::HashMap;

/// Fused validation result after combining multiple validator responses
pub struct FusedResult {
    /// Sources that contributed to this result
    pub sources: Vec<ApiSource>,
    /// Consensus discrepancies (only issues that multiple validators agree on)
    pub discrepancies: Vec<Discrepancy>,
    /// Whether we found any valid matches
    pub has_matches: bool,
}

/// Fuse results from multiple validators to find consensus
pub fn fuse_results(local: &Entry, results: &[ValidationResult]) -> FusedResult {
    // Filter to only results that have a matched entry
    let valid_results: Vec<_> = results
        .iter()
        .filter(|r| r.matched_entry.is_some() && r.confidence > 0.0)
        .collect();

    if valid_results.is_empty() {
        return FusedResult {
            sources: vec![],
            discrepancies: vec![],
            has_matches: false,
        };
    }

    let sources: Vec<_> = valid_results.iter().map(|r| r.source).collect();
    let mut fused_discrepancies = Vec::new();

    // Fuse year information
    if let Some(discrepancy) = fuse_year(local, &valid_results) {
        fused_discrepancies.push(discrepancy);
    }

    // Fuse title information
    if let Some(discrepancy) = fuse_title(local, &valid_results) {
        fused_discrepancies.push(discrepancy);
    }

    // Fuse author information
    fused_discrepancies.extend(fuse_authors(local, &valid_results));

    // Check for missing DOI (any validator reporting it is enough)
    if let Some(discrepancy) = check_missing_doi(local, &valid_results) {
        fused_discrepancies.push(discrepancy);
    }

    FusedResult {
        sources,
        discrepancies: fused_discrepancies,
        has_matches: true,
    }
}

/// Fuse year information - only report if majority agrees
fn fuse_year(local: &Entry, results: &[&ValidationResult]) -> Option<Discrepancy> {
    let local_year = local.year?;

    // Collect years from all matched entries
    let mut year_counts: HashMap<i32, Vec<ApiSource>> = HashMap::new();
    for result in results {
        if let Some(ref matched) = result.matched_entry {
            if let Some(year) = matched.year {
                year_counts.entry(year).or_default().push(result.source);
            }
        }
    }

    if year_counts.is_empty() {
        return None;
    }

    // Find the most common year
    let (consensus_year, sources) = year_counts
        .iter()
        .max_by_key(|(_, sources)| sources.len())?;

    // Only report if:
    // 1. The consensus year differs from local
    // 2. At least 2 validators agree, OR it's the only validator that found something
    let min_agreement = if results.len() == 1 { 1 } else { 2 };

    if *consensus_year != local_year && sources.len() >= min_agreement {
        let source_names: Vec<_> = sources.iter().map(|s| s.to_string()).collect();
        Some(Discrepancy {
            field: DiscrepancyField::Year,
            severity: Severity::Error,
            local_value: local_year.to_string(),
            remote_value: consensus_year.to_string(),
            message: format!(
                "Year mismatch: {} vs {} (agreed by {})",
                local_year,
                consensus_year,
                source_names.join(", ")
            ),
        })
    } else {
        None
    }
}

/// Fuse title information - only report significant differences with consensus
fn fuse_title(_local: &Entry, results: &[&ValidationResult]) -> Option<Discrepancy> {
    // Count how many validators report a title discrepancy
    let title_issues: Vec<_> = results
        .iter()
        .filter_map(|r| {
            r.discrepancies
                .iter()
                .find(|d| d.field == DiscrepancyField::Title && d.severity == Severity::Error)
                .map(|d| (r.source, d))
        })
        .collect();

    // Only report if multiple validators agree there's a title problem
    // A single validator reporting title mismatch is likely a bad match - ignore it
    // Exception: if there's only one validator total AND it found a match, we trust it
    let total_with_matches = results.len();
    let min_agreement = if total_with_matches <= 1 { 1 } else { 2 };

    if title_issues.len() >= min_agreement {
        let (_source, discrepancy) = &title_issues[0];
        let sources: Vec<_> = title_issues.iter().map(|(s, _)| s.to_string()).collect();
        Some(Discrepancy {
            field: DiscrepancyField::Title,
            severity: Severity::Error,
            local_value: discrepancy.local_value.clone(),
            remote_value: discrepancy.remote_value.clone(),
            message: format!(
                "{} (confirmed by {})",
                discrepancy.message,
                sources.join(", ")
            ),
        })
    } else {
        None
    }
}

/// Fuse author information
fn fuse_authors(local: &Entry, results: &[&ValidationResult]) -> Vec<Discrepancy> {
    let mut discrepancies = Vec::new();

    // Check author count - only report if majority agrees
    let mut count_mismatches: HashMap<usize, usize> = HashMap::new();
    for result in results {
        if let Some(ref matched) = result.matched_entry {
            if !matched.authors.is_empty() && matched.authors.len() != local.authors.len() {
                *count_mismatches.entry(matched.authors.len()).or_default() += 1;
            }
        }
    }

    // Report author count mismatch if at least 2 validators agree
    if let Some((remote_count, agreement)) = count_mismatches.iter().max_by_key(|(_, v)| *v) {
        if *agreement >= 2 || (results.len() == 1 && *agreement == 1) {
            discrepancies.push(Discrepancy {
                field: DiscrepancyField::Authors,
                severity: Severity::Warning,
                local_value: format!("{} authors", local.authors.len()),
                remote_value: format!("{} authors", remote_count),
                message: format!(
                    "Author count differs: {} (local) vs {} (remote)",
                    local.authors.len(),
                    remote_count
                ),
            });
        }
    }

    discrepancies
}

/// Check if DOI is missing locally but present in any remote entry
fn check_missing_doi(local: &Entry, results: &[&ValidationResult]) -> Option<Discrepancy> {
    if local.doi.is_some() {
        return None;
    }

    // Find any remote DOI
    for result in results {
        if let Some(ref matched) = result.matched_entry {
            if let Some(ref doi) = matched.doi {
                return Some(Discrepancy {
                    field: DiscrepancyField::Doi,
                    severity: Severity::Warning,
                    local_value: "(none)".to_string(),
                    remote_value: doi.clone(),
                    message: "Missing DOI in local entry".to_string(),
                });
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(source: ApiSource, year: Option<i32>) -> ValidationResult {
        let mut entry = Entry::new("test".to_string(), "article".to_string());
        entry.year = year;
        ValidationResult {
            source,
            matched_entry: Some(entry),
            confidence: 0.9,
            discrepancies: vec![],
        }
    }

    #[test]
    fn test_year_consensus() {
        let mut local = Entry::new("test".to_string(), "article".to_string());
        local.year = Some(2020);

        let results = vec![
            make_result(ApiSource::CrossRef, Some(2019)),
            make_result(ApiSource::Dblp, Some(2019)),
            make_result(ApiSource::SemanticScholar, Some(2020)),
        ];

        let refs: Vec<_> = results.iter().collect();
        let discrepancy = fuse_year(&local, &refs);

        // 2 validators say 2019, 1 says 2020 - should report 2019 as consensus
        assert!(discrepancy.is_some());
        let d = discrepancy.unwrap();
        assert_eq!(d.remote_value, "2019");
    }

    #[test]
    fn test_no_consensus_no_error() {
        let mut local = Entry::new("test".to_string(), "article".to_string());
        local.year = Some(2020);

        let results = vec![
            make_result(ApiSource::CrossRef, Some(2019)),
            make_result(ApiSource::Dblp, Some(2018)),
            make_result(ApiSource::SemanticScholar, Some(2020)),
        ];

        let refs: Vec<_> = results.iter().collect();
        let discrepancy = fuse_year(&local, &refs);

        // No consensus (all different years) - shouldn't report error
        assert!(discrepancy.is_none());
    }
}
