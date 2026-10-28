use stackvet_analyzers::run_all_analyzers;
use stackvet_core::{Confidence, Diagnostic, ProjectContext, Severity};
use stackvet_node::detect_node_project;
use stackvet_python::detect_python_project;
use stackvet_remediation::{FixVerificationResult, RemediationEngine};
use stackvet_scanner::ProjectScanner;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempTestDir {
    path: PathBuf,
}

impl TempTestDir {
    fn new(prefix: &str) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("stackvet-fix-test-{}-{}", prefix, ts));
        fs::create_dir_all(&path).expect("create temp dir");
        TempTestDir { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn scan_repo(root: &Path) -> (ProjectContext, Vec<Diagnostic>) {
    let scanner = ProjectScanner::new(root).with_parents(false);
    let mut ctx = scanner.build_context().expect("build context");
    ctx.python = detect_python_project(root);
    ctx.node = detect_node_project(root);
    let diags = run_all_analyzers(&ctx, true).expect("run analyzers");
    (ctx, diags)
}

#[test]
fn test_safe_fix_pyproject_pep621_preserves_comments_and_resolves_finding() {
    let temp = TempTestDir::new("pyproject-pep621");
    let root = temp.path();

    let original_toml = r#"# Top-level comment
[project]
name = "pep621-app"
version = "0.1.0"
# Inline comment before dependencies
dependencies = [
    # Core dependency comment
    "urllib3>=1.26.0",
]

[build-system]
requires = ["setuptools"]
"#;

    fs::write(root.join("pyproject.toml"), original_toml).unwrap();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src").join("main.py"),
        r#"import urllib3
import fastapi

app = fastapi.FastAPI()
"#,
    )
    .unwrap();

    // 1. Initial scan: DEP-002 for missing fastapi
    let (_ctx, pre_diags) = scan_repo(root);
    let missing: Vec<_> = pre_diags
        .iter()
        .filter(|d| d.rule_id.0 == "DEP-002" && d.message.contains("fastapi"))
        .collect();
    assert_eq!(missing.len(), 1, "Expected 1 DEP-002 for fastapi");
    assert_eq!(missing[0].severity, Severity::Error);
    assert_eq!(missing[0].confidence, Confidence::High);

    let fix = missing[0].fix.as_ref().expect("FixAction must be present");
    assert_eq!(fix.replacement, "fastapi");

    // 2. Apply fix transactionally
    let engine = RemediationEngine::new();
    let report = engine
        .apply_fixes_transactional_with_diagnostics(
            std::slice::from_ref(fix),
            Some(root),
            Some(&pre_diags),
        )
        .expect("apply fix");
    assert_eq!(report.applied_count, 1);

    // 3. Post-fix scan: verify resolution and zero regressions
    let (_post_ctx, post_diags) = scan_repo(root);
    let verify = FixVerificationResult::verify(&pre_diags, &post_diags, std::slice::from_ref(fix));
    assert!(
        verify.is_clean,
        "Post-fix verification must be clean: {:?}",
        verify
    );
    assert_eq!(verify.resolved_diagnostics.len(), 1);
    assert_eq!(verify.resolved_diagnostics[0].rule_id.0, "DEP-002");
    assert!(verify.new_regressions.is_empty());

    // 4. File preservation check: comments and existing entries preserved
    let updated_toml = fs::read_to_string(root.join("pyproject.toml")).unwrap();
    assert!(updated_toml.contains("# Top-level comment"));
    assert!(updated_toml.contains("# Inline comment before dependencies"));
    assert!(updated_toml.contains("# Core dependency comment"));
    assert!(updated_toml.contains("\"urllib3>=1.26.0\""));
    assert!(updated_toml.contains("\"fastapi\""));

    // 5. Idempotency check: re-applying the same fix must produce 0 changes
    let second_report = engine
        .apply_fixes_transactional_with_diagnostics(
            std::slice::from_ref(fix),
            Some(root),
            Some(&post_diags),
        )
        .expect("re-apply fix");
    assert_eq!(second_report.applied_count, 0);
    assert_eq!(
        fs::read_to_string(root.join("pyproject.toml")).unwrap(),
        updated_toml
    );
}

#[test]
fn test_safe_fix_pyproject_poetry_preserves_structure_and_resolves_finding() {
    let temp = TempTestDir::new("pyproject-poetry");
    let root = temp.path();

    let original_toml = r#"[tool.poetry]
name = "poetry-app"
version = "0.1.0"

[tool.poetry.dependencies]
python = "^3.11"
# Existing dependency
requests = "^2.28.0"
"#;

    fs::write(root.join("pyproject.toml"), original_toml).unwrap();

    fs::write(
        root.join("app.py"),
        r#"import requests
import pydantic

class Model(pydantic.BaseModel):
    name: str
"#,
    )
    .unwrap();

    let (_ctx, pre_diags) = scan_repo(root);
    let missing: Vec<_> = pre_diags
        .iter()
        .filter(|d| d.rule_id.0 == "DEP-002" && d.message.contains("pydantic"))
        .collect();
    assert_eq!(missing.len(), 1);

    let fix = missing[0].fix.as_ref().expect("FixAction must be present");
    let engine = RemediationEngine::new();
    let report = engine
        .apply_fixes_transactional_with_diagnostics(
            std::slice::from_ref(fix),
            Some(root),
            Some(&pre_diags),
        )
        .unwrap();
    assert_eq!(report.applied_count, 1);

    let (_post_ctx, post_diags) = scan_repo(root);
    let verify = FixVerificationResult::verify(&pre_diags, &post_diags, std::slice::from_ref(fix));
    assert!(verify.is_clean);

    let updated_toml = fs::read_to_string(root.join("pyproject.toml")).unwrap();
    assert!(updated_toml.contains("# Existing dependency"));
    assert!(updated_toml.contains("requests = \"^2.28.0\""));
    assert!(updated_toml.contains("pydantic = \"*\""));
}

#[test]
fn test_safe_fix_requirements_txt_preserves_comments_and_resolves_finding() {
    let temp = TempTestDir::new("requirements-txt");
    let root = temp.path();

    let original_reqs = r#"# Production dependencies
flask>=2.0.0

# Database connector
psycopg2-binary>=2.9.0
"#;

    fs::write(root.join("requirements.txt"), original_reqs).unwrap();
    fs::write(
        root.join("app.py"),
        r#"import flask
import psycopg2
import httpx

client = httpx.Client()
"#,
    )
    .unwrap();

    let (_ctx, pre_diags) = scan_repo(root);
    let missing: Vec<_> = pre_diags
        .iter()
        .filter(|d| d.rule_id.0 == "DEP-002" && d.message.contains("httpx"))
        .collect();
    assert_eq!(missing.len(), 1);

    let fix = missing[0].fix.as_ref().expect("FixAction present");
    let engine = RemediationEngine::new();
    let report = engine
        .apply_fixes_transactional_with_diagnostics(
            std::slice::from_ref(fix),
            Some(root),
            Some(&pre_diags),
        )
        .unwrap();
    assert_eq!(report.applied_count, 1);

    let (_post_ctx, post_diags) = scan_repo(root);
    let verify = FixVerificationResult::verify(&pre_diags, &post_diags, std::slice::from_ref(fix));
    assert!(verify.is_clean);

    let updated_reqs = fs::read_to_string(root.join("requirements.txt")).unwrap();
    assert!(updated_reqs.contains("# Production dependencies"));
    assert!(updated_reqs.contains("flask>=2.0.0"));
    assert!(updated_reqs.contains("# Database connector"));
    assert!(updated_reqs.contains("psycopg2-binary>=2.9.0"));
    assert!(updated_reqs.contains("httpx"));
}

#[test]
fn test_safe_fix_package_json_preserves_formatting_and_resolves_finding() {
    let temp = TempTestDir::new("package-json-node");
    let root = temp.path();

    let original_pkg = r#"{
  "name": "node-app",
  "version": "1.0.0",
  "dependencies": {
    "chalk": "^5.0.0"
  }
}
"#;

    fs::write(root.join("package.json"), original_pkg).unwrap();
    fs::write(
        root.join("index.ts"),
        r#"import chalk from 'chalk';
import lodash from 'lodash';

console.log(chalk.green(lodash.capitalize('success')));
"#,
    )
    .unwrap();

    let (_ctx, pre_diags) = scan_repo(root);
    let missing: Vec<_> = pre_diags
        .iter()
        .filter(|d| d.rule_id.0 == "NDEP-001" && d.message.contains("lodash"))
        .collect();
    assert_eq!(missing.len(), 1);

    let fix = missing[0].fix.as_ref().expect("FixAction present");
    let engine = RemediationEngine::new();
    let report = engine
        .apply_fixes_transactional_with_diagnostics(
            std::slice::from_ref(fix),
            Some(root),
            Some(&pre_diags),
        )
        .unwrap();
    assert_eq!(report.applied_count, 1);

    let (_post_ctx, post_diags) = scan_repo(root);
    let verify = FixVerificationResult::verify(&pre_diags, &post_diags, std::slice::from_ref(fix));
    assert!(verify.is_clean);

    let updated_pkg = fs::read_to_string(root.join("package.json")).unwrap();
    assert!(updated_pkg.contains("\"chalk\": \"^5.0.0\""));
    assert!(updated_pkg.contains("\"lodash\": \"*\""));
    assert!(updated_pkg.contains("\"name\": \"node-app\""));
    assert!(updated_pkg.ends_with('\n'));
    let parsed: serde_json::Value = serde_json::from_str(&updated_pkg).expect("valid JSON");
    assert_eq!(parsed["dependencies"]["lodash"], "*");
    assert_eq!(parsed["dependencies"]["chalk"], "^5.0.0");
}

#[test]
fn test_safe_fix_unused_dependency_removal_resolves_and_preserves() {
    let temp = TempTestDir::new("pyproject-unused-dep");
    let root = temp.path();

    let original_toml = r#"[project]
name = "unused-demo"
version = "0.1.0"
dependencies = [
    # Kept dependency
    "click>=8.0.0",
    # Target to remove
    "unused-library>=2.0.0",
]
"#;

    fs::write(root.join("pyproject.toml"), original_toml).unwrap();
    fs::write(root.join("uv.lock"), "version = 1\nrevision = 1\n[[package]]\nname = \"click\"\nversion = \"8.0.0\"\n[[package]]\nname = \"unused-library\"\nversion = \"2.0.0\"\n").unwrap();

    fs::write(
        root.join("main.py"),
        r#"import click

@click.command()
def cli():
    pass
"#,
    )
    .unwrap();

    let (_ctx, pre_diags) = scan_repo(root);
    let unused: Vec<_> = pre_diags
        .iter()
        .filter(|d| d.rule_id.0 == "DEP-003" && d.message.contains("unused-library"))
        .collect();
    assert_eq!(unused.len(), 1);

    let fix = unused[0].fix.as_ref().expect("FixAction present");
    let engine = RemediationEngine::new();
    let report = engine
        .apply_fixes_transactional_with_diagnostics(
            std::slice::from_ref(fix),
            Some(root),
            Some(&pre_diags),
        )
        .unwrap();
    assert_eq!(report.applied_count, 1);

    let (_post_ctx, post_diags) = scan_repo(root);
    let verify = FixVerificationResult::verify(&pre_diags, &post_diags, std::slice::from_ref(fix));
    assert!(verify.is_clean);

    let updated_toml = fs::read_to_string(root.join("pyproject.toml")).unwrap();
    assert!(!updated_toml.contains("unused-library"));
    assert!(updated_toml.contains("click>=8.0.0"));
    assert!(updated_toml.contains("# Kept dependency"));
}

#[test]
fn test_ambiguous_and_unsafe_cases_skipped_without_fix_action() {
    let temp = TempTestDir::new("ambiguous-driver");
    let root = temp.path();

    fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "ambiguous-app"
version = "0.1.0"
dependencies = [
    "django-cors-headers>=3.0.0",
]
"#,
    )
    .unwrap();
    fs::write(root.join("uv.lock"), "version = 1\nrevision = 1\n[[package]]\nname = \"django-cors-headers\"\nversion = \"3.0.0\"\n").unwrap();

    fs::write(
        root.join("main.py"),
        "print('no direct import of cors headers')\n",
    )
    .unwrap();

    let (_ctx, diags) = scan_repo(root);
    let unused: Vec<_> = diags
        .iter()
        .filter(|d| d.rule_id.0 == "DEP-003" && d.message.contains("django-cors-headers"))
        .collect();

    assert_eq!(unused.len(), 1);
    let diag = unused[0];
    assert_eq!(diag.confidence, Confidence::Medium);
    assert!(
        diag.fix.is_none(),
        "Ambiguous/plugin dependencies must NOT attach automated fix: {:?}",
        diag
    );
}

#[test]
fn test_cli_fix_command_executes_safely_and_creates_rollback() {
    let temp = TempTestDir::new("cli-fix-command");
    let root = temp.path();

    fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "cli-fix-app"
version = "0.1.0"
dependencies = [
    "urllib3>=1.26.0",
]
"#,
    )
    .unwrap();
    fs::write(root.join("uv.lock"), "version = 1\nrevision = 1\n[[package]]\nname = \"urllib3\"\nversion = \"1.26.0\"\n[[package]]\nname = \"click\"\nversion = \"8.0.0\"\n").unwrap();

    fs::write(
        root.join("main.py"),
        r#"import urllib3
import click

@click.command()
def run():
    pass
"#,
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_stackvet");

    // 1. Run stackvet fix --yes
    let out = Command::new(exe)
        .arg("--path")
        .arg(root)
        .arg("--offline")
        .arg("fix")
        .arg("--yes")
        .output()
        .expect("run stackvet fix");

    assert!(
        out.status.success(),
        "stackvet fix must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 2. pyproject.toml should now contain click
    let fixed_toml = fs::read_to_string(root.join("pyproject.toml")).unwrap();
    assert!(fixed_toml.contains("\"click\""));

    // 3. Rollback snapshot must exist
    let engine = RemediationEngine::new();
    assert!(
        engine.has_rollback_session(root),
        "Rollback session must be preserved on disk"
    );

    // 4. Verify post-fix check is clean
    let check_out = Command::new(exe)
        .arg("--path")
        .arg(root)
        .arg("--format")
        .arg("json")
        .arg("check")
        .output()
        .expect("run stackvet check");

    let check_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&check_out.stdout)).unwrap();
    let has_dep_002 = check_json["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["rule_id"].as_str() == Some("DEP-002"));
    assert!(
        !has_dep_002,
        "DEP-002 must be resolved in post-fix scan: {:?}",
        check_json["diagnostics"]
    );
}
