use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use thiserror::Error;

const CACHE_TTL_SECS: u64 = 86400 * 7; // 7 days

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("Failed to create cache directory: {0}")]
    CreateDir(std::io::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub struct Cache {
    cache_dir: PathBuf,
    enabled: bool,
}

impl Cache {
    pub fn new(enabled: bool) -> Result<Self, CacheError> {
        let base_dir = dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".cache"));
        let preferred = base_dir.join("bibval");
        let legacy = base_dir.join("biblatex-validator");

        // Prefer the new path, but continue to use legacy if it already exists
        let cache_dir = if legacy.exists() && !preferred.exists() {
            legacy
        } else {
            preferred
        };

        if enabled {
            fs::create_dir_all(&cache_dir).map_err(CacheError::CreateDir)?;
        }

        Ok(Self { cache_dir, enabled })
    }

    /// Generate a cache key from the API name and query
    fn cache_key(&self, api: &str, query: &str) -> PathBuf {
        // Use a stable hash of the query to avoid filesystem issues and ensure repeatability
        let hash = blake3_hash(query);
        self.cache_dir.join(format!("{}_{}.json", api, hash))
    }

    /// Get a cached response if it exists and is not expired
    pub fn get<T: DeserializeOwned>(&self, api: &str, query: &str) -> Option<T> {
        if !self.enabled {
            return None;
        }

        let path = self.cache_key(api, query);

        // Check if file exists and is not expired
        let metadata = fs::metadata(&path).ok()?;
        let modified = metadata.modified().ok()?;
        let age = SystemTime::now().duration_since(modified).ok()?;

        if age > Duration::from_secs(CACHE_TTL_SECS) {
            // Cache expired, remove the file
            let _ = fs::remove_file(&path);
            return None;
        }

        let content = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Store a response in the cache
    pub fn set<T: Serialize>(&self, api: &str, query: &str, value: &T) -> Result<(), CacheError> {
        if !self.enabled {
            return Ok(());
        }

        let path = self.cache_key(api, query);
        let content = serde_json::to_string(value)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Clear all cached data
    pub fn clear(&self) -> Result<(), CacheError> {
        if self.cache_dir.exists() {
            for entry in fs::read_dir(&self.cache_dir)? {
                let entry = entry?;
                if entry.path().extension().map_or(false, |e| e == "json") {
                    fs::remove_file(entry.path())?;
                }
            }
        }
        Ok(())
    }
}

/// Simple hash function for cache keys (stable across runs)
fn blake3_hash(s: &str) -> String {
    blake3::hash(s.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestData {
        value: String,
    }

    #[test]
    fn test_cache_round_trip() {
        let dir = tempdir().unwrap();
        let mut cache = Cache::new(true).unwrap();
        cache.cache_dir = dir.path().to_path_buf();

        let data = TestData {
            value: "test".to_string(),
        };

        cache.set("test_api", "query", &data).unwrap();
        let retrieved: Option<TestData> = cache.get("test_api", "query");

        assert_eq!(retrieved, Some(data));
    }

    #[test]
    fn cache_keys_are_stable() {
        let dir = tempdir().unwrap();
        let cache = Cache {
            cache_dir: dir.path().to_path_buf(),
            enabled: true,
        };

        let first = cache.cache_key("api", "query");
        let second = cache.cache_key("api", "query");
        let different = cache.cache_key("api", "other");

        assert_eq!(first, second);
        assert_ne!(first, different);
    }
}
