use stackvet_analyzers::run_all_analyzers;
use stackvet_core::{Diagnostic, Severity};
use stackvet_frameworks::run_all_framework_analyzers;
use stackvet_node::detect_node_project;
use stackvet_python::detect_python_project;
use stackvet_scanner::ProjectScanner;
use std::collections::HashSet;
use std::path::PathBuf;

fn analyze_fixture(rel_path: &str) -> Vec<Diagnostic> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root");
    let fixture_path = workspace_root
        .join("tests")
        .join("fixtures")
        .join("regression")
        .join(rel_path);

    assert!(
        fixture_path.exists(),
        "Fixture path does not exist: {}",
        fixture_path.display()
    );

    let scanner = ProjectScanner::new(&fixture_path).with_parents(false);
    let mut ctx = scanner.build_context().expect("Failed to build context");
    ctx.python = detect_python_project(&fixture_path);
    ctx.node = detect_node_project(&fixture_path);

    let mut diags = run_all_analyzers(&ctx, true).expect("Failed to run core analyzers");
    let mut fw_diags =
        run_all_framework_analyzers(&ctx).expect("Failed to run framework analyzers");
    diags.append(&mut fw_diags);
    diags
}

#[test]
fn test_fastapi_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/fastapi_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(
        rule_ids.contains("FAP-001"),
        "Expected FAP-001 (blocking sleep)"
    );
    assert!(
        rule_ids.contains("FAP-003"),
        "Expected FAP-003 (missing response_model)"
    );
    assert!(
        rule_ids.contains("FAP-004"),
        "Expected FAP-004 (insecure CORS)"
    );
    assert!(
        rule_ids.contains("FAP-006"),
        "Expected FAP-006 (path parameter mismatch)"
    );
    assert!(
        rule_ids.contains("FAP-008"),
        "Expected FAP-008 (deprecated on_event)"
    );
    assert!(
        rule_ids.contains("FAP-010"),
        "Expected FAP-010 (duplicate route)"
    );
    assert!(
        rule_ids.contains("FAP-011"),
        "Expected FAP-011 (mutable default argument)"
    );
    assert!(
        rule_ids.contains("FAP-015"),
        "Expected FAP-015 (sync file I/O in async route)"
    );

    // Verify severities
    let fap001 = diags.iter().find(|d| d.rule_id.0 == "FAP-001").unwrap();
    assert_eq!(fap001.severity, Severity::Critical);

    let fap004 = diags.iter().find(|d| d.rule_id.0 == "FAP-004").unwrap();
    assert_eq!(fap004.severity, Severity::Critical);

    let fap006 = diags.iter().find(|d| d.rule_id.0 == "FAP-006").unwrap();
    assert_eq!(fap006.severity, Severity::Error);

    // Verify locations
    assert!(fap001
        .location
        .file_path
        .to_string_lossy()
        .contains("main.py"));
    assert!(fap001.location.span.is_some());
}

#[test]
fn test_fastapi_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_fastapi");
    let fap_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("FAP"))
        .collect();
    assert!(
        fap_diags.is_empty(),
        "Expected 0 FAP false-positive findings, found: {:?}",
        fap_diags
    );
}

#[test]
fn test_sqlalchemy_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/sqlalchemy_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(rule_ids.contains("SQL-001"), "Expected SQL-001 (N+1 query)");
    assert!(
        rule_ids.contains("SQL-004"),
        "Expected SQL-004 (SQL injection)"
    );
    assert!(
        rule_ids.contains("SQL-005"),
        "Expected SQL-005 (legacy query)"
    );
    assert!(
        rule_ids.contains("SQL-008"),
        "Expected SQL-008 (missing pool_pre_ping)"
    );
    assert!(
        rule_ids.contains("SQL-010"),
        "Expected SQL-010 (hardcoded credentials)"
    );
    assert!(
        rule_ids.contains("SQL-014"),
        "Expected SQL-014 (deprecated declarative_base)"
    );
    assert!(
        rule_ids.contains("SQL-025"),
        "Expected SQL-025 (engine.execute)"
    );

    let sql004 = diags.iter().find(|d| d.rule_id.0 == "SQL-004").unwrap();
    assert_eq!(sql004.severity, Severity::Critical);

    let sql010 = diags.iter().find(|d| d.rule_id.0 == "SQL-010").unwrap();
    assert_eq!(sql010.severity, Severity::Critical);
}

#[test]
fn test_sqlalchemy_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_sqlalchemy");
    let sql_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("SQL"))
        .collect();
    assert!(
        sql_diags.is_empty(),
        "Expected 0 SQL false-positive findings, found: {:?}",
        sql_diags
    );
}

#[test]
fn test_django_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/django_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(
        rule_ids.contains("DJG-001"),
        "Expected DJG-001 (DEBUG = True)"
    );
    assert!(
        rule_ids.contains("DJG-002"),
        "Expected DJG-002 (insecure SECRET_KEY)"
    );
    assert!(
        rule_ids.contains("DJG-004"),
        "Expected DJG-004 (insecure cookies)"
    );
    assert!(
        rule_ids.contains("DJG-006"),
        "Expected DJG-006 (@csrf_exempt)"
    );

    let djg002 = diags.iter().find(|d| d.rule_id.0 == "DJG-002").unwrap();
    assert_eq!(djg002.severity, Severity::Critical);
}

#[test]
fn test_django_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_django");
    let djg_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("DJG"))
        .collect();
    assert!(
        djg_diags.is_empty(),
        "Expected 0 DJG false-positive findings, found: {:?}",
        djg_diags
    );
}

#[test]
fn test_celery_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/celery_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(
        rule_ids.contains("CEL-001"),
        "Expected CEL-001 (.apply synchronous call)"
    );
    assert!(
        rule_ids.contains("CEL-003"),
        "Expected CEL-003 (missing autoretry_for)"
    );
    assert!(
        rule_ids.contains("CEL-004"),
        "Expected CEL-004 (missing time_limit)"
    );
}

#[test]
fn test_celery_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_celery");
    let cel_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("CEL"))
        .collect();
    assert!(
        cel_diags.is_empty(),
        "Expected 0 CEL false-positive findings, found: {:?}",
        cel_diags
    );
}

#[test]
fn test_docker_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/docker_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(
        rule_ids.contains("DOC-001"),
        "Expected DOC-001 (layer caching)"
    );
    assert!(
        rule_ids.contains("DOC-002"),
        "Expected DOC-002 (unpinned image tag)"
    );
    assert!(
        rule_ids.contains("DOC-003"),
        "Expected DOC-003 (missing .dockerignore)"
    );
    assert!(
        rule_ids.contains("DOC-004"),
        "Expected DOC-004 (running as root user)"
    );
    assert!(
        rule_ids.contains("DOC-005"),
        "Expected DOC-005 (missing HEALTHCHECK)"
    );
    assert!(
        rule_ids.contains("DOC-006"),
        "Expected DOC-006 (secret in ENV)"
    );
    assert!(
        rule_ids.contains("DOC-007"),
        "Expected DOC-007 (apt-get without clean)"
    );
    assert!(
        rule_ids.contains("DOC-008"),
        "Expected DOC-008 (pip without --no-cache-dir)"
    );
    assert!(
        rule_ids.contains("DOC-009"),
        "Expected DOC-009 (missing PYTHONUNBUFFERED)"
    );
    assert!(
        rule_ids.contains("DOC-010"),
        "Expected DOC-010 (missing PYTHONDONTWRITEBYTECODE)"
    );
    assert!(
        rule_ids.contains("DOC-011"),
        "Expected DOC-011 (web server without EXPOSE)"
    );
    assert!(
        rule_ids.contains("DOC-012"),
        "Expected DOC-012 (ADD instead of COPY)"
    );
    assert!(
        rule_ids.contains("DOC-013"),
        "Expected DOC-013 (sudo installed)"
    );

    let doc006 = diags.iter().find(|d| d.rule_id.0 == "DOC-006").unwrap();
    assert_eq!(doc006.severity, Severity::Critical);
}

#[test]
fn test_docker_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_docker");
    let doc_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("DOC"))
        .collect();
    assert!(
        doc_diags.is_empty(),
        "Expected 0 DOC false-positive findings, found: {:?}",
        doc_diags
    );
}

#[test]
fn test_env_drift_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/env_drift_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(
        rule_ids.contains("ENV-001"),
        "Expected ENV-001 (.python-version / .nvmrc drift)"
    );
    assert!(
        rule_ids.contains("ENV-002"),
        "Expected ENV-002 (Dockerfile runtime drift)"
    );
    assert!(
        rule_ids.contains("ENV-003"),
        "Expected ENV-003 (CI matrix runtime drift)"
    );

    let env002 = diags.iter().find(|d| d.rule_id.0 == "ENV-002").unwrap();
    assert_eq!(env002.severity, Severity::Error);

    let env003 = diags.iter().find(|d| d.rule_id.0 == "ENV-003").unwrap();
    assert_eq!(env003.severity, Severity::Error);
}

#[test]
fn test_env_drift_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_env");
    let env_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("ENV"))
        .collect();
    assert!(
        env_diags.is_empty(),
        "Expected 0 ENV false-positive findings, found: {:?}",
        env_diags
    );
}

#[test]
fn test_imports_arch_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/imports_arch_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(
        rule_ids.contains("IMP-001"),
        "Expected IMP-001 (circular import cycle)"
    );
    assert!(
        rule_ids.contains("IMP-002"),
        "Expected IMP-002 (unresolved relative import)"
    );
    assert!(
        rule_ids.contains("IMP-003"),
        "Expected IMP-003 (orphan dead code file)"
    );
    assert!(
        rule_ids.contains("IMP-004"),
        "Expected IMP-004 (removed stdlib module in PEP 594)"
    );

    let imp001 = diags.iter().find(|d| d.rule_id.0 == "IMP-001").unwrap();
    assert_eq!(imp001.severity, Severity::Error);

    let imp002 = diags.iter().find(|d| d.rule_id.0 == "IMP-002").unwrap();
    assert_eq!(imp002.severity, Severity::Error);

    let imp003 = diags.iter().find(|d| d.rule_id.0 == "IMP-003").unwrap();
    assert_eq!(imp003.severity, Severity::Warning);

    let imp004 = diags.iter().find(|d| d.rule_id.0 == "IMP-004").unwrap();
    assert_eq!(imp004.severity, Severity::Error);
}

#[test]
fn test_imports_arch_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_imports");
    let imp_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("IMP"))
        .collect();
    assert!(
        imp_diags.is_empty(),
        "Expected 0 IMP false-positive findings, found: {:?}",
        imp_diags
    );
}

#[test]
fn test_react_next_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/react_next_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(
        rule_ids.contains("NXT-001"),
        "Expected NXT-001 (hook in server component)"
    );
    assert!(
        rule_ids.contains("NXT-002"),
        "Expected NXT-002 (private secret in client component)"
    );
    assert!(
        rule_ids.contains("RCT-002"),
        "Expected RCT-002 (conditional hook)"
    );
    assert!(
        rule_ids.contains("RCT-004"),
        "Expected RCT-004 (missing key in map)"
    );
    assert!(
        rule_ids.contains("RCT-005"),
        "Expected RCT-005 (barrel import bloat)"
    );

    let nxt002 = diags.iter().find(|d| d.rule_id.0 == "NXT-002").unwrap();
    assert_eq!(nxt002.severity, Severity::Critical);
}

#[test]
fn test_react_next_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_react_next");
    let rct_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("RCT") || d.rule_id.0.starts_with("NXT"))
        .collect();
    assert!(
        rct_diags.is_empty(),
        "Expected 0 RCT/NXT false-positive findings, found: {:?}",
        rct_diags
    );
}

#[test]
fn test_python_deps_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/python_deps_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(
        rule_ids.contains("DEP-002"),
        "Expected DEP-002 (missing dependency)"
    );
    assert!(
        rule_ids.contains("DEP-003"),
        "Expected DEP-003 (unused dependency)"
    );

    let dep002 = diags.iter().find(|d| d.rule_id.0 == "DEP-002").unwrap();
    assert_eq!(dep002.severity, Severity::Error);
    assert!(dep002
        .location
        .file_path
        .to_string_lossy()
        .contains("app.py"));

    let dep003 = diags.iter().find(|d| d.rule_id.0 == "DEP-003").unwrap();
    assert_eq!(dep003.severity, Severity::Warning);
    assert!(dep003
        .location
        .file_path
        .to_string_lossy()
        .contains("pyproject.toml"));
}

#[test]
fn test_python_deps_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_python_deps");
    let dep_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("DEP"))
        .collect();
    assert!(
        dep_diags.is_empty(),
        "Expected 0 DEP false-positive findings, found: {:?}",
        dep_diags
    );
}

#[test]
fn test_node_deps_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/node_deps_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(
        rule_ids.contains("NDEP-001"),
        "Expected NDEP-001 (undeclared package)"
    );
    assert!(
        rule_ids.contains("NDEP-002"),
        "Expected NDEP-002 (unused package)"
    );

    let ndep001 = diags.iter().find(|d| d.rule_id.0 == "NDEP-001").unwrap();
    assert_eq!(ndep001.severity, Severity::Error);
    assert!(ndep001
        .location
        .file_path
        .to_string_lossy()
        .contains("index.ts"));

    let ndep002 = diags.iter().find(|d| d.rule_id.0 == "NDEP-002").unwrap();
    assert_eq!(ndep002.severity, Severity::Warning);
    assert!(ndep002
        .location
        .file_path
        .to_string_lossy()
        .contains("package.json"));
}

#[test]
fn test_node_deps_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_node_deps");
    let ndep_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("NDEP"))
        .collect();
    assert!(
        ndep_diags.is_empty(),
        "Expected 0 NDEP false-positive findings, found: {:?}",
        ndep_diags
    );
}

#[test]
fn test_monorepo_positive_findings_and_severity() {
    let diags = analyze_fixture("positive/monorepo_violations");
    let rule_ids: HashSet<String> = diags.iter().map(|d| d.rule_id.0.clone()).collect();

    assert!(
        rule_ids.contains("NDEP-006"),
        "Expected NDEP-006 (conflicting dependency versions across workspace members)"
    );
    assert!(
        rule_ids.contains("NDEP-004"),
        "Expected NDEP-004 (unresolved workspace dependency)"
    );

    let ndep006 = diags.iter().find(|d| d.rule_id.0 == "NDEP-006").unwrap();
    assert_eq!(ndep006.severity, Severity::Warning);

    let ndep004 = diags.iter().find(|d| d.rule_id.0 == "NDEP-004").unwrap();
    assert_eq!(ndep004.severity, Severity::Warning);
}

#[test]
fn test_monorepo_negative_zero_false_positives() {
    let diags = analyze_fixture("negative/clean_monorepo");
    let ndep_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("NDEP"))
        .collect();
    assert!(
        ndep_diags.is_empty(),
        "Expected 0 NDEP false-positive findings in clean monorepo, found: {:?}",
        ndep_diags
    );
}

#[test]
fn test_python_pep503_normalization_zero_false_positives() {
    let temp_dir = std::env::temp_dir().join(format!(
        "stackvet_pep503_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();

    // pyproject declares canonical names with capital letters and underscores
    std::fs::write(
        temp_dir.join("pyproject.toml"),
        r#"[project]
name = "pep503-app"
version = "0.1.0"
dependencies = [
    "PyYAML>=6.0",
    "typing_extensions>=4.0",
]
"#,
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("uv.lock"),
        r#"version = 1
revision = 1

[[package]]
name = "pyyaml"
version = "6.0.1"

[[package]]
name = "typing-extensions"
version = "4.10.0"
"#,
    )
    .unwrap();

    // app.py imports the canonical module names
    std::fs::write(
        temp_dir.join("app.py"),
        r#"import yaml
import typing_extensions

def parse():
    return yaml.safe_load("a: 1")
"#,
    )
    .unwrap();

    let scanner = ProjectScanner::new(&temp_dir).with_parents(false);
    let mut ctx = scanner.build_context().expect("Failed to build context");
    ctx.python = detect_python_project(&temp_dir);

    let diags = run_all_analyzers(&ctx, true).expect("Failed to run core analyzers");
    let dep_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0.starts_with("DEP"))
        .collect();

    assert!(
        dep_diags.is_empty(),
        "PEP 503 normalized package names must have 0 false-positive findings, found: {:?}",
        dep_diags
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}
