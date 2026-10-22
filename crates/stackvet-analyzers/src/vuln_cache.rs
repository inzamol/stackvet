use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_CACHE_TTL_SECS: u64 = 7 * 24 * 3600; // 7 days

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
}

impl VulnCacheManager {
    pub fn new(root: &Path) -> Self {
        let cache_path = root
            .join(".stackvet")
            .join("cache")
            .join("vulnerabilities.json");
        Self { cache_path }
    }

    pub fn with_custom_path(cache_path: PathBuf) -> Self {
        Self { cache_path }
    }

    pub fn cache_path(&self) -> &Path {
        &self.cache_path
    }

    pub fn ttl_seconds(&self) -> u64 {
        DEFAULT_CACHE_TTL_SECS
    }

    pub fn load_cache(&self) -> Result<Option<VulnDatabaseCache>> {
        if !self.cache_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.cache_path).with_context(|| {
            format!(
                "Failed to read vulnerability cache at {:?}",
                self.cache_path
            )
        })?;

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
