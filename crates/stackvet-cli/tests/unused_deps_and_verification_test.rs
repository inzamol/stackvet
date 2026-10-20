use stackvet_analyzers::run_all_analyzers;
use stackvet_core::{Confidence, Diagnostic, FixAction, LanguageTarget, Severity};
use stackvet_node::detect_node_project;
use stackvet_python::detect_python_project;
use stackvet_remediation::{FixVerificationResult, RemediationEngine};
use stackvet_scanner::ProjectScanner;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("stackvet-unused-test-{}-{}", name, ts));
        fs::create_dir_all(&path).expect("create test temp dir");
        TestDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn test_python_unused_dependency_high_confidence_with_fix() {
    let temp = TestDir::new("py-unused");
    let root = temp.path();

    fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "demo-pkg"
version = "0.1.0"
dependencies = [
    "requests>=2.28.0",
    "unused-lib>=1.0.0",
]
"#,
    )
    .unwrap();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src").join("main.py"),
        r#"import requests

def main():
    print(requests.__name__)
"#,
    )
    .unwrap();

    let mut ctx = ProjectScanner::new(root).build_context().unwrap();
    ctx.python = detect_python_project(root);

    let diags = run_all_analyzers(&ctx, true).unwrap();
    let unused_diags: Vec<_> = diags.iter().filter(|d| d.rule_id.0 == "DEP-003").collect();

    assert_eq!(
        unused_diags.len(),
        1,
        "Expected 1 unused dependency finding"
    );
    let diag = unused_diags[0];
    assert!(diag.message.contains("unused-lib"));
    assert_eq!(diag.confidence, Confidence::High);
    assert!(
        diag.fix.is_some(),
        "High confidence finding must include FixAction"
    );

    let fix = diag.fix.as_ref().unwrap();
    assert!(fix.description.to_lowercase().contains("remove unused-lib"));

    // Apply the fix and verify removal
    let engine = RemediationEngine::new();
    let report = engine
        .apply_fixes_transactional_with_diagnostics(
            std::slice::from_ref(fix),
            Some(root),
            Some(&diags),
        )
        .unwrap();
    assert_eq!(report.applied_count, 1);

    let updated_toml = fs::read_to_string(root.join("pyproject.toml")).unwrap();
    assert!(!updated_toml.contains("unused-lib"));
    assert!(updated_toml.contains("requests>=2.28.0"));

    // Rescan and verify
    let mut post_ctx = ProjectScanner::new(root).build_context().unwrap();
    post_ctx.python = detect_python_project(root);
    let post_diags = run_all_analyzers(&post_ctx, true).unwrap();
    let verify = FixVerificationResult::verify(&diags, &post_diags, std::slice::from_ref(fix));
    assert!(
        verify.is_clean,
        "Verification should be clean after removing unused dependency"
    );
    assert_eq!(verify.resolved_diagnostics.len(), 1);
    assert_eq!(verify.new_regressions.len(), 0);
}

#[test]
fn test_python_dynamic_literal_usage_not_flagged() {
    let temp = TestDir::new("py-dynamic");
    let root = temp.path();

    fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "dynamic-app"
version = "0.1.0"
dependencies = [
    "redis>=4.0.0",
]
"#,
    )
    .unwrap();

    fs::write(
        root.join("settings.py"),
        r#"
BROKER_URL = "redis://localhost:6379/0"
"#,
    )
    .unwrap();

    let mut ctx = ProjectScanner::new(root).build_context().unwrap();
    ctx.python = detect_python_project(root);

    let diags = run_all_analyzers(&ctx, true).unwrap();
    let unused_diags: Vec<_> = diags.iter().filter(|d| d.rule_id.0 == "DEP-003").collect();
    assert!(
        unused_diags.is_empty(),
        "Dependency referenced in string literal must not be flagged as unused"
    );
}

#[test]
fn test_python_plugin_medium_confidence_without_fix() {
    let temp = TestDir::new("py-plugin");
    let root = temp.path();

    fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "django-app"
version = "0.1.0"
dependencies = [
    "django-cors-headers>=3.0.0",
]
"#,
    )
    .unwrap();

    fs::write(
        root.join("app.py"),
        r#"
print("Hello world")
"#,
    )
    .unwrap();

    let mut ctx = ProjectScanner::new(root).build_context().unwrap();
    ctx.python = detect_python_project(root);

    let diags = run_all_analyzers(&ctx, true).unwrap();
    let unused_diags: Vec<_> = diags.iter().filter(|d| d.rule_id.0 == "DEP-003").collect();

    assert_eq!(unused_diags.len(), 1);
    let diag = unused_diags[0];
    assert_eq!(diag.confidence, Confidence::Medium);
    assert!(
        diag.fix.is_none(),
        "Plugin/driver candidate with Medium confidence must NOT include automated FixAction"
    );
    assert!(diag.explanation.contains("known plugin"));
}

#[test]
fn test_node_unused_dependency_high_confidence_with_fix() {
    let temp = TestDir::new("node-unused");
    let root = temp.path();

    fs::write(
        root.join("package.json"),
        r#"{
  "name": "node-app",
  "version": "1.0.0",
  "dependencies": {
    "chalk": "^5.0.0",
    "dead-package": "^1.0.0"
  }
}
"#,
    )
    .unwrap();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src").join("index.js"),
        r#"import chalk from 'chalk';
console.log(chalk.blue('hello'));
"#,
    )
    .unwrap();

    let mut ctx = ProjectScanner::new(root).build_context().unwrap();
    ctx.node = detect_node_project(root);

    let diags = run_all_analyzers(&ctx, true).unwrap();
    let unused: Vec<_> = diags.iter().filter(|d| d.rule_id.0 == "NDEP-002").collect();

    assert_eq!(unused.len(), 1);
    let diag = unused[0];
    assert!(diag.message.contains("dead-package"));
    assert_eq!(diag.confidence, Confidence::High);
    assert!(diag.fix.is_some());

    let fix = diag.fix.as_ref().unwrap();
    let engine = RemediationEngine::new();
    let report = engine
        .apply_fixes_transactional_with_diagnostics(
            std::slice::from_ref(fix),
            Some(root),
            Some(&diags),
        )
        .unwrap();
    assert_eq!(report.applied_count, 1);

    let updated_json = fs::read_to_string(root.join("package.json")).unwrap();
    assert!(!updated_json.contains("dead-package"));
    assert!(updated_json.contains("chalk"));

    // Verify
    let mut post_ctx = ProjectScanner::new(root).build_context().unwrap();
    post_ctx.node = detect_node_project(root);
    let post_diags = run_all_analyzers(&post_ctx, true).unwrap();
    let verify = FixVerificationResult::verify(&diags, &post_diags, std::slice::from_ref(fix));
    assert!(verify.is_clean);
    assert_eq!(verify.resolved_diagnostics.len(), 1);
}

#[test]
fn test_node_scripts_usage_not_flagged() {
    let temp = TestDir::new("node-scripts");
    let root = temp.path();

    fs::write(
        root.join("package.json"),
        r#"{
  "name": "node-scripts-app",
  "version": "1.0.0",
  "scripts": {
    "migrate": "prisma migrate dev",
    "build": "tsc"
  },
  "dependencies": {
    "prisma": "^5.0.0"
  }
}
"#,
    )
    .unwrap();

    fs::write(root.join("index.ts"), "export const x = 1;\n").unwrap();

    let mut ctx = ProjectScanner::new(root).build_context().unwrap();
    ctx.node = detect_node_project(root);

    let diags = run_all_analyzers(&ctx, true).unwrap();
    let unused: Vec<_> = diags.iter().filter(|d| d.rule_id.0 == "NDEP-002").collect();
    assert!(
        unused.is_empty(),
        "Dependency used in package.json scripts must not be flagged as unused"
    );
}

#[test]
fn test_node_plugin_medium_confidence_without_fix() {
    let temp = TestDir::new("node-plugin");
    let root = temp.path();

    fs::write(
        root.join("package.json"),
        r#"{
  "name": "plugin-app",
  "version": "1.0.0",
  "dependencies": {
    "eslint-plugin-react": "^7.30.0"
  }
}
"#,
    )
    .unwrap();

    fs::write(root.join("index.js"), "console.log('hi');\n").unwrap();

    let mut ctx = ProjectScanner::new(root).build_context().unwrap();
    ctx.node = detect_node_project(root);

    let diags = run_all_analyzers(&ctx, true).unwrap();
    let unused: Vec<_> = diags.iter().filter(|d| d.rule_id.0 == "NDEP-002").collect();

    assert_eq!(unused.len(), 1);
    let diag = unused[0];
    assert_eq!(diag.confidence, Confidence::Medium);
    assert!(
        diag.fix.is_none(),
        "Tool plugin must not have automated removal FixAction"
    );
}

#[test]
fn test_fix_verification_detects_regressions_and_unresolved() {
    let p1 = PathBuf::from("src/main.rs");
    let p2 = PathBuf::from("src/lib.rs");

    let pre_diag = Diagnostic::builder("DEP-003", Severity::Warning, LanguageTarget::Python)
        .file(&p1)
        .message("Unused dep")
        .fix(FixAction::new("Fix dep", p1.clone(), "foo"))
        .build();

    let existing_diag = Diagnostic::builder("DEP-001", Severity::Error, LanguageTarget::Python)
        .file(&p2)
        .message("Syntax error")
        .build();

    let regression_diag = Diagnostic::builder("DEP-002", Severity::Error, LanguageTarget::Python)
        .file(&p1)
        .message("New regression error")
        .build();

    let targeted = vec![FixAction::new("Fix dep", p1.clone(), "foo")];

    // Case 1: Issue resolved, regression introduced
    let pre = vec![pre_diag.clone(), existing_diag.clone()];
    let post = vec![existing_diag.clone(), regression_diag.clone()];

    let res = FixVerificationResult::verify(&pre, &post, &targeted);
    assert!(!res.is_clean);
    assert_eq!(res.resolved_diagnostics.len(), 1);
    assert_eq!(res.resolved_diagnostics[0].rule_id.0, "DEP-003");
    assert_eq!(res.new_regressions.len(), 1);
    assert_eq!(res.new_regressions[0].rule_id.0, "DEP-002");

    // Case 2: Issue unresolved (still present in post)
    let post_unresolved = vec![pre_diag.clone(), existing_diag.clone()];
    let res_unresolved = FixVerificationResult::verify(&pre, &post_unresolved, &targeted);
    assert!(!res_unresolved.is_clean);
    assert_eq!(res_unresolved.unresolved_diagnostics.len(), 1);
    assert_eq!(res_unresolved.resolved_diagnostics.len(), 0);
}
