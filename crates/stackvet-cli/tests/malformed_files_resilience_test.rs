use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use stackvet_analyzers::run_all_analyzers;
use stackvet_core::{ExitCode, Severity};
use stackvet_node::{detect_node_project_with_diagnostics, parse_node_imports_with_diagnostics};
use stackvet_python::{
    detect_python_project_with_diagnostics, parse_python_imports_with_diagnostics,
};
use stackvet_reporter::ReportSummary;
use stackvet_scanner::ProjectScanner;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn make_temp_repo(name: &str) -> PathBuf {
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "stackvet_test_malformed_{}_{}_{}_{}",
        name,
        std::process::id(),
        unique,
        count
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_malformed_pyproject_toml_emits_disc_004() {
    let repo = make_temp_repo("malformed_pyproject");
    let pyproject = repo.join("pyproject.toml");

    let secret_token = "sk-live-998877665544332211";
    let bad_toml = format!(
        "# Project configuration\napi_key = \"{}\"\n[project\nname = 'broken'\n",
        secret_token
    );
    fs::write(&pyproject, bad_toml).unwrap();

    let (info, diags) = detect_python_project_with_diagnostics(&repo);
    assert!(info.is_none());
    assert!(!diags.is_empty());

    let d = &diags[0];
    assert_eq!(d.rule_id.0, "DISC-004");
    assert_eq!(d.severity, Severity::Error);
    assert!(d.message.contains("pyproject.toml"));

    // Secret leakage check: Error message must never leak raw token strings
    assert!(
        !d.message.contains(secret_token),
        "Diagnostic message must not leak file tokens or secrets"
    );

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_malformed_package_json_emits_disc_004() {
    let repo = make_temp_repo("malformed_package_json");
    let pkg_json = repo.join("package.json");

    let bad_json = r#"{
      "name": "broken-pkg",
      "dependencies": {
        "react": "^18.0.0",
      }
    "#; // Missing closing bracket and unparseable JSON
    fs::write(&pkg_json, bad_json).unwrap();

    let (info, diags) = detect_node_project_with_diagnostics(&repo);
    assert!(info.is_none());
    assert!(!diags.is_empty());

    let d = &diags[0];
    assert_eq!(d.rule_id.0, "DISC-004");
    assert_eq!(d.severity, Severity::Error);
    assert!(d.message.contains("package.json"));

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_python_syntax_error_emits_disc_002_and_does_not_panic() {
    let bad_code = r#"
def invalid_syntax(
    return 123
"#;
    let (imports, diags) = parse_python_imports_with_diagnostics(bad_code, Path::new("broken.py"));
    assert!(imports.is_empty());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].rule_id.0, "DISC-002");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("broken.py"));
}

#[test]
fn test_js_ts_syntax_error_emits_disc_003_and_does_not_panic() {
    let bad_ts = r#"
const foo = ;;;
function broken( {
"#;
    let (imports, diags) = parse_node_imports_with_diagnostics(bad_ts, Path::new("broken.ts"));
    assert!(imports.is_empty());
    assert!(!diags.is_empty());
    assert_eq!(diags[0].rule_id.0, "DISC-003");
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("broken.ts"));
}

#[test]
fn test_mixed_resilience_and_incomplete_summary() {
    let repo = make_temp_repo("mixed_resilience");

    // Valid file
    fs::write(repo.join("good.py"), "import os\nfrom . import local_mod\n").unwrap();

    // Syntax error file
    fs::write(repo.join("bad.py"), "def foo(:\n").unwrap();

    // Malformed manifest
    fs::write(repo.join("pyproject.toml"), "[invalid toml").unwrap();

    let scanner = ProjectScanner::new(&repo);
    let mut ctx = scanner.build_context().expect("Scanner build context");
    let (py_info, py_diags) = detect_python_project_with_diagnostics(&repo);
    ctx.python = py_info;
    ctx.discovery_diagnostics.extend(py_diags);

    // Analysis must not panic or abort
    let all_diags = run_all_analyzers(&ctx, true).expect("Analyzers execution");

    // Must contain DISC-004 and DISC-002
    assert!(
        all_diags.iter().any(|d| d.rule_id.0 == "DISC-004"),
        "Should contain DISC-004 manifest error"
    );
    assert!(
        all_diags.iter().any(|d| d.rule_id.0 == "DISC-002"),
        "Should contain DISC-002 syntax error"
    );

    // Summary must flag is_complete as false
    let summary = ReportSummary::from_diagnostics(&all_diags);
    assert!(!summary.is_complete);
    assert!(!summary.incomplete_reasons.is_empty());

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_cli_exit_code_4_on_incomplete_scan() {
    let repo = make_temp_repo("cli_incomplete_scan");

    // Create a malformed file
    fs::write(repo.join("broken.py"), "def unclosed(\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_stackvet");

    // 1. With --fail-on-incomplete: must exit with code 4
    let output = Command::new(exe)
        .arg("--path")
        .arg(&repo)
        .arg("check")
        .arg("--fail-on-incomplete")
        .output()
        .expect("Run stackvet");

    assert_eq!(
        output.status.code(),
        Some(ExitCode::IncompleteAnalysis.as_i32()),
        "Expected exit code 4 (IncompleteAnalysis)"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Incomplete") || stderr.contains("DISC-"),
        "Expected incomplete notice in stderr"
    );

    let _ = fs::remove_dir_all(&repo);
}
