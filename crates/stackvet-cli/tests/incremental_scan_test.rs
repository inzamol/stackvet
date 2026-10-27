use std::fs;
use std::path::PathBuf;
use std::process::Command;
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

#[test]
fn test_incremental_cache_preserves_and_restores_vulnerability_check_status() {
    let repo = make_temp_repo("vuln_status_cache");
    let cache_mgr = IncrementalCacheManager::new(&repo);
    let config_hash = hash_bytes(b"test_conf");

    let mut cache_data = ScanCacheData::new(config_hash.clone());
    let vuln_summary = stackvet_core::VulnerabilityCheckSummary {
        status: stackvet_core::VulnerabilityCheckStatus::CompletedWithWarnings,
        message: "Vulnerability scan performed using expired local advisory cache".to_string(),
        advisory_database: Some("OSV.dev".to_string()),
        cache_status: Some("stale".to_string()),
        vulnerabilities_found: Some(1),
    };
    cache_data.vulnerability_check = Some(vuln_summary.clone());

    cache_mgr.save(&cache_data).expect("save cache");
    let content = fs::read_to_string(cache_mgr.cache_file_path()).expect("read cache file");
    let loaded: ScanCacheData = serde_json::from_str(&content).expect("deserialize cache");
    assert_eq!(loaded.vulnerability_check, Some(vuln_summary));

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_cli_scan_excludes_stackvet_cache_and_custom_cache_dir() {
    let repo = make_temp_repo("cli_cache_exclusion");
    let manifest = repo.join("pyproject.toml");
    let main_file = repo.join("main.py");

    fs::write(
        &manifest,
        "[project]\nname = 'clean-test-app'\nversion = '0.1.0'\n",
    )
    .unwrap();
    fs::write(&main_file, "print('hello world')\n").unwrap();

    // Set scanner max_file_size to 1000 bytes via .stackvet.toml
    fs::write(
        repo.join(".stackvet.toml"),
        "[scanner]\nmax_file_size = 1000\n",
    )
    .unwrap();

    // 1. Populate standard .stackvet/cache/scan_cache.json with 50KB data
    let default_cache_dir = repo.join(".stackvet").join("cache");
    fs::create_dir_all(&default_cache_dir).unwrap();
    let large_json = format!("{{\"data\": \"{}\"}}", "x".repeat(50_000));
    fs::write(default_cache_dir.join("scan_cache.json"), &large_json).unwrap();
    fs::write(default_cache_dir.join("vulnerabilities.json"), "[]").unwrap();

    let exe = env!("CARGO_BIN_EXE_stackvet");

    // Scan with tiny max-file-size 1000 bytes (configured via .stackvet.toml) and offline mode
    let output = Command::new(exe)
        .arg("--path")
        .arg(&repo)
        .arg("--offline")
        .arg("--format")
        .arg("json")
        .arg("scan")
        .output()
        .expect("Run stackvet scan json");

    assert!(
        output.status.success(),
        "Scan should exit with success: code={:?}, stdout={}, stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout).expect("parse json");

    let diagnostics = val["diagnostics"].as_array().expect("diagnostics array");
    let disc_005_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d["rule_id"].as_str() == Some("DISC-005"))
        .collect();
    assert!(
        disc_005_diags.is_empty(),
        "Expected no DISC-005 diagnostics for .stackvet cache, found: {:?}",
        disc_005_diags
    );

    // 2. Populate custom cache dir and pass --cache-dir custom_cache
    let custom_cache_dir = repo.join("custom_cache");
    fs::create_dir_all(&custom_cache_dir).unwrap();
    fs::write(custom_cache_dir.join("scan_cache.json"), &large_json).unwrap();

    let output_custom = Command::new(exe)
        .arg("--path")
        .arg(&repo)
        .arg("--cache-dir")
        .arg("custom_cache")
        .arg("--offline")
        .arg("--format")
        .arg("json")
        .arg("scan")
        .output()
        .expect("Run stackvet scan with custom cache dir");

    assert!(
        output_custom.status.success(),
        "Scan with custom cache-dir should succeed: code={:?}, stdout={}, stderr={}",
        output_custom.status.code(),
        String::from_utf8_lossy(&output_custom.stdout),
        String::from_utf8_lossy(&output_custom.stderr)
    );
    let stdout_custom = String::from_utf8_lossy(&output_custom.stdout);
    let val_custom: serde_json::Value =
        serde_json::from_str(&stdout_custom).expect("parse json custom");

    let custom_diagnostics = val_custom["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    let disc_005_custom: Vec<_> = custom_diagnostics
        .iter()
        .filter(|d| d["rule_id"].as_str() == Some("DISC-005"))
        .collect();
    assert!(
        disc_005_custom.is_empty(),
        "Expected no DISC-005 diagnostics for custom cache dir, found: {:?}",
        disc_005_custom
    );

    let _ = fs::remove_dir_all(&repo);
}
