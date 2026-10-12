use stackvet_core::{Diagnostic, DiagnosticLocation, LanguageTarget, RuleId, Severity};
use stackvet_correlation::{calculate_health_score, score_grade, CorrelationEngine};
use stackvet_reporter::Reporter;

#[test]
fn test_doctor_full_flow() {
    let diags = vec![
        Diagnostic {
            rule_id: RuleId::new("FAP-001"),
            message: "Blocking call 'time.sleep()' inside async handler".to_string(),
            severity: Severity::Critical,
            location: DiagnosticLocation::new("backend/app.py"),
            explanation: "Event loop freeze".to_string(),
            suggested_remediation: "Use await asyncio.sleep()".to_string(),
            fix: None,
            language: LanguageTarget::Python,
        },
        Diagnostic {
            rule_id: RuleId::new("DEP-002"),
            message: "Missing requests in pyproject.toml".to_string(),
            severity: Severity::Error,
            location: DiagnosticLocation::new("backend/app.py"),
            explanation: "Runtime ModuleNotFoundError".to_string(),
            suggested_remediation: "Add requests to dependencies".to_string(),
            fix: None,
            language: LanguageTarget::Python,
        },
    ];

    let score = calculate_health_score(&diags);
    assert_eq!(score, 72); // 100 - 20 - 8 = 72
    assert_eq!(score_grade(score), "C (Degraded)");

    let correlator = CorrelationEngine::new();
    let root_causes = correlator.correlate(diags.clone());
    assert!(!root_causes.is_empty());

    let reporter = Reporter::new();
    let text = reporter.render_text(score, score_grade(score), &root_causes, &diags);
    assert!(text.contains("StackVet Universal Project Health Report"));
    assert!(text.contains("Health Score:"));
    assert!(text.contains("72"));
    assert!(text.contains("/ 100"));

    let sarif = reporter.render_sarif(&diags).unwrap();
    assert!(sarif.contains("FAP-001"));
    assert!(sarif.contains("sarif-schema-2.1.0.json"));

    let html = reporter.render_html(score, score_grade(score), &root_causes, &diags);
    assert!(html.contains("StackVet Project Health Report"));
    assert!(html.contains("72"));
}

#[test]
fn test_remediation_preview_and_rollback_flow() {
    use stackvet_core::FixAction;
    use stackvet_remediation::RemediationEngine;

    let test_dir = std::env::temp_dir().join(format!(
        "stackvet_cli_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&test_dir).unwrap();

    let pyproject_path = test_dir.join("pyproject.toml");
    let original_pyproject = r#"# Main pyproject
[project]
name = "demo"
dependencies = [
    # Existing dependency
    "click>=8.0",
]
"#;
    std::fs::write(&pyproject_path, original_pyproject).unwrap();

    let pkg_json_path = test_dir.join("package.json");
    let original_pkg_json = r#"{
  "name": "demo-frontend",
  "dependencies": {
    "react": "^18.0.0"
  }
}
"#;
    std::fs::write(&pkg_json_path, original_pkg_json).unwrap();

    let engine = RemediationEngine::new();
    let fixes = vec![
        FixAction::new("Add fastapi", &pyproject_path, "fastapi"),
        FixAction::new("Add axios", &pkg_json_path, "axios"),
    ];

    // 1. Preview
    let previews = engine.preview_fixes(&fixes).unwrap();
    assert_eq!(previews.len(), 2);
    for p in &previews {
        assert!(p.has_changes);
        assert!(!p.diff.is_empty());
    }

    // 2. Transactional Apply
    let report = engine
        .apply_fixes_transactional(&fixes, Some(&test_dir))
        .unwrap();
    assert_eq!(report.applied_count, 2);
    assert!(engine.has_rollback_session(&test_dir));

    let modified_py = std::fs::read_to_string(&pyproject_path).unwrap();
    assert!(modified_py.contains("\"fastapi\""));
    assert!(modified_py.contains("# Main pyproject"));
    assert!(modified_py.contains("# Existing dependency"));

    let modified_pkg = std::fs::read_to_string(&pkg_json_path).unwrap();
    assert!(modified_pkg.contains("\"axios\""));

    // 3. Rollback
    let restored = engine.rollback_session(&test_dir).unwrap();
    assert_eq!(restored.len(), 2);
    assert!(!engine.has_rollback_session(&test_dir));

    assert_eq!(
        std::fs::read_to_string(&pyproject_path).unwrap(),
        original_pyproject
    );
    assert_eq!(
        std::fs::read_to_string(&pkg_json_path).unwrap(),
        original_pkg_json
    );

    let _ = std::fs::remove_dir_all(&test_dir);
}

