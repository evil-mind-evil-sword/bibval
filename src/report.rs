use crate::entry::{ApiSource, Discrepancy, Entry, Severity, ValidationResult};
use colored::Colorize;

/// A complete validation report for all entries
pub struct Report {
    pub entries: Vec<EntryReport>,
}

/// Report for a single bibliography entry
pub struct EntryReport {
    pub entry: Entry,
    pub status: EntryStatus,
    pub validation_results: Vec<ValidationResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryStatus {
    /// Entry validated successfully with no issues
    Ok(ApiSource),
    /// Entry has warnings but is likely correct
    Warning,
    /// Entry has errors that need attention
    Error,
    /// Could not validate (no matches found)
    NotFound,
    /// Validation failed due to API error
    Failed(String),
}

impl Report {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, report: EntryReport) {
        self.entries.push(report);
    }

    /// Count entries by status
    pub fn count_ok(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::Ok(_)))
            .count()
    }

    pub fn count_warnings(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::Warning))
            .count()
    }

    pub fn count_errors(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::Error))
            .count()
    }

    pub fn count_not_found(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::NotFound))
            .count()
    }

    pub fn count_failed(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::Failed(_)))
            .count()
    }

    /// Print the report to stdout with colors
    pub fn print(&self) {
        println!();
        println!("{}", "biblatex-validator Report".bold());
        println!("{}", "=".repeat(50));
        println!();

        let total = self.entries.len();
        let ok = self.count_ok();
        let warnings = self.count_warnings();
        let errors = self.count_errors();
        let not_found = self.count_not_found();
        let failed = self.count_failed();

        println!("Processed: {} entries", total);
        println!(
            "  {} validated, {} warnings, {} errors, {} failed, {} not found",
            ok.to_string().green(),
            warnings.to_string().yellow(),
            errors.to_string().red(),
            failed.to_string().red().bold(),
            not_found.to_string().dimmed()
        );
        println!();

        // Print errors first
        let error_entries: Vec<_> = self
            .entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::Error))
            .collect();

        if !error_entries.is_empty() {
            println!("{}", format!("ERRORS ({})", error_entries.len()).red().bold());
            for entry_report in error_entries {
                print_entry_report(entry_report);
            }
            println!();
        }

        // Print failures (API issues)
        let failed_entries: Vec<_> = self
            .entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::Failed(_)))
            .collect();

        if !failed_entries.is_empty() {
            println!("{}", format!("FAILED ({})", failed_entries.len()).red().bold());
            for entry_report in failed_entries {
                if let EntryStatus::Failed(reason) = &entry_report.status {
                    println!(
                        "  {} {}",
                        format!("[{}]", entry_report.entry.key).dimmed(),
                        reason
                    );
                }
            }
            println!();
        }

        // Print warnings
        let warning_entries: Vec<_> = self
            .entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::Warning))
            .collect();

        if !warning_entries.is_empty() {
            println!(
                "{}",
                format!("WARNINGS ({})", warning_entries.len())
                    .yellow()
                    .bold()
            );
            for entry_report in warning_entries {
                print_entry_report(entry_report);
            }
            println!();
        }

        // Print not found
        let not_found_entries: Vec<_> = self
            .entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::NotFound))
            .collect();

        if !not_found_entries.is_empty() {
            println!(
                "{}",
                format!("NOT FOUND ({})", not_found_entries.len())
                    .dimmed()
                    .bold()
            );
            for entry_report in not_found_entries {
                let title = entry_report
                    .entry
                    .title
                    .as_deref()
                    .unwrap_or("(no title)");
                println!(
                    "  {} {}",
                    format!("[{}]", entry_report.entry.key).dimmed(),
                    title
                );
            }
            println!();
        }

        // Print OK entries (brief)
        let ok_entries: Vec<_> = self
            .entries
            .iter()
            .filter(|e| matches!(e.status, EntryStatus::Ok(_)))
            .collect();

        if !ok_entries.is_empty() {
            println!("{}", format!("OK ({})", ok_entries.len()).green().bold());
            for entry_report in ok_entries.iter().take(5) {
                if let EntryStatus::Ok(source) = &entry_report.status {
                    println!(
                        "  {} Validated against {}",
                        format!("[{}]", entry_report.entry.key).dimmed(),
                        source.to_string().green()
                    );
                }
            }
            if ok_entries.len() > 5 {
                println!(
                    "  {} {} more...",
                    "...".dimmed(),
                    (ok_entries.len() - 5).to_string().dimmed()
                );
            }
        }

        println!();
    }
}

impl Default for Report {
    fn default() -> Self {
        Self::new()
    }
}

fn print_entry_report(entry_report: &EntryReport) {
    let key = format!("[{}]", entry_report.entry.key);

    for result in &entry_report.validation_results {
        for discrepancy in &result.discrepancies {
            print_discrepancy(&key, discrepancy, &result.source);
        }
    }
}

fn print_discrepancy(key: &str, discrepancy: &Discrepancy, source: &ApiSource) {
    let severity_str = match discrepancy.severity {
        Severity::Error => "ERROR".red(),
        Severity::Warning => "WARN".yellow(),
        Severity::Info => "INFO".blue(),
    };

    println!(
        "  {} {} {} (via {})",
        key.dimmed(),
        severity_str,
        discrepancy.message,
        source
    );

    if discrepancy.severity >= Severity::Warning {
        println!(
            "       Local:  {}",
            truncate(&discrepancy.local_value, 60).dimmed()
        );
        println!(
            "       Remote: {}",
            truncate(&discrepancy.remote_value, 60).dimmed()
        );
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
