use bibval::{parser, BibValidator, ValidatorConfig};
use clap::Parser;
use colored::Colorize;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "bibval")]
#[command(version = "0.1.0")]
#[command(about = "Validate BibTeX/BibLaTeX references against academic databases", long_about = None)]
struct Args {
    /// Input .bib file(s) to validate
    #[arg(required = true)]
    files: Vec<PathBuf>,

    /// Disable CrossRef API
    #[arg(long)]
    no_crossref: bool,

    /// Disable DBLP API
    #[arg(long)]
    no_dblp: bool,

    /// Disable ArXiv API
    #[arg(long)]
    no_arxiv: bool,

    /// Disable Semantic Scholar API
    #[arg(long)]
    no_semantic: bool,

    /// Disable OpenAlex API
    #[arg(long)]
    no_openalex: bool,

    /// Disable Open Library API
    #[arg(long)]
    no_openlibrary: bool,

    /// Disable caching of API responses
    #[arg(long)]
    no_cache: bool,

    /// Strict mode: exit with error code if any issues found
    #[arg(long, short)]
    strict: bool,

    /// Verbose output
    #[arg(long, short)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    // Initialize logging
    if args.verbose {
        tracing_subscriber::fmt()
            .with_env_filter("bibval=debug")
            .init();
    }

    // Parse all input files
    let mut all_entries = Vec::new();

    for file in &args.files {
        if !file.exists() {
            eprintln!("{} File not found: {}", "Error:".red().bold(), file.display());
            return ExitCode::FAILURE;
        }

        println!("Parsing {}...", file.display().to_string().cyan());

        match parser::parse_bib_file(file) {
            Ok(entries) => {
                println!("  Found {} entries", entries.len());
                all_entries.extend(entries);
            }
            Err(e) => {
                eprintln!(
                    "{} Failed to parse {}: {}",
                    "Error:".red().bold(),
                    file.display(),
                    e
                );
                return ExitCode::FAILURE;
            }
        }
    }

    if all_entries.is_empty() {
        println!("{}", "No entries found to validate.".yellow());
        return ExitCode::SUCCESS;
    }

    println!();
    println!("Validating {} entries...", all_entries.len());
    println!();

    // Configure validator
    let config = ValidatorConfig {
        use_crossref: !args.no_crossref,
        use_dblp: !args.no_dblp,
        use_arxiv: !args.no_arxiv,
        use_semantic: !args.no_semantic,
        use_openalex: !args.no_openalex,
        use_openlibrary: !args.no_openlibrary,
        cache_enabled: !args.no_cache,
    };

    // Create validator
    let validator = match BibValidator::new(config) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} Failed to initialize validator: {}", "Error:".red().bold(), e);
            return ExitCode::FAILURE;
        }
    };

    // Run validation
    let report = validator.validate(all_entries).await;

    // Print report
    report.print();

    // Determine exit code
    if args.strict && (report.count_errors() > 0 || report.count_warnings() > 0) {
        ExitCode::FAILURE
    } else if report.count_errors() > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
