use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use stackvet_core::Diagnostic;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CACHE_SCHEMA_VERSION: u32 = 2;
pub const DEFAULT_CACHE_DIR: &str = ".stackvet/cache";
pub const DEFAULT_CACHE_FILE: &str = "scan_cache.json";

/// Computes a fast, deterministic hex hash for a byte slice.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Computes a hash for a file's content on disk.
pub fn hash_file_content(path: &Path) -> Result<String> {
    let bytes = fs::read(path)
        .with_context(|| format!("Failed to read file for hashing: {}", path.display()))?;
    Ok(hash_bytes(&bytes))
}

/// Normalizes a path relative to the repository root with forward slashes for cross-platform stability.
pub fn normalize_rel_path(root: &Path, file_path: &Path) -> String {
    let rel = if file_path.is_absolute() {
        file_path.strip_prefix(root).unwrap_or(file_path)
    } else {
        file_path
    };
    rel.to_string_lossy().replace('\\', "/")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileCacheEntry {
    pub content_hash: String,
    pub file_size: u64,
    pub mtime: u128,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanCacheData {
    pub schema_version: u32,
    pub stackvet_version: String,
    pub timestamp: u128,
    pub config_hash: String,
    pub manifest_hashes: BTreeMap<String, String>,
    pub files: BTreeMap<String, FileCacheEntry>,
    #[serde(default)]
    pub project_diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vulnerability_check: Option<stackvet_core::VulnerabilityCheckSummary>,
}

impl ScanCacheData {
    pub fn new(config_hash: String) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            stackvet_version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp,
            config_hash,
            manifest_hashes: BTreeMap::new(),
            files: BTreeMap::new(),
            project_diagnostics: Vec::new(),
            vulnerability_check: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheInvalidationReason {
    CacheMissing,
    CacheCorrupted(String),
    VersionMismatch { cached: String, current: String },
    SchemaMismatch { cached: u32, current: u32 },
    ConfigChanged,
    ManifestChanged { manifest: String },
    ManifestCountChanged { cached: usize, current: usize },
    Disabled,
}

impl std::fmt::Display for CacheInvalidationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CacheMissing => write!(f, "Cache file does not exist"),
            Self::CacheCorrupted(err) => write!(f, "Cache is corrupted: {err}"),
            Self::VersionMismatch { cached, current } => {
                write!(f, "StackVet version changed from {cached} to {current}")
            }
            Self::SchemaMismatch { cached, current } => {
                write!(f, "Cache schema changed from {cached} to {current}")
            }
            Self::ConfigChanged => write!(f, "Configuration or ignore rules changed"),
            Self::ManifestChanged { manifest } => {
                write!(f, "Dependency manifest modified: {manifest}")
            }
            Self::ManifestCountChanged { cached, current } => {
                write!(
                    f,
                    "Manifest count changed (cached: {cached}, current: {current})"
                )
            }
            Self::Disabled => write!(f, "Incremental caching is disabled"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanChanges {
    pub unchanged_files: Vec<PathBuf>,
    pub modified_files: Vec<PathBuf>,
    pub added_files: Vec<PathBuf>,
    pub deleted_files: Vec<PathBuf>,
}

impl ScanChanges {
    pub fn has_changes(&self) -> bool {
        !self.modified_files.is_empty()
            || !self.added_files.is_empty()
            || !self.deleted_files.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum CacheValidationResult {
    Valid {
        cached_data: ScanCacheData,
        changes: ScanChanges,
    },
    Invalid(CacheInvalidationReason),
}

pub struct IncrementalCacheManager {
    root: PathBuf,
    cache_dir: PathBuf,
    cache_file: PathBuf,
}

impl IncrementalCacheManager {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let r = root.as_ref().to_path_buf();
        let c_dir = r.join(DEFAULT_CACHE_DIR);
        let c_file = c_dir.join(DEFAULT_CACHE_FILE);
        Self {
            root: r,
            cache_dir: c_dir,
            cache_file: c_file,
        }
    }

    pub fn with_custom_cache_dir(root: impl AsRef<Path>, cache_dir: impl AsRef<Path>) -> Self {
        let r = root.as_ref().to_path_buf();
        let c = cache_dir.as_ref();
        let c_dir = if c.is_absolute() {
            c.to_path_buf()
        } else {
            r.join(c)
        };
        let c_file = c_dir.join(DEFAULT_CACHE_FILE);
        Self {
            root: r,
            cache_dir: c_dir,
            cache_file: c_file,
        }
    }

    pub fn cache_file_path(&self) -> &Path {
        &self.cache_file
    }

    /// Loads and validates the cache against current configuration, manifests, and source files.
    pub fn validate(
        &self,
        current_manifests: &[PathBuf],
        current_source_files: &[PathBuf],
        current_config_hash: &str,
    ) -> CacheValidationResult {
        if !self.cache_file.exists() {
            return CacheValidationResult::Invalid(CacheInvalidationReason::CacheMissing);
        }

        let content = match fs::read_to_string(&self.cache_file) {
            Ok(c) => c,
            Err(e) => {
                return CacheValidationResult::Invalid(CacheInvalidationReason::CacheCorrupted(
                    e.to_string(),
                ))
            }
        };

        let cached_data: ScanCacheData = match serde_json::from_str(&content) {
            Ok(d) => d,
            Err(e) => {
                return CacheValidationResult::Invalid(CacheInvalidationReason::CacheCorrupted(
                    e.to_string(),
                ))
            }
        };

        // 1. Check schema version
        if cached_data.schema_version != CACHE_SCHEMA_VERSION {
            return CacheValidationResult::Invalid(CacheInvalidationReason::SchemaMismatch {
                cached: cached_data.schema_version,
                current: CACHE_SCHEMA_VERSION,
            });
        }

        // 2. Check StackVet engine version
        let current_version = env!("CARGO_PKG_VERSION");
        if cached_data.stackvet_version != current_version {
            return CacheValidationResult::Invalid(CacheInvalidationReason::VersionMismatch {
                cached: cached_data.stackvet_version,
                current: current_version.to_string(),
            });
        }

        // 3. Check configuration hash
        if cached_data.config_hash != current_config_hash {
            return CacheValidationResult::Invalid(CacheInvalidationReason::ConfigChanged);
        }

        // 4. Check manifests & lockfiles
        if cached_data.manifest_hashes.len() != current_manifests.len() {
            return CacheValidationResult::Invalid(CacheInvalidationReason::ManifestCountChanged {
                cached: cached_data.manifest_hashes.len(),
                current: current_manifests.len(),
            });
        }

        for manifest_path in current_manifests {
            let rel = normalize_rel_path(&self.root, manifest_path);
            let current_hash = match hash_file_content(manifest_path) {
                Ok(h) => h,
                Err(_) => {
                    return CacheValidationResult::Invalid(
                        CacheInvalidationReason::ManifestChanged { manifest: rel },
                    );
                }
            };

            match cached_data.manifest_hashes.get(&rel) {
                Some(cached_h) if cached_h == &current_hash => {}
                _ => {
                    return CacheValidationResult::Invalid(
                        CacheInvalidationReason::ManifestChanged { manifest: rel },
                    );
                }
            }
        }

        // 5. Categorize source files into unchanged, modified, added, deleted
        let mut changes = ScanChanges::default();
        let mut current_set = HashMap::new();

        for path in current_source_files {
            let rel = normalize_rel_path(&self.root, path);
            current_set.insert(rel.clone(), path.clone());

            let metadata = match fs::metadata(path) {
                Ok(m) => m,
                Err(_) => {
                    changes.modified_files.push(path.clone());
                    continue;
                }
            };

            let file_size = metadata.len();
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);

            if let Some(entry) = cached_data.files.get(&rel) {
                // If mtime and file size match and mtime is strictly older than cache timestamp,
                // assume unchanged without re-reading content.
                // Otherwise re-read and compare content hash.
                if entry.file_size == file_size
                    && entry.mtime == mtime
                    && mtime < cached_data.timestamp
                {
                    changes.unchanged_files.push(path.clone());
                } else {
                    let content_hash = hash_file_content(path).unwrap_or_default();
                    if entry.content_hash == content_hash {
                        changes.unchanged_files.push(path.clone());
                    } else {
                        changes.modified_files.push(path.clone());
                    }
                }
            } else {
                changes.added_files.push(path.clone());
            }
        }

        // Detect deleted files
        for cached_rel in cached_data.files.keys() {
            if !current_set.contains_key(cached_rel) {
                changes.deleted_files.push(self.root.join(cached_rel));
            }
        }

        CacheValidationResult::Valid {
            cached_data,
            changes,
        }
    }

    /// Saves updated scan cache atomically using a temporary file.
    pub fn save(&self, data: &ScanCacheData) -> Result<()> {
        if !self.cache_dir.exists() {
            fs::create_dir_all(&self.cache_dir).with_context(|| {
                format!(
                    "Failed to create cache directory '{}'",
                    self.cache_dir.display()
                )
            })?;
        }

        let serialized = serde_json::to_string_pretty(data)
            .context("Failed to serialize scan cache data to JSON")?;

        let tmp_file = self.cache_dir.join(format!("{}.tmp", DEFAULT_CACHE_FILE));
        fs::write(&tmp_file, &serialized).with_context(|| {
            format!(
                "Failed to write temporary cache file '{}'",
                tmp_file.display()
            )
        })?;

        fs::rename(&tmp_file, &self.cache_file).with_context(|| {
            format!(
                "Failed to atomically rename cache file to '{}'",
                self.cache_file.display()
            )
        })?;

        Ok(())
    }

    /// Clears any cached scan data.
    pub fn clear(&self) -> Result<()> {
        if self.cache_file.exists() {
            fs::remove_file(&self.cache_file).with_context(|| {
                format!(
                    "Failed to remove cache file '{}'",
                    self.cache_file.display()
                )
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "stackvet_test_cache_{}_{}",
            name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_cache_missing_validation() {
        let dir = make_test_dir("missing");
        let manager = IncrementalCacheManager::new(&dir);
        let res = manager.validate(&[], &[], "hash1");
        assert_eq!(
            res,
            CacheValidationResult::Invalid(CacheInvalidationReason::CacheMissing)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cache_atomic_save_and_load() {
        let dir = make_test_dir("save_load");
        let manager = IncrementalCacheManager::new(&dir);

        let mut data = ScanCacheData::new("conf_hash_1".to_string());
        data.files.insert(
            "app.py".to_string(),
            FileCacheEntry {
                content_hash: "abcd".to_string(),
                file_size: 100,
                mtime: 1000,
                diagnostics: vec![],
            },
        );

        manager.save(&data).unwrap();
        assert!(manager.cache_file_path().exists());

        // Validate with identical config and no manifests
        let res = manager.validate(&[], &[], "conf_hash_1");
        match res {
            CacheValidationResult::Valid {
                cached_data,
                changes,
            } => {
                assert_eq!(cached_data.config_hash, "conf_hash_1");
                assert_eq!(changes.deleted_files.len(), 1); // app.py not in current_source_files
            }
            _ => panic!("Expected valid cache"),
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cache_config_invalidation() {
        let dir = make_test_dir("config_inv");
        let manager = IncrementalCacheManager::new(&dir);

        let data = ScanCacheData::new("conf_hash_1".to_string());
        manager.save(&data).unwrap();

        let res = manager.validate(&[], &[], "conf_hash_2");
        assert_eq!(
            res,
            CacheValidationResult::Invalid(CacheInvalidationReason::ConfigChanged)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cache_manifest_invalidation() {
        let dir = make_test_dir("manifest_inv");
        let manifest = dir.join("pyproject.toml");
        fs::write(&manifest, "[project]\nname='test'\n").unwrap();

        let manager = IncrementalCacheManager::new(&dir);
        let mut data = ScanCacheData::new("conf1".to_string());
        data.manifest_hashes.insert(
            "pyproject.toml".to_string(),
            hash_file_content(&manifest).unwrap(),
        );
        manager.save(&data).unwrap();

        // Mutate manifest
        fs::write(&manifest, "[project]\nname='test2'\n").unwrap();

        let res = manager.validate(std::slice::from_ref(&manifest), &[], "conf1");
        match res {
            CacheValidationResult::Invalid(CacheInvalidationReason::ManifestChanged {
                manifest: m,
            }) => {
                assert_eq!(m, "pyproject.toml");
            }
            _ => panic!("Expected ManifestChanged invalidation"),
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
