use stackvet_analyzers::security::SecurityAnalyzer;
use stackvet_analyzers::vuln_cache::{
    CachedVulnerability, VulnCacheEntry, VulnCacheManager, VulnCacheStatus, VulnDatabaseCache,
};
use stackvet_core::{Analyzer, ProjectContext, PythonProjectInfo};

use std::fs;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("stackvet_test_vuln_cache")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn create_dummy_python_context(root: &Path, direct_deps: &[(&str, &str)]) -> ProjectContext {
    let manifest_file = root.join("pyproject.toml");
    fs::write(&manifest_file, "[project]\nname = \"dummy\"\n").unwrap();

    let mut py_info = PythonProjectInfo {
        manifest_file: manifest_file.clone(),
        ..Default::default()
    };
    for (pkg, ver) in direct_deps {
        py_info
            .direct_dependencies
            .insert(pkg.to_string(), ver.to_string());
        py_info
            .locked_packages
            .insert(pkg.to_string(), ver.to_string());
    }

    let mut ctx = ProjectContext::new(root.to_path_buf());
    ctx.manifest_files.push(manifest_file);
    ctx.python = Some(py_info);
    ctx
}

#[test]
fn test_vuln_cache_missing_in_offline_mode_emits_sec001() {
    let dir = temp_test_dir("missing_cache");
    let ctx = create_dummy_python_context(&dir, &[("requests", "2.25.0")]);

    let cache_path = dir.join("nonexistent_cache.json");
    let analyzer = SecurityAnalyzer::with_cache_path(true, false, cache_path);

    let diags = analyzer.analyze(&ctx).expect("analyze");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id.0, "SEC-001");
    assert!(diags[0].message.contains("offline mode is active"));
}

#[test]
fn test_vuln_cache_fresh_detects_vulnerability() {
    let dir = temp_test_dir("fresh_cache");
    let ctx = create_dummy_python_context(&dir, &[("requests", "2.25.0")]);

    let cache_path = dir.join("vulnerabilities.json");
    let mgr = VulnCacheManager::with_custom_path(cache_path.clone());

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let cache = VulnDatabaseCache {
        schema_version: "1.0.0".to_string(),
        source: "osv.dev".to_string(),
        last_updated: now,
        ttl_seconds: 604800,
        entries: vec![VulnCacheEntry {
            package: "requests".to_string(),
            ecosystem: "PyPI".to_string(),
            version: "2.25.0".to_string(),
            vulnerabilities: vec![CachedVulnerability {
                id: "GHSA-j8r2-6x86-q33q".to_string(),
                summary: "Requests Proxy-Authorization leak".to_string(),
                severity: "HIGH".to_string(),
                fixed_in: Some("2.31.0".to_string()),
            }],
        }],
    };

    mgr.save_cache(&cache).expect("save cache");
    assert_eq!(
        mgr.status(Some(&cache)),
        VulnCacheStatus::Fresh { age_seconds: 0 }
    );

    let analyzer = SecurityAnalyzer::with_cache_path(true, false, cache_path);
    let diags = analyzer.analyze(&ctx).expect("analyze");

    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id.0, "DEP-006");
    assert!(diags[0].message.contains("requests==2.25.0"));
    assert!(diags[0].message.contains("GHSA-j8r2-6x86-q33q"));
    assert!(diags[0]
        .suggested_remediation
        .contains("Upgrade 'requests' to version 2.31.0"));
}

#[test]
fn test_vuln_cache_stale_emits_sec002_and_vulnerabilities() {
    let dir = temp_test_dir("stale_cache");
    let ctx = create_dummy_python_context(&dir, &[("urllib3", "1.26.4")]);

    let cache_path = dir.join("vulnerabilities.json");
    let mgr = VulnCacheManager::with_custom_path(cache_path.clone());

    let old_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - (10 * 86400); // 10 days old (TTL is 7 days)

    let cache = VulnDatabaseCache {
        schema_version: "1.0.0".to_string(),
        source: "osv.dev".to_string(),
        last_updated: old_time,
        ttl_seconds: 604800,
        entries: vec![VulnCacheEntry {
            package: "urllib3".to_string(),
            ecosystem: "PyPI".to_string(),
            version: "1.26.4".to_string(),
            vulnerabilities: vec![CachedVulnerability {
                id: "CVE-2021-33503".to_string(),
                summary: "Catastrophic backtracking in URL authority parser".to_string(),
                severity: "CRITICAL".to_string(),
                fixed_in: Some("1.26.5".to_string()),
            }],
        }],
    };

    mgr.save_cache(&cache).expect("save cache");

    let analyzer = SecurityAnalyzer::with_cache_path(true, false, cache_path.clone());
    let diags = analyzer.analyze(&ctx).expect("analyze");

    // Must emit SEC-002 (stale cache warning) AND DEP-006 (finding from stale cache)
    assert_eq!(diags.len(), 2);
    let rule_ids: Vec<&str> = diags.iter().map(|d| d.rule_id.0.as_str()).collect();
    assert!(
        rule_ids.contains(&"SEC-002"),
        "Expected SEC-002 warning for stale cache"
    );
    assert!(
        rule_ids.contains(&"DEP-006"),
        "Expected DEP-006 vulnerability finding"
    );
}

#[test]
fn test_refresh_cache_updates_timestamp() {
    let dir = temp_test_dir("refresh_cache");
    let ctx = create_dummy_python_context(&dir, &[("requests", "2.25.0")]);

    let cache_path = dir.join("vulnerabilities.json");
    let mgr = VulnCacheManager::with_custom_path(cache_path.clone());

    let past_time = 1_000_000;
    let cache = VulnDatabaseCache {
        schema_version: "1.0.0".to_string(),
        source: "osv.dev".to_string(),
        last_updated: past_time,
        ttl_seconds: 604800,
        entries: Vec::new(),
    };
    mgr.save_cache(&cache).expect("save cache");

    // Run with refresh_cache = true
    let analyzer = SecurityAnalyzer::with_cache_path(false, true, cache_path);
    let _ = analyzer.analyze(&ctx).expect("analyze");

    let updated_cache = mgr.load_cache().expect("load").expect("exists");
    assert!(updated_cache.last_updated > past_time);
}

#[test]
fn test_refresh_cache_when_cache_missing_populates_and_saves() {
    let dir = temp_test_dir("missing_refresh");
    let ctx = create_dummy_python_context(&dir, &[("jinja2", "2.4.1")]);

    let cache_path = dir.join("vulnerabilities.json");
    let mgr = VulnCacheManager::with_custom_path(cache_path.clone());

    // Cache does not exist initially
    assert!(!cache_path.exists());

    // Run with refresh_cache = true and offline = false
    let analyzer = SecurityAnalyzer::with_cache_path(false, true, cache_path.clone());
    let diags = analyzer.analyze(&ctx).expect("analyze");

    // Cache should now exist on disk
    assert!(cache_path.exists(), "Cache file must be created on refresh");

    // SEC-001 (missing cache warning) must NOT be emitted
    let has_sec001 = diags.iter().any(|d| d.rule_id.0 == "SEC-001");
    assert!(
        !has_sec001,
        "SEC-001 should not be emitted when cache is refreshed"
    );

    // Loaded cache must have entries
    let loaded = mgr.load_cache().expect("load").expect("cache exists");
    assert!(
        !loaded.entries.is_empty(),
        "Cache should be populated with entries"
    );
}

#[test]
fn test_corrupt_cache_is_treated_as_missing_and_emits_sec001() {
    let dir = temp_test_dir("corrupt_cache_offline");
    let ctx = create_dummy_python_context(&dir, &[("requests", "2.25.0")]);

    let cache_path = dir.join("vulnerabilities.json");
    // Write invalid/corrupt JSON content
    fs::write(&cache_path, "{ invalid_json: true, unterminated...").unwrap();

    let analyzer = SecurityAnalyzer::with_cache_path(true, false, cache_path);
    // Should NOT return Err, but succeed and emit SEC-001
    let diags = analyzer
        .analyze(&ctx)
        .expect("analyze must not fail on corrupt cache");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id.0, "SEC-001");
    assert!(
        diags[0].message.contains("corrupted or invalid"),
        "Message should indicate corrupt cache: {}",
        diags[0].message
    );
}

#[test]
#[ignore = "requires network access to api.osv.dev"]
fn test_corrupt_cache_is_repaired_on_refresh() {
    let dir = temp_test_dir("corrupt_cache_refresh");
    let ctx = create_dummy_python_context(&dir, &[("jinja2", "2.4.1")]);

    let cache_path = dir.join("vulnerabilities.json");
    fs::write(&cache_path, "MALFORMED_NON_JSON_DATA").unwrap();

    let analyzer = SecurityAnalyzer::with_cache_path(false, true, cache_path.clone());
    let diags = analyzer
        .analyze(&ctx)
        .expect("analyze must succeed when refreshing corrupt cache");

    // SEC-001 should not be emitted because cache was refreshed
    let has_sec001 = diags.iter().any(|d| d.rule_id.0 == "SEC-001");
    assert!(
        !has_sec001,
        "SEC-001 should not be emitted after successful refresh"
    );

    let mgr = VulnCacheManager::with_custom_path(cache_path);
    let loaded = mgr
        .load_cache()
        .expect("load")
        .expect("repaired cache exists");
    assert!(
        !loaded.entries.is_empty(),
        "Repaired cache has fresh OSV entries"
    );
}
