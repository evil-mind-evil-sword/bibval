# bibval

**Citation validator.** Check BibTeX entries against academic databases.

Validates bibliographic references by querying CrossRef, DBLP, arXiv, Semantic Scholar, and other sources.

## Installation

```bash
cargo install --path .
```

Or from crates.io (once published):

```bash
cargo install bibval
```

## Why?

BibTeX files accumulate errors over time. You copy a citation from Google Scholar, but the year is wrong. You import from Zotero, but the title has curly quotes that break compilation. You cite a preprint that's since been published, but now your bibliography points to the wrong venue.

bibval catches these by checking your entries against the source of truth: CrossRef for DOIs, DBLP for CS publications, arXiv for preprints, and five other academic databases. It queries them in parallel and caches responses locally, so repeated runs are fast. When it finds a mismatch—wrong year, different title, missing DOI—it tells you exactly what's wrong and where the correct data came from.

## Usage

```bash
bibval references.bib
```

Validate multiple files:

```bash
bibval paper.bib thesis.bib
```

### Options

| Flag | Description |
|------|-------------|
| `--no-crossref` | Disable CrossRef API |
| `--no-dblp` | Disable DBLP API |
| `--no-arxiv` | Disable ArXiv API |
| `--no-semantic` | Disable Semantic Scholar API |
| `--no-openalex` | Disable OpenAlex API |
| `--no-openlibrary` | Disable Open Library API |
| `--no-openreview` | Disable OpenReview API |
| `--no-zenodo` | Disable Zenodo API |
| `--no-cache` | Disable caching of API responses |
| `-s, --strict` | Exit with error if any issues found |
| `-v, --verbose` | Verbose output |
| `-k, --key KEY` | Only validate entries with these citation keys (comma-separated or repeatable) |

### Example Output

```
bibval Report
==================================================

Processed: 84 entries
  58 validated, 9 warnings, 13 errors, 4 not found

ERRORS (13)
  [bingham_pyro_2019] ERROR Year mismatch: 2019 vs 2018 (via DBLP)
       Local:  2019
       Remote: 2018
  ...

WARNINGS (9)
  [carpenter_stan_2017] WARN Title slightly different (similarity: 88%) (via CrossRef)
  ...

OK (58)
  [lew_probabilistic_2023] Validated against CrossRef
  ...
```

## Validators

bibval queries multiple academic databases in parallel:

- **CrossRef** - DOI resolution and metadata
- **DBLP** - Computer science bibliography
- **ArXiv** - Preprint repository
- **Semantic Scholar** - AI-powered academic search
- **OpenAlex** - Open catalog of 250M+ scholarly works
- **Open Library** - Books and older publications
- **OpenReview** - ML conference papers (ICLR, NeurIPS, etc.)
- **Zenodo** - Software artifacts, datasets, and research outputs

## What It Checks

- **Year mismatches** - Publication year differs from database
- **Title differences** - Fuzzy matching with similarity scores
- **Author discrepancies** - Missing authors or spelling variations
- **Missing DOIs** - Entry lacks DOI when one exists

## Caching

API responses are cached locally to speed up repeated validations. Cache is stored in:

- Linux/macOS: `~/.cache/bibval/`
- Windows: `%LOCALAPPDATA%\bibval\`

Disable with `--no-cache`.

## Exit Codes

- `0` - All entries validated successfully (or warnings only)
- `1` - Errors found or validation failed

Use `--strict` to treat warnings as errors.

## License

MIT
