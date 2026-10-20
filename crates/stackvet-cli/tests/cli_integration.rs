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

    assert!(text.contains("Health Score:"));
    assert!(text.contains("78"));
    assert!(text.contains("/ 100"));
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

#[test]
fn test_exit_codes_mapping() {
    use stackvet_core::ExitCode;
    assert_eq!(ExitCode::Success.as_i32(), 0);
    assert_eq!(ExitCode::FindingsViolation.as_i32(), 1);
    assert_eq!(ExitCode::ConfigurationError.as_i32(), 2);
    assert_eq!(ExitCode::ExecutionError.as_i32(), 3);
}

#[test]
fn test_baseline_aware_ci_workflow() {
    use stackvet_core::{
        BaselineData, Diagnostic, DiagnosticLocation, LanguageTarget, RuleId, Severity, TextSpan,
    };

    let d_legacy1 = Diagnostic {
        rule_id: RuleId::new("DEP-001"),
        message: "Legacy missing dep".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new("backend/legacy.py")
            .with_span(TextSpan::new(10, 10, 1, 5)),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };
    let d_legacy2 = Diagnostic {
        rule_id: RuleId::new("FAP-001"),
        message: "Legacy blocking call".to_string(),
        severity: Severity::Critical,
        location: DiagnosticLocation::new("backend/routes.py")
            .with_span(TextSpan::new(20, 20, 1, 5)),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };

    // Step 1: Record baseline from initial diagnostics
    let initial_diags = vec![d_legacy1.clone(), d_legacy2.clone()];
    let mut baseline = BaselineData::from_diagnostics(&initial_diags);
    assert_eq!(baseline.issue_fingerprints.len(), 2);

    // Step 2: In CI with no changes, baseline diff has 0 new issues and 2 tolerated
    let diff_clean = baseline.diff_diagnostics(&initial_diags);
    assert!(diff_clean.new_issues.is_empty());
    assert_eq!(diff_clean.tolerated_issues.len(), 2);
    assert!(diff_clean.resolved_fingerprints.is_empty());
    assert!(diff_clean
        .new_issues_above_threshold(Severity::Error)
        .is_empty());

    // Step 3: Developer fixes d_legacy1 (resolved!) and adds non-violating warning (d_warn)
    let d_warn = Diagnostic {
        rule_id: RuleId::new("SQL-001"),
        message: "Unindexed query".to_string(),
        severity: Severity::Warning,
        location: DiagnosticLocation::new("backend/models.py")
            .with_span(TextSpan::new(30, 30, 1, 5)),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };
    let current_diags = vec![d_legacy2.clone(), d_warn.clone()];
    let diff_progress = baseline.diff_diagnostics(&current_diags);

    // d_legacy1 was fixed!
    assert_eq!(diff_progress.resolved_fingerprints.len(), 1);
    assert!(diff_progress.resolved_fingerprints[0].contains("DEP-001"));
    // d_legacy2 tolerated
    assert_eq!(diff_progress.tolerated_issues.len(), 1);
    // d_warn is new, but with fail_on = Error, it does NOT violate threshold!
    assert_eq!(diff_progress.new_issues.len(), 1);
    assert!(diff_progress
        .new_issues_above_threshold(Severity::Error)
        .is_empty());
    // With fail_on = Warning, it DOES violate threshold
    assert_eq!(
        diff_progress
            .new_issues_above_threshold(Severity::Warning)
            .len(),
        1
    );

    // Step 4: Update baseline
    baseline.update_from_diagnostics(&current_diags);
    assert_eq!(baseline.issue_fingerprints.len(), 2);
    let diff_after_update = baseline.diff_diagnostics(&current_diags);
    assert!(diff_after_update.new_issues.is_empty());
    assert_eq!(diff_after_update.tolerated_issues.len(), 2);
    assert!(diff_after_update.resolved_fingerprints.is_empty());
}

#[test]
fn test_config_rule_overrides_and_threshold_precedence() {
    use stackvet_core::{
        Diagnostic, DiagnosticLocation, LanguageTarget, RuleId, Severity, StackvetConfig,
    };

    let config_toml = r#"
[thresholds]
fail_on = "warning"
min_score = 85

[rules]
DEP-001 = "warning"
FAP-001 = "disabled"
SQL-001 = { enabled = true }
"#;
    let config = StackvetConfig::from_toml_str(config_toml).expect("parse toml");

    let diags = vec![
        Diagnostic {
            rule_id: RuleId::new("DEP-001"),
            message: "Missing dependency".to_string(),
            severity: Severity::Error,
            location: DiagnosticLocation::new("main.py"),
            explanation: "".to_string(),
            suggested_remediation: "".to_string(),
            fix: None,
            language: LanguageTarget::Python,
        },
        Diagnostic {
            rule_id: RuleId::new("FAP-001"),
            message: "Blocking sleep".to_string(),
            severity: Severity::Critical,
            location: DiagnosticLocation::new("main.py"),
            explanation: "".to_string(),
            suggested_remediation: "".to_string(),
            fix: None,
            language: LanguageTarget::Python,
        },
        Diagnostic {
            rule_id: RuleId::new("SQL-001"),
            message: "Unindexed query".to_string(),
            severity: Severity::Critical,
            location: DiagnosticLocation::new("main.py"),
            explanation: "".to_string(),
            suggested_remediation: "".to_string(),
            fix: None,
            language: LanguageTarget::Python,
        },
    ];

    let processed = config.apply_rule_overrides(diags).expect("apply overrides");
    // FAP-001 disabled -> dropped!
    assert_eq!(processed.len(), 2);
    // DEP-001 downgraded to Warning
    assert_eq!(processed[0].rule_id.as_str(), "DEP-001");
    assert_eq!(processed[0].severity, Severity::Warning);
    // SQL-001 enabled=true preserves native Critical severity
    assert_eq!(processed[1].rule_id.as_str(), "SQL-001");
    assert_eq!(processed[1].severity, Severity::Critical);

    // Verify precedence
    // CLI --fail-on critical overrides config fail_on = warning
    assert_eq!(
        config.resolve_fail_on(Some(Severity::Critical), false, Severity::Error),
        Severity::Critical
    );
    // Config fail_on = warning overrides default Error
    assert_eq!(
        config.resolve_fail_on(None, false, Severity::Error),
        Severity::Warning
    );
    // CLI --strict forces Warning
    assert_eq!(
        config.resolve_fail_on(None, true, Severity::Error),
        Severity::Warning
    );
    // Min score
    assert_eq!(config.resolve_min_score(Some(95)), Some(95));
    assert_eq!(config.resolve_min_score(None), Some(85));
}

#[test]
fn test_configurable_suppressions_and_expiration() {
    use stackvet_core::{
        apply_suppressions, Diagnostic, DiagnosticLocation, LanguageTarget, RuleId, Severity,
        StackvetConfig,
    };
    use stackvet_correlation::calculate_health_score_explained;

    let config_toml = r#"
[[suppressions]]
rule = "DEP-001"
reason = "Legacy package issue"

[[suppressions]]
rule = "NDEP-001"
path = "legacy/**/*.js"
reason = "Ignore in legacy directory"

[[suppressions]]
rule = "SQL-001"
until = "2020-01-01"
reason = "Expired temporary bypass"

[[suppressions]]
rule = "FAP-001"
until = "2099-12-31"
reason = "Valid temporary bypass"
"#;

    let config = StackvetConfig::from_toml_str(config_toml).expect("parse valid config");
    config.validate().expect("validation must pass");

    let d_dep = Diagnostic {
        rule_id: RuleId::new("DEP-001"),
        message: "Missing dependency".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new("src/main.py"),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };

    let d_ndep_match = Diagnostic {
        rule_id: RuleId::new("NDEP-001"),
        message: "Missing node dep".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new("legacy/sub/app.js"),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Node,
    };

    let d_ndep_no_match = Diagnostic {
        rule_id: RuleId::new("NDEP-001"),
        message: "Missing node dep".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new("src/index.js"),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Node,
    };

    let d_sql_expired = Diagnostic {
        rule_id: RuleId::new("SQL-001"),
        message: "Unindexed query".to_string(),
        severity: Severity::Critical,
        location: DiagnosticLocation::new("src/db.py"),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };

    let d_fap_unexpired = Diagnostic {
        rule_id: RuleId::new("FAP-001"),
        message: "Blocking sleep in async func".to_string(),
        severity: Severity::Critical,
        location: DiagnosticLocation::new("src/api.py"),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };

    let diagnostics = vec![
        d_dep.clone(),
        d_ndep_match.clone(),
        d_ndep_no_match.clone(),
        d_sql_expired.clone(),
        d_fap_unexpired.clone(),
    ];

    let outcome = apply_suppressions(
        std::path::Path::new(""),
        &config.suppressions,
        diagnostics,
        None,
    );

    // DEP-001 suppressed (rule-level)
    // NDEP-001 matching legacy/**/*.js suppressed
    // FAP-001 suppressed (unexpired)
    // Total suppressed: 3
    assert_eq!(outcome.suppressed.len(), 3);

    // Active findings: NDEP-001 in src/index.js (no path match), SQL-001 (expired)
    assert_eq!(outcome.active_diagnostics.len(), 2);
    let active_rules: Vec<&str> = outcome
        .active_diagnostics
        .iter()
        .map(|d| d.rule_id.as_str())
        .collect();
    assert!(active_rules.contains(&"NDEP-001"));
    assert!(active_rules.contains(&"SQL-001"));

    // Expired suppression was detected and reported
    assert_eq!(outcome.expired_configured.len(), 1);
    assert_eq!(
        outcome.expired_configured[0].rule.as_deref(),
        Some("SQL-001")
    );

    // Suppressed findings do not deduct from health score
    let mut explanation = calculate_health_score_explained(&outcome.active_diagnostics);
    explanation.suppressed_findings_count = outcome.suppressed.len();

    // Active findings: NDEP-001 (Error: -8) + SQL-001 (Critical: -20) = 28 deduction -> score 72
    assert_eq!(explanation.final_score, 72);
    assert_eq!(explanation.suppressed_findings_count, 3);
}

#[test]
fn test_inline_comment_suppressions_all_syntaxes_and_scopes() {
    use stackvet_core::{
        apply_suppressions, parse_inline_suppressions, Diagnostic, DiagnosticLocation,
        LanguageTarget, RuleId, Severity, TextSpan,
    };
    use std::fs;

    let test_dir = std::env::temp_dir().join(format!(
        "stackvet_inline_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir_all(&test_dir).unwrap();

    let python_code = r#"# File header
# stackvet:ignore-file[DJG-001]: Suppress Django warning across entire file

def bad_func():
    time.sleep(1)  # stackvet:ignore[FAP-001]: intentional delay
    print("line 6")

# stackvet:ignore-next-line[DEP-001]: Next line missing dep is expected
import missing_pkg

import another_missing_pkg
"#;
    let py_path = test_dir.join("app.py");
    fs::write(&py_path, python_code).unwrap();

    let inline_py = parse_inline_suppressions(python_code);
    assert_eq!(inline_py.len(), 3);

    let ts_code = r#"// stackvet:disable=NDEP-001: Intentional legacy import
import 'legacy-pkg';

/* stackvet:ignore[RCT-001]: Legacy hook usage */
useLegacyHook();

// stackvet:ignore-next-line[NXT-001]
export const dynamic = 'force-dynamic';
"#;
    let ts_path = test_dir.join("app.tsx");
    fs::write(&ts_path, ts_code).unwrap();

    let inline_ts = parse_inline_suppressions(ts_code);
    assert_eq!(inline_ts.len(), 3);

    let d1 = Diagnostic {
        rule_id: RuleId::new("FAP-001"),
        message: "Blocking sleep".to_string(),
        severity: Severity::Critical,
        location: DiagnosticLocation::new("app.py").with_span(TextSpan::new(5, 5, 5, 17)),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };

    let d2 = Diagnostic {
        rule_id: RuleId::new("DEP-001"),
        message: "Missing dep".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new("app.py").with_span(TextSpan::new(9, 9, 1, 19)),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };

    let d2_unsuppressed = Diagnostic {
        rule_id: RuleId::new("DEP-001"),
        message: "Another missing dep".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new("app.py").with_span(TextSpan::new(11, 11, 1, 27)),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };

    let d3_file = Diagnostic {
        rule_id: RuleId::new("DJG-001"),
        message: "Django issue".to_string(),
        severity: Severity::Warning,
        location: DiagnosticLocation::new("app.py").with_span(TextSpan::new(20, 20, 1, 10)),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Python,
    };

    let d_ts1 = Diagnostic {
        rule_id: RuleId::new("NDEP-001"),
        message: "Legacy import".to_string(),
        severity: Severity::Error,
        location: DiagnosticLocation::new("app.tsx").with_span(TextSpan::new(1, 1, 1, 20)),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Node,
    };

    let d_ts2 = Diagnostic {
        rule_id: RuleId::new("RCT-001"),
        message: "Legacy hook".to_string(),
        severity: Severity::Warning,
        location: DiagnosticLocation::new("app.tsx").with_span(TextSpan::new(4, 4, 1, 16)),
        explanation: "".to_string(),
        suggested_remediation: "".to_string(),
        fix: None,
        language: LanguageTarget::Node,
    };

    let diags = vec![d1, d2, d2_unsuppressed.clone(), d3_file, d_ts1, d_ts2];

    let outcome = apply_suppressions(&test_dir, &[], diags, None);

    // Only d2_unsuppressed should remain active!
    assert_eq!(outcome.active_diagnostics.len(), 1);
    assert_eq!(
        outcome.active_diagnostics[0]
            .location
            .span
            .as_ref()
            .unwrap()
            .start_line,
        11
    );

    // 5 diagnostics suppressed
    assert_eq!(outcome.suppressed.len(), 5);

    let _ = fs::remove_dir_all(&test_dir);
}

#[test]
fn test_suppression_validation_unknown_rules_and_malformed_dates() {
    use stackvet_core::StackvetConfig;

    // Unknown rule ID must fail validation
    let bad_rule_toml = r#"
[[suppressions]]
rule = "UNKNOWN-999"
reason = "Invalid rule"
"#;
    let err = StackvetConfig::from_toml_str(bad_rule_toml).unwrap_err();
    assert!(err.to_string().contains("UNKNOWN-999"));

    // Malformed date must fail validation
    let bad_date_toml = r#"
[[suppressions]]
rule = "DEP-001"
until = "invalid-date"
reason = "Malformed date"
"#;
    let err = StackvetConfig::from_toml_str(bad_date_toml).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("invalid date"));

    // Valid rule and date must pass validation
    let good_toml = r#"
[[suppressions]]
rule = "DEP-001"
until = "2026-12-31"
reason = "Valid rule and date"
"#;
    let config = StackvetConfig::from_toml_str(good_toml).unwrap();
    assert!(config.validate().is_ok());
}

#[test]
fn test_ignore_exclusions_precedence_and_stackvetignore() {
    use stackvet_core::StackvetConfig;
    use stackvet_scanner::ProjectScanner;
    use std::fs;

    let test_dir = std::env::temp_dir().join(format!(
        "stackvet_ignore_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir_all(&test_dir).unwrap();

    // 1. Root manifests (should be preserved!)
    fs::write(test_dir.join("package.json"), "{}").unwrap();
    fs::write(test_dir.join("pyproject.toml"), "").unwrap();

    // 2. Default exclusion dirs
    let node_modules = test_dir.join("node_modules").join("some-pkg");
    fs::create_dir_all(&node_modules).unwrap();
    fs::write(node_modules.join("index.js"), "module.exports = {}").unwrap();
    fs::write(node_modules.join("package.json"), "{}").unwrap();

    let target_dir = test_dir.join("target").join("debug");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(target_dir.join("app.js"), "").unwrap();

    let venv_dir = test_dir.join(".venv").join("lib");
    fs::create_dir_all(&venv_dir).unwrap();
    fs::write(venv_dir.join("site.py"), "").unwrap();

    // 3. Regular active files
    fs::write(test_dir.join("main.py"), "import os").unwrap();
    fs::write(test_dir.join("index.ts"), "console.log('hi');").unwrap();

    // 4. .stackvetignore patterns
    let legacy_dir = test_dir.join("legacy");
    fs::create_dir_all(&legacy_dir).unwrap();
    fs::write(legacy_dir.join("old.py"), "pass").unwrap();
    fs::write(test_dir.join(".stackvetignore"), "legacy/**\n").unwrap();

    // 5. Config ignore patterns
    let generated_dir = test_dir.join("generated");
    fs::create_dir_all(&generated_dir).unwrap();
    fs::write(generated_dir.join("schema.ts"), "export type A = string;").unwrap();

    let config_toml = r#"
[ignore]
patterns = ["generated/**"]
"#;
    let config = StackvetConfig::from_toml_str(config_toml).unwrap();

    // 6. CLI --ignore pattern
    let cli_ignored_file = test_dir.join("temp_debug.py");
    fs::write(&cli_ignored_file, "import sys").unwrap();
    let cli_ignores = vec!["temp_debug.py".to_string()];

    // Merge config + cli ignores
    let mut all_ignores = config.ignore.patterns().to_vec();
    all_ignores.extend(cli_ignores);

    let scanner = ProjectScanner::new(&test_dir).with_ignores(all_ignores);
    let res = scanner.scan();

    // Root manifests preserved
    assert!(res.manifest_files.contains(&test_dir.join("package.json")));
    assert!(res
        .manifest_files
        .contains(&test_dir.join("pyproject.toml")));

    // node_modules package.json excluded
    assert!(!res
        .manifest_files
        .contains(&node_modules.join("package.json")));

    // Active source files discovered
    assert!(res.source_files.contains(&test_dir.join("main.py")));
    assert!(res.source_files.contains(&test_dir.join("index.ts")));

    // Excluded by default exclusions
    assert!(!res.source_files.contains(&node_modules.join("index.js")));
    assert!(!res.source_files.contains(&target_dir.join("app.js")));
    assert!(!res.source_files.contains(&venv_dir.join("site.py")));

    // Excluded by .stackvetignore
    assert!(!res.source_files.contains(&legacy_dir.join("old.py")));

    // Excluded by config [ignore]
    assert!(!res.source_files.contains(&generated_dir.join("schema.ts")));

    // Excluded by CLI --ignore
    assert!(!res.source_files.contains(&cli_ignored_file));

    // Cleanup
    let _ = fs::remove_dir_all(&test_dir);
}
