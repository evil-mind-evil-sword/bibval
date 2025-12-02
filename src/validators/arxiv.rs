use super::{async_trait, Validator, ValidatorError};
use crate::entry::Entry;
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::Client;

const ARXIV_API_BASE: &str = "https://export.arxiv.org/api/query";

pub struct ArxivClient {
    client: Client,
}

impl ArxivClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("biblatex-validator/0.1.0")
            .build()
            .expect("Failed to create HTTP client");
        Self { client }
    }
}

impl Default for ArxivClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Validator for ArxivClient {
    async fn search_by_doi(&self, doi: &str) -> Result<Option<Entry>, ValidatorError> {
        // ArXiv doesn't support DOI search directly
        // Some papers have DOI but we can't search by it
        let _ = doi;
        Ok(None)
    }

    async fn search_by_title(&self, title: &str) -> Result<Vec<Entry>, ValidatorError> {
        let url = format!(
            "{}?search_query=ti:{}&max_results=5",
            ARXIV_API_BASE,
            urlencoding::encode(&format!("\"{}\"", title))
        );

        let response = self.client.get(&url).send().await?;
        let text = response.text().await?;

        parse_arxiv_atom(&text)
    }

    async fn search_by_arxiv_id(&self, arxiv_id: &str) -> Result<Option<Entry>, ValidatorError> {
        let url = format!("{}?id_list={}", ARXIV_API_BASE, arxiv_id);

        let response = self.client.get(&url).send().await?;
        let text = response.text().await?;

        let entries = parse_arxiv_atom(&text)?;
        Ok(entries.into_iter().next())
    }

    fn name(&self) -> &'static str {
        "ArXiv"
    }
}

/// Parse ArXiv Atom XML response
fn parse_arxiv_atom(xml: &str) -> Result<Vec<Entry>, ValidatorError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut entries = Vec::new();
    let mut current_entry: Option<Entry> = None;
    let mut current_tag = String::new();
    let mut in_author = false;
    let mut current_author = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                current_tag = name.clone();

                if name == "entry" {
                    current_entry = Some(Entry::new(String::new(), "article".to_string()));
                } else if name == "author" {
                    in_author = true;
                    current_author.clear();
                }
            }
            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                if name == "entry" {
                    if let Some(entry) = current_entry.take() {
                        if entry.title.is_some() {
                            entries.push(entry);
                        }
                    }
                } else if name == "author" {
                    in_author = false;
                    if let Some(ref mut entry) = current_entry {
                        if !current_author.is_empty() {
                            entry.authors.push(current_author.clone());
                        }
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Some(ref mut entry) = current_entry {
                    let text = e.unescape().unwrap_or_default().to_string();

                    match current_tag.as_str() {
                        "title" if !in_author => {
                            // Clean up title (remove newlines, extra spaces)
                            entry.title = Some(
                                text.split_whitespace()
                                    .collect::<Vec<_>>()
                                    .join(" "),
                            );
                        }
                        "id" => {
                            // Extract arXiv ID from URL: http://arxiv.org/abs/2301.12345v1
                            if text.contains("arxiv.org/abs/") {
                                let id = text
                                    .split("arxiv.org/abs/")
                                    .nth(1)
                                    .unwrap_or(&text)
                                    .to_string();
                                entry.arxiv_id = Some(id.clone());
                                entry.key = id;
                            }
                        }
                        "published" => {
                            // Extract year from date: 2023-01-15T00:00:00Z
                            if let Some(year_str) = text.split('-').next() {
                                entry.year = year_str.parse().ok();
                            }
                        }
                        "name" if in_author => {
                            current_author = text;
                        }
                        "arxiv:doi" | "doi" => {
                            entry.doi = Some(text);
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(ValidatorError::ParseError(format!(
                    "Error parsing ArXiv XML: {}",
                    e
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_arxiv_atom() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <entry>
    <id>http://arxiv.org/abs/2301.12345v1</id>
    <title>A Great Paper About Machine Learning</title>
    <published>2023-01-15T00:00:00Z</published>
    <author>
      <name>John Smith</name>
    </author>
    <author>
      <name>Jane Doe</name>
    </author>
  </entry>
</feed>"#;

        let entries = parse_arxiv_atom(xml).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].title,
            Some("A Great Paper About Machine Learning".to_string())
        );
        assert_eq!(entries[0].arxiv_id, Some("2301.12345v1".to_string()));
        assert_eq!(entries[0].year, Some(2023));
        assert_eq!(entries[0].authors.len(), 2);
    }
}
