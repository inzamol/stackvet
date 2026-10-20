use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use stackvet_core::{Diagnostic, LanguageTarget, Severity};
use stackvet_scanner::{
    hash_bytes, hash_file_content, CacheValidationResult, FileCacheEntry, IncrementalCacheManager,
    ScanCacheData,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn make_temp_repo(name: &str) -> PathBuf {
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "stackvet_test_incr_{}_{}_{}_{}",
        name,
        std::process::id(),
        unique,
        count
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_incremental_cache_cold_start_and_subsequent_hit() {
    let repo = make_temp_repo("cold_start");
    let file_a = repo.join("a.py");
    let file_b = repo.join("b.py");
    let manifest = repo.join("pyproject.toml");

    fs::write(&file_a, "import os\nprint('hello')\n").unwrap();
    fs::write(&file_b, "import sys\nprint('world')\n").unwrap();
    fs::write(
        &manifest,
        "[project]\nname = 'test-pkg'\nversion = '0.1.0'\n",
    )
    .unwrap();

    let cache_mgr = IncrementalCacheManager::new(&repo);
    let config_hash = hash_bytes(b"test_config");

    let manifests = vec![manifest.clone()];
    let source_files = vec![file_a.clone(), file_b.clone()];

    // 1. First run: cache is missing -> Invalid(CacheMissing)
    let val1 = cache_mgr.validate(&manifests, &source_files, &config_hash);
    match val1 {
        CacheValidationResult::Invalid(reason) => {
            assert_eq!(
                reason,
                stackvet_scanner::CacheInvalidationReason::CacheMissing
            );
        }
        _ => panic!("Expected CacheMissing on cold start"),
    }

    // Populate initial cache
    let mut cache_data = ScanCacheData::new(config_hash.clone());
    cache_data.manifest_hashes.insert(
        "pyproject.toml".to_string(),
        hash_file_content(&manifest).unwrap(),
    );

    let diag_a = Diagnostic::builder("DEP-003", Severity::Warning, LanguageTarget::Python)
        .message("Unused dependency")
        .location(stackvet_core::DiagnosticLocation::new(&file_a))
        .build();

    cache_data.files.insert(
        "a.py".to_string(),
        FileCacheEntry {
            content_hash: hash_file_content(&file_a).unwrap(),
            file_size: fs::metadata(&file_a).unwrap().len(),
            mtime: fs::metadata(&file_a)
                .unwrap()
                .modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            diagnostics: vec![diag_a.clone()],
        },
    );
    cache_data.files.insert(
        "b.py".to_string(),
        FileCacheEntry {
            content_hash: hash_file_content(&file_b).unwrap(),
            file_size: fs::metadata(&file_b).unwrap().len(),
            mtime: fs::metadata(&file_b)
                .unwrap()
                .modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            diagnostics: vec![],
        },
    );

    cache_mgr.save(&cache_data).unwrap();
    assert!(cache_mgr.cache_file_path().exists());

    // 2. Second run with unchanged files: Valid with NO changes (cache hit)
    let val2 = cache_mgr.validate(&manifests, &source_files, &config_hash);
    match val2 {
        CacheValidationResult::Valid {
            cached_data,
            changes,
        } => {
            assert!(
                !changes.has_changes(),
                "Expected no changes on identical files"
            );
            assert_eq!(changes.unchanged_files.len(), 2);
            assert_eq!(cached_data.files.len(), 2);
            assert_eq!(cached_data.files.get("a.py").unwrap().diagnostics.len(), 1);
        }
        _ => panic!("Expected valid cache hit"),
    }

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_incremental_cache_detects_modified_and_added_files() {
    let repo = make_temp_repo("mod_add");
    let file_a = repo.join("a.py");
    let manifest = repo.join("pyproject.toml");

    fs::write(&file_a, "print('initial')\n").unwrap();
    fs::write(&manifest, "[project]\nname = 'test'\n").unwrap();

    let cache_mgr = IncrementalCacheManager::new(&repo);
    let config_hash = hash_bytes(b"conf");
    let manifests = vec![manifest.clone()];

    let mut cache_data = ScanCacheData::new(config_hash.clone());
    cache_data.manifest_hashes.insert(
        "pyproject.toml".to_string(),
        hash_file_content(&manifest).unwrap(),
    );
    cache_data.files.insert(
        "a.py".to_string(),
        FileCacheEntry {
            content_hash: hash_file_content(&file_a).unwrap(),
            file_size: fs::metadata(&file_a).unwrap().len(),
            mtime: fs::metadata(&file_a)
                .unwrap()
                .modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            diagnostics: vec![],
        },
    );
    cache_mgr.save(&cache_data).unwrap();

    // Now: modify a.py and add c.py
    fs::write(&file_a, "print('modified content with different length')\n").unwrap();
    let file_c = repo.join("c.py");
    fs::write(&file_c, "print('newly added')\n").unwrap();

    let current_sources = vec![file_a.clone(), file_c.clone()];
    let val = cache_mgr.validate(&manifests, &current_sources, &config_hash);

    match val {
        CacheValidationResult::Valid { changes, .. } => {
            assert!(changes.has_changes());
            assert_eq!(changes.unchanged_files.len(), 0);
            assert_eq!(changes.modified_files.len(), 1);
            assert_eq!(changes.modified_files[0], file_a);
            assert_eq!(changes.added_files.len(), 1);
            assert_eq!(changes.added_files[0], file_c);
            assert_eq!(changes.deleted_files.len(), 0);
        }
        _ => panic!("Expected valid cache with detected changes"),
    }

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_incremental_cache_invalidates_on_manifest_change() {
    let repo = make_temp_repo("manifest_inval");
    let file_a = repo.join("main.py");
    let manifest = repo.join("pyproject.toml");

    fs::write(&file_a, "print('app')\n").unwrap();
    fs::write(&manifest, "[project]\ndependencies = ['requests']\n").unwrap();

    let cache_mgr = IncrementalCacheManager::new(&repo);
    let config_hash = hash_bytes(b"conf");

    let mut cache_data = ScanCacheData::new(config_hash.clone());
    cache_data.manifest_hashes.insert(
        "pyproject.toml".to_string(),
        hash_file_content(&manifest).unwrap(),
    );
    cache_mgr.save(&cache_data).unwrap();

    // Mutate manifest (e.g. adding httpx)
    fs::write(
        &manifest,
        "[project]\ndependencies = ['requests', 'httpx']\n",
    )
    .unwrap();

    let val = cache_mgr.validate(std::slice::from_ref(&manifest), &[file_a], &config_hash);
    match val {
        CacheValidationResult::Invalid(
            stackvet_scanner::CacheInvalidationReason::ManifestChanged { manifest: m },
        ) => {
            assert_eq!(m, "pyproject.toml");
        }
        _ => panic!("Expected cache invalidation on manifest modification"),
    }

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_incremental_cache_invalidates_on_config_change() {
    let repo = make_temp_repo("config_inval");
    let file_a = repo.join("main.py");
    let manifest = repo.join("pyproject.toml");

    fs::write(&file_a, "print('app')\n").unwrap();
    fs::write(&manifest, "[project]\nname='app'\n").unwrap();

    let cache_mgr = IncrementalCacheManager::new(&repo);
    let config_hash_1 = hash_bytes(b"config_v1");
    let config_hash_2 = hash_bytes(b"config_v2_with_new_rules");

    let mut cache_data = ScanCacheData::new(config_hash_1.clone());
    cache_data.manifest_hashes.insert(
        "pyproject.toml".to_string(),
        hash_file_content(&manifest).unwrap(),
    );
    cache_mgr.save(&cache_data).unwrap();

    // Scan with new config hash
    let val = cache_mgr.validate(&[manifest], &[file_a], &config_hash_2);
    assert_eq!(
        val,
        CacheValidationResult::Invalid(stackvet_scanner::CacheInvalidationReason::ConfigChanged)
    );

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_incremental_cache_handles_corruption_safely() {
    let repo = make_temp_repo("corrupt");
    let file_a = repo.join("main.py");
    let manifest = repo.join("pyproject.toml");

    fs::write(&file_a, "print(1)\n").unwrap();
    fs::write(&manifest, "[project]\n").unwrap();

    let cache_mgr = IncrementalCacheManager::new(&repo);
    let config_hash = hash_bytes(b"conf");

    // Write invalid/corrupt JSON into cache file
    fs::create_dir_all(repo.join(".stackvet/cache")).unwrap();
    fs::write(cache_mgr.cache_file_path(), "{ broken json: corrupt !!!").unwrap();

    let val = cache_mgr.validate(&[manifest], &[file_a], &config_hash);
    match val {
        CacheValidationResult::Invalid(
            stackvet_scanner::CacheInvalidationReason::CacheCorrupted(_),
        ) => {
            // Expected safe graceful fallback without panicking
        }
        _ => panic!("Expected CacheCorrupted invalidation on malformed cache file"),
    }

    let _ = fs::remove_dir_all(&repo);
}
