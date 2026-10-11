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
