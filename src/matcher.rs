use crate::entry::{normalize_string, Discrepancy, DiscrepancyField, Entry, Severity};
use strsim::jaro_winkler;

/// Threshold for title similarity (0.0 to 1.0)
const TITLE_MATCH_THRESHOLD: f64 = 0.85;
const TITLE_WARNING_THRESHOLD: f64 = 0.90;

/// Threshold for author name similarity
const AUTHOR_MATCH_THRESHOLD: f64 = 0.80;

/// Maximum year difference for a valid match
const MAX_YEAR_DIFFERENCE: i32 = 2;

/// Minimum author overlap ratio for a valid match
const MIN_AUTHOR_OVERLAP: f64 = 0.3;

/// Compare two entries and return a list of discrepancies
pub fn compare_entries(local: &Entry, remote: &Entry) -> Vec<Discrepancy> {
    let mut discrepancies = Vec::new();

    // Compare titles
    if let (Some(local_title), Some(remote_title)) = (&local.title, &remote.title) {
        let local_norm = normalize_string(local_title);
        let remote_norm = normalize_string(remote_title);

        let similarity = jaro_winkler(&local_norm, &remote_norm);

        if similarity < TITLE_MATCH_THRESHOLD {
            discrepancies.push(Discrepancy {
                field: DiscrepancyField::Title,
                severity: Severity::Error,
                local_value: local_title.clone(),
                remote_value: remote_title.clone(),
                message: format!(
                    "Title significantly different (similarity: {:.0}%)",
                    similarity * 100.0
                ),
            });
        } else if similarity < TITLE_WARNING_THRESHOLD {
            discrepancies.push(Discrepancy {
                field: DiscrepancyField::Title,
                severity: Severity::Warning,
                local_value: local_title.clone(),
                remote_value: remote_title.clone(),
                message: format!(
                    "Title slightly different (similarity: {:.0}%)",
                    similarity * 100.0
                ),
            });
        }
    }

    // Compare years
    if let (Some(local_year), Some(remote_year)) = (local.year, remote.year) {
        if local_year != remote_year {
            discrepancies.push(Discrepancy {
                field: DiscrepancyField::Year,
                severity: Severity::Error,
                local_value: local_year.to_string(),
                remote_value: remote_year.to_string(),
                message: format!("Year mismatch: {} vs {}", local_year, remote_year),
            });
        }
    }

    // Compare authors
    let author_issues = compare_authors(&local.authors, &remote.authors);
    discrepancies.extend(author_issues);

    // Check for missing DOI
    if local.doi.is_none() && remote.doi.is_some() {
        discrepancies.push(Discrepancy {
            field: DiscrepancyField::Doi,
            severity: Severity::Warning,
            local_value: "(none)".to_string(),
            remote_value: remote.doi.clone().unwrap(),
            message: "Missing DOI in local entry".to_string(),
        });
    }

    // Compare venues
    if let (Some(local_venue), Some(remote_venue)) = (&local.venue, &remote.venue) {
        let local_norm = normalize_string(local_venue);
        let remote_norm = normalize_string(remote_venue);

        let similarity = jaro_winkler(&local_norm, &remote_norm);

        if similarity < 0.70 {
            discrepancies.push(Discrepancy {
                field: DiscrepancyField::Venue,
                severity: Severity::Info,
                local_value: local_venue.clone(),
                remote_value: remote_venue.clone(),
                message: "Venue name differs".to_string(),
            });
        }
    }

    discrepancies
}

/// Compare author lists and return discrepancies
fn compare_authors(local: &[String], remote: &[String]) -> Vec<Discrepancy> {
    let mut discrepancies = Vec::new();

    if local.is_empty() || remote.is_empty() {
        return discrepancies;
    }

    // Check author count
    if local.len() != remote.len() {
        discrepancies.push(Discrepancy {
            field: DiscrepancyField::Authors,
            severity: Severity::Warning,
            local_value: format!("{} authors", local.len()),
            remote_value: format!("{} authors", remote.len()),
            message: format!(
                "Author count differs: {} (local) vs {} (remote)",
                local.len(),
                remote.len()
            ),
        });
    }

    // Check each local author against remote authors
    for local_author in local {
        let local_norm = normalize_string(local_author);
        let best_match = remote
            .iter()
            .map(|r| {
                let remote_norm = normalize_string(r);
                (r, jaro_winkler(&local_norm, &remote_norm))
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        if let Some((remote_author, similarity)) = best_match {
            if similarity < AUTHOR_MATCH_THRESHOLD {
                discrepancies.push(Discrepancy {
                    field: DiscrepancyField::Authors,
                    severity: Severity::Warning,
                    local_value: local_author.clone(),
                    remote_value: remote_author.clone(),
                    message: format!(
                        "Author name spelling may differ: '{}' vs '{}'",
                        local_author, remote_author
                    ),
                });
            }
        }
    }

    discrepancies
}

/// Calculate title similarity between two entries
pub fn title_similarity(a: &Entry, b: &Entry) -> f64 {
    match (&a.title, &b.title) {
        (Some(title_a), Some(title_b)) => {
            let norm_a = normalize_string(title_a);
            let norm_b = normalize_string(title_b);
            jaro_winkler(&norm_a, &norm_b)
        }
        _ => 0.0,
    }
}

/// Check if years are within acceptable range
pub fn years_compatible(a: &Entry, b: &Entry) -> bool {
    match (a.year, b.year) {
        (Some(year_a), Some(year_b)) => (year_a - year_b).abs() <= MAX_YEAR_DIFFERENCE,
        // If either year is missing, don't filter on year
        _ => true,
    }
}

/// Calculate author overlap ratio (0.0 to 1.0)
/// Returns the fraction of local authors that have a fuzzy match in remote authors
pub fn author_overlap(local: &Entry, remote: &Entry) -> f64 {
    if local.authors.is_empty() || remote.authors.is_empty() {
        // Can't compute overlap, assume it's okay
        return 1.0;
    }

    let mut matches = 0;
    for local_author in &local.authors {
        let local_norm = normalize_string(local_author);
        // Check if any remote author matches this local author
        let has_match = remote.authors.iter().any(|remote_author| {
            let remote_norm = normalize_string(remote_author);
            // Check full name similarity
            let full_sim = jaro_winkler(&local_norm, &remote_norm);
            // Also check if last names match (common case: "John Smith" vs "J. Smith")
            let local_last = local_norm.split_whitespace().last().unwrap_or("");
            let remote_last = remote_norm.split_whitespace().last().unwrap_or("");
            let last_name_sim = jaro_winkler(local_last, remote_last);

            full_sim >= AUTHOR_MATCH_THRESHOLD || last_name_sim >= 0.9
        });
        if has_match {
            matches += 1;
        }
    }

    matches as f64 / local.authors.len() as f64
}

/// Calculate a combined match score considering title, year, and authors
pub fn match_score(target: &Entry, candidate: &Entry) -> f64 {
    let title_sim = title_similarity(target, candidate);

    // Hard filter: title must be reasonably similar
    if title_sim < TITLE_MATCH_THRESHOLD {
        return 0.0;
    }

    // Hard filter: years must be compatible
    if !years_compatible(target, candidate) {
        return 0.0;
    }

    // Compute author overlap
    let author_sim = author_overlap(target, candidate);

    // If we have author info and overlap is too low, reject
    if !target.authors.is_empty() && !candidate.authors.is_empty() && author_sim < MIN_AUTHOR_OVERLAP {
        return 0.0;
    }

    // Combined score: weight title heavily, but boost with author match
    // Title: 70%, Authors: 30%
    let base_score = title_sim * 0.7 + author_sim * 0.3;

    // Boost if DOIs match exactly
    if let (Some(doi_a), Some(doi_b)) = (&target.doi, &candidate.doi) {
        if doi_a.to_lowercase() == doi_b.to_lowercase() {
            return 1.0; // Perfect match
        }
    }

    base_score
}

/// Find the best matching entry from a list of candidates
pub fn find_best_match<'a>(target: &Entry, candidates: &'a [Entry]) -> Option<(&'a Entry, f64)> {
    candidates
        .iter()
        .map(|c| (c, match_score(target, c)))
        .filter(|(_, score)| *score > 0.0)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_similarity() {
        let mut a = Entry::new("a".to_string(), "article".to_string());
        a.title = Some("Deep Learning for Image Classification".to_string());

        let mut b = Entry::new("b".to_string(), "article".to_string());
        b.title = Some("Deep Learning for Image Classification".to_string());

        assert!(title_similarity(&a, &b) > 0.99);

        b.title = Some("Deep Learning for Image Recognition".to_string());
        assert!(title_similarity(&a, &b) > 0.85);

        b.title = Some("Quantum Computing in Finance".to_string());
        assert!(title_similarity(&a, &b) < 0.7);
    }

    #[test]
    fn test_year_mismatch() {
        let mut local = Entry::new("test".to_string(), "article".to_string());
        local.title = Some("Test Paper".to_string());
        local.year = Some(2021);

        let mut remote = Entry::new("test".to_string(), "article".to_string());
        remote.title = Some("Test Paper".to_string());
        remote.year = Some(2020);

        let discrepancies = compare_entries(&local, &remote);
        assert!(discrepancies.iter().any(|d| d.field == DiscrepancyField::Year));
    }
}
