use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_CACHE_TTL_SECS: u64 = 7 * 24 * 3600; // 7 days
pub const DEFAULT_MAX_VULN_CACHE_SIZE_BYTES: u64 = 20 * 1024 * 1024; // 20 MB

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CachedVulnerability {
    pub id: String,
    pub summary: String,
    pub severity: String,
    pub fixed_in: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VulnCacheEntry {
    pub package: String,
    pub ecosystem: String,
    pub version: String,
    pub vulnerabilities: Vec<CachedVulnerability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnDatabaseCache {
    pub schema_version: String,
    pub source: String,
    pub last_updated: u64,
    pub ttl_seconds: u64,
    pub entries: Vec<VulnCacheEntry>,
}

impl VulnDatabaseCache {
    pub fn new(ttl_seconds: u64) -> Self {
        Self {
            schema_version: "1.0.0".to_string(),
            source: "osv.dev".to_string(),
            last_updated: current_timestamp(),
            ttl_seconds,
            entries: Vec::new(),
        }
    }
}

impl Default for VulnDatabaseCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_TTL_SECS)
    }
}

pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VulnCacheStatus {
    Fresh { age_seconds: u64 },
    Stale { age_seconds: u64, ttl_seconds: u64 },
    Missing,
}

#[derive(Debug, Clone)]
pub struct VulnCacheManager {
    cache_path: PathBuf,
    max_file_size: u64,
    root: Option<PathBuf>,
}

impl VulnCacheManager {
    pub fn new(root: &Path) -> Self {
        let cache_path = root
            .join(".stackvet")
            .join("cache")
            .join("vulnerabilities.json");
        Self {
            cache_path,
            max_file_size: DEFAULT_MAX_VULN_CACHE_SIZE_BYTES,
            root: Some(root.to_path_buf()),
        }
    }

    pub fn with_custom_path(cache_path: PathBuf) -> Self {
        Self {
            cache_path,
            max_file_size: DEFAULT_MAX_VULN_CACHE_SIZE_BYTES,
            root: None,
        }
    }

    pub fn with_root(mut self, root: impl AsRef<Path>) -> Self {
        self.root = Some(root.as_ref().to_path_buf());
        self
    }

    pub fn with_max_file_size(mut self, max_size: u64) -> Self {
        self.max_file_size = max_size;
        self
    }

    pub fn max_file_size(&self) -> u64 {
        self.max_file_size
    }

    pub fn cache_path(&self) -> &Path {
        &self.cache_path
    }

    pub fn ttl_seconds(&self) -> u64 {
        DEFAULT_CACHE_TTL_SECS
    }

    pub fn load_cache(&self) -> Result<Option<VulnDatabaseCache>> {
        let symlink_meta = match fs::symlink_metadata(&self.cache_path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                anyhow::bail!(
                    "Failed to inspect vulnerability cache at {:?}: {e}",
                    self.cache_path
                );
            }
        };

        if symlink_meta.is_symlink() {
            if let Ok(canonical_path) = self.cache_path.canonicalize() {
                if let Some(ref root) = self.root {
                    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.clone());
                    if !canonical_path.starts_with(&canonical_root) {
                        anyhow::bail!(
                            "Vulnerability cache symlink at {:?} escapes repository boundary",
                            self.cache_path
                        );
                    }
                }
            } else {
                anyhow::bail!(
                    "Vulnerability cache at {:?} is a broken or unresolvable symlink",
                    self.cache_path
                );
            }
        }

        let metadata = fs::metadata(&self.cache_path).with_context(|| {
            format!(
                "Failed to read metadata for vulnerability cache at {:?}",
                self.cache_path
            )
        })?;

        if !metadata.is_file() {
            anyhow::bail!(
                "Vulnerability cache at {:?} is not a regular file",
                self.cache_path
            );
        }

        if metadata.len() > self.max_file_size {
            anyhow::bail!(
                "Vulnerability cache at {:?} exceeds maximum allowed size ({} bytes > {} bytes limit)",
                self.cache_path,
                metadata.len(),
                self.max_file_size
            );
        }

        let file = fs::File::open(&self.cache_path).with_context(|| {
            format!(
                "Failed to open vulnerability cache at {:?}",
                self.cache_path
            )
        })?;

        use std::io::Read;
        let mut handle = std::io::Read::take(file, self.max_file_size + 1);
        let mut content = String::new();
        handle.read_to_string(&mut content).with_context(|| {
            format!(
                "Failed to read vulnerability cache at {:?}",
                self.cache_path
            )
        })?;

        if content.len() as u64 > self.max_file_size {
            anyhow::bail!(
                "Vulnerability cache at {:?} exceeds maximum allowed size ({} bytes limit)",
                self.cache_path,
                self.max_file_size
            );
        }

        let cache: VulnDatabaseCache = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse vulnerability cache JSON at {:?}",
                self.cache_path
            )
        })?;

        Ok(Some(cache))
    }

    pub fn status(&self, cache: Option<&VulnDatabaseCache>) -> VulnCacheStatus {
        match cache {
            None => VulnCacheStatus::Missing,
            Some(c) => {
                let now = current_timestamp();
                let age = now.saturating_sub(c.last_updated);
                if age > c.ttl_seconds {
                    VulnCacheStatus::Stale {
                        age_seconds: age,
                        ttl_seconds: c.ttl_seconds,
                    }
                } else {
                    VulnCacheStatus::Fresh { age_seconds: age }
                }
            }
        }
    }

    pub fn save_cache(&self, cache: &VulnDatabaseCache) -> Result<()> {
        if let Some(parent) = self.cache_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create cache directory {:?}", parent))?;
        }

        let json = serde_json::to_string_pretty(cache)
            .context("Failed to serialize vulnerability cache")?;

        let tmp_path = self.cache_path.with_extension("tmp");
        fs::write(&tmp_path, json)
            .with_context(|| format!("Failed to write temporary cache at {:?}", tmp_path))?;

        fs::rename(&tmp_path, &self.cache_path)
            .with_context(|| format!("Failed to commit cache file to {:?}", self.cache_path))?;

        Ok(())
    }

    pub fn lookup<'a>(
        &self,
        cache: &'a VulnDatabaseCache,
        package: &str,
        ecosystem: &str,
        version: &str,
    ) -> Option<&'a [CachedVulnerability]> {
        let norm_pkg = normalize_pkg_name(package, ecosystem);
        let norm_eco = ecosystem.to_ascii_lowercase();

        for entry in &cache.entries {
            if entry.ecosystem.to_ascii_lowercase() == norm_eco
                && normalize_pkg_name(&entry.package, ecosystem) == norm_pkg
                && (entry.version == version || entry.version == "*")
            {
                return Some(&entry.vulnerabilities);
            }
        }
        None
    }
}

pub fn normalize_pkg_name(pkg: &str, ecosystem: &str) -> String {
    if ecosystem.eq_ignore_ascii_case("pypi") || ecosystem.eq_ignore_ascii_case("python") {
        pkg.replace('_', "-").to_lowercase()
    } else {
        pkg.to_lowercase()
    }
}
