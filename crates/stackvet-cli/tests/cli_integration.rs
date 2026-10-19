use stackvet_core::{Diagnostic, DiagnosticLocation, LanguageTarget, RuleId, Severity};
use stackvet_correlation::{score_grade, CorrelationEngine};
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

    let explanation = stackvet_correlation::calculate_health_score_explained(&diags);
    let score = explanation.final_score;
    assert_eq!(score, 72); // 100 - 20 - 8 = 72
    assert_eq!(score_grade(score), "C (Degraded)");
    assert_eq!(explanation.total_deductions, 28);
    assert_eq!(explanation.unique_findings_count, 2);
    assert_eq!(explanation.duplicate_findings_ignored, 0);

    let correlator = CorrelationEngine::new();
    let root_causes = correlator.correlate(diags.clone());
    assert!(!root_causes.is_empty());

    let reporter = Reporter::new();
    let text = reporter.render_text(
        score,
        score_grade(score),
        &root_causes,
        &diags,
        Some(&explanation),
    );
    assert!(text.contains("StackVet Universal Project Health Report"));
    assert!(text.contains("Health Score:"));
    assert!(text.contains("72"));
    assert!(text.contains("/ 100"));
    assert!(text.contains("Health Score Deduction Breakdown"));

    let json = reporter
        .render_json(
            score,
            score_grade(score),
            &root_causes,
            &diags,
            Some(&explanation),
        )
        .unwrap();
    assert!(json.contains("\"score_breakdown\""));
    assert!(json.contains("\"total_deductions\": 28"));

    let sarif = reporter.render_sarif(&diags).unwrap();
    assert!(sarif.contains("FAP-001"));
    assert!(sarif.contains("sarif-schema-2.1.0.json"));

    let html = reporter.render_html(
        score,
        score_grade(score),
        &root_causes,
        &diags,
        Some(&explanation),
    );
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

#[test]
fn test_doctor_score_explanation_and_deduplication() {
    let duplicate_diag = Diagnostic {
        rule_id: RuleId::new("FAP-001"),
        message: "Blocking call in async endpoint".to_string(),
        severity: Severity::Critical, // -20
        location: DiagnosticLocation::new("server/main.py"),
        explanation: "Blocking async loop".to_string(),
        suggested_remediation: "Use async".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };

    let warn_diag = Diagnostic {
        rule_id: RuleId::new("DEP-003"),
        message: "Unused dependency 'black'".to_string(),
        severity: Severity::Warning, // -2
        location: DiagnosticLocation::new("pyproject.toml"),
        explanation: "Unused package".to_string(),
        suggested_remediation: "Remove".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };

    // Include 4 duplicates of FAP-001 and 1 DEP-003
    let diags = vec![
        duplicate_diag.clone(),
        duplicate_diag.clone(),
        duplicate_diag.clone(),
        duplicate_diag.clone(),
        warn_diag.clone(),
    ];

    let explanation = stackvet_correlation::calculate_health_score_explained(&diags);
    // Deduplication should ensure FAP-001 is only penalized ONCE (-20), plus DEP-003 (-2) = 78
    assert_eq!(explanation.final_score, 78);
    assert_eq!(explanation.total_deductions, 22);
    assert_eq!(explanation.grade, "B (Fair)");
    assert_eq!(explanation.unique_findings_count, 2);
    assert_eq!(explanation.duplicate_findings_ignored, 3);

    let correlator = CorrelationEngine::new();
    let root_causes = correlator.correlate(diags.clone());

    let reporter = Reporter::new();
    let text = reporter.render_text(
        explanation.final_score,
        &explanation.grade,
        &root_causes,
        &diags,
        Some(&explanation),
    );

    assert!(text.contains("Health Score: 78 / 100"));
    assert!(text.contains("3 duplicates ignored"));
    assert!(text.contains("FAP-001"));
    assert!(text.contains("-20 pts"));
    assert!(text.contains("DEP-003"));
    assert!(text.contains("-2 pts"));
}

#[test]
fn test_repeated_scans_are_deterministic() {
    use stackvet_analyzers::run_all_analyzers;
    use stackvet_node::detect_node_project;
    use stackvet_python::detect_python_project;
    use stackvet_scanner::ProjectScanner;
    use std::path::PathBuf;

    let target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("regression")
        .join("positive")
        .join("fastapi_violations");

    assert!(target_dir.exists(), "Target directory must exist");

    let scanner = ProjectScanner::new(&target_dir);
    let mut ctx = scanner.build_context().expect("Scanner build context");
    ctx.python = detect_python_project(&target_dir);
    ctx.node = detect_node_project(&target_dir);

    // Run 10 consecutive diagnostic collections
    let mut initial_diags: Option<Vec<Diagnostic>> = None;
    let mut initial_score: Option<u32> = None;
    let mut initial_rc_ids: Option<Vec<String>> = None;

    for iteration in 0..10 {
        let raw_diags = run_all_analyzers(&ctx, true).expect("run analyzers");
        let (mut deduped, _) = stackvet_correlation::deduplicate_diagnostics(&raw_diags);
        deduped.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.rule_id.0.cmp(&b.rule_id.0))
                .then_with(|| a.location.file_path.cmp(&b.location.file_path))
                .then_with(|| {
                    let a_line = a.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);
                    let b_line = b.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);
                    a_line.cmp(&b_line)
                })
                .then_with(|| a.message.cmp(&b.message))
        });

        let explanation = stackvet_correlation::calculate_health_score_explained(&deduped);
        let score = explanation.final_score;

        let correlator = CorrelationEngine::new();
        let root_causes = correlator.correlate(deduped.clone());
        let rc_ids: Vec<String> = root_causes.into_iter().map(|rc| rc.id).collect();

        if let Some(expected_diags) = &initial_diags {
            assert_eq!(
                expected_diags.len(),
                deduped.len(),
                "Diagnostics length changed on iteration {}",
                iteration
            );
            for (idx, (d1, d2)) in expected_diags.iter().zip(deduped.iter()).enumerate() {
                assert_eq!(
                    d1.rule_id, d2.rule_id,
                    "RuleId mismatch at index {} on iteration {}",
                    idx, iteration
                );
                assert_eq!(
                    d1.message, d2.message,
                    "Message mismatch at index {} on iteration {}",
                    idx, iteration
                );
                assert_eq!(
                    d1.severity, d2.severity,
                    "Severity mismatch at index {} on iteration {}",
                    idx, iteration
                );
            }
        } else {
            initial_diags = Some(deduped.clone());
        }

        if let Some(expected_score) = initial_score {
            assert_eq!(
                expected_score, score,
                "Score mismatch on iteration {}",
                iteration
            );
        } else {
            initial_score = Some(score);
        }

        if let Some(expected_rc) = &initial_rc_ids {
            assert_eq!(
                expected_rc, &rc_ids,
                "Root cause IDs order changed on iteration {}",
                iteration
            );
        } else {
            initial_rc_ids = Some(rc_ids);
        }
    }
}
