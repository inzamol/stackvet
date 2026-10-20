use stackvet_core::{
    Confidence, Diagnostic, DiagnosticLocation, FixAction, LanguageTarget, Severity, TextSpan,
};
use stackvet_reporter::{FullReport, ReportSummary, Reporter, ToolInfo, SCHEMA_VERSION};

#[test]
fn test_schema_version_and_metadata() {
    assert_eq!(SCHEMA_VERSION, "1.0.0");

    let tool = ToolInfo::default();
    assert_eq!(tool.name, "stackvet");
    assert!(!tool.version.is_empty());
    assert_eq!(
        tool.repository.as_deref(),
        Some("https://github.com/stackvet/stackvet")
    );
}

#[test]
fn test_report_summary_from_diagnostics() {
    let diags = vec![
        Diagnostic::builder("DEP-001", Severity::Critical, LanguageTarget::General)
            .message("Critical error")
            .build(),
        Diagnostic::builder("DEP-002", Severity::Error, LanguageTarget::General)
            .message("Error with fix")
            .fix(FixAction::new("fix desc", "file.py", "replacement"))
            .build(),
        Diagnostic::builder("DEP-003", Severity::Warning, LanguageTarget::General)
            .message("Warning issue")
            .build(),
        Diagnostic::builder("DEP-004", Severity::Info, LanguageTarget::General)
            .message("Info issue")
            .build(),
        Diagnostic::builder("DEP-005", Severity::Hint, LanguageTarget::General)
            .message("Hint issue")
            .build(),
    ];

    let summary = ReportSummary::from_diagnostics(&diags);
    assert_eq!(summary.total_issues, 5);
    assert_eq!(summary.critical_count, 1);
    assert_eq!(summary.error_count, 1);
    assert_eq!(summary.warning_count, 1);
    assert_eq!(summary.info_count, 1);
    assert_eq!(summary.hint_count, 1);
    assert_eq!(summary.fixable_count, 1);
}

#[test]
fn test_json_report_serialization_and_deserialization() {
    let reporter = Reporter::new();

    let diag = Diagnostic::builder("IMP-001", Severity::Error, LanguageTarget::Python)
        .message("Circular import cycle: a.py -> b.py -> a.py")
        .location(
            DiagnosticLocation::new("src/a.py")
                .with_span(TextSpan::new(5, 7, 1, 20))
                .with_snippet("import b"),
        )
        .explanation("Circular dependency causes runtime import failure.")
        .remediation("Extract shared dependencies.")
        .fix(FixAction::new(
            "Extract shared code",
            "src/a.py",
            "# refactored",
        ))
        .related_location(
            DiagnosticLocation::new("src/b.py")
                .with_span(TextSpan::single_line(12, 1, 10))
                .with_message("Part of cycle"),
        )
        .confidence(Confidence::High)
        .build();

    let json_str = reporter
        .render_json(85, "B", &[], std::slice::from_ref(&diag), None)
        .expect("render json");

    // Verify root schema fields
    let v: serde_json::Value = serde_json::from_str(&json_str).expect("parse json");
    assert_eq!(v["schema_version"], "1.0.0");
    assert_eq!(v["tool"]["name"], "stackvet");
    assert_eq!(v["summary"]["total_issues"], 1);
    assert_eq!(v["summary"]["error_count"], 1);
    assert_eq!(v["health_score"], 85);
    assert_eq!(v["grade"], "B");

    // Deserialize into typed FullReport
    let report: FullReport = serde_json::from_str(&json_str).expect("deserialize FullReport");
    assert_eq!(report.schema_version, "1.0.0");
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].rule_id.as_str(), "IMP-001");
    assert_eq!(report.diagnostics[0].related_locations.len(), 1);
    assert_eq!(
        report.diagnostics[0].related_locations[0].display_location(),
        "src/b.py:12"
    );
}

#[test]
fn test_json_schema_backward_compatibility() {
    // Legacy JSON report without schema_version, tool, or summary
    let legacy_json = r#"{
        "health_score": 92,
        "grade": "A",
        "root_causes": [],
        "diagnostics": [
            {
                "rule_id": "DEP-002",
                "message": "Missing dependency",
                "severity": "ERROR",
                "location": {
                    "file_path": "main.py"
                },
                "explanation": "Runtime fail",
                "suggested_remediation": "Add dependency",
                "language": "Python"
            }
        ]
    }"#;

    let report: FullReport = serde_json::from_str(legacy_json).expect("deserialize legacy report");
    // Default schema_version 1.0.0 is applied
    assert_eq!(report.schema_version, "1.0.0");
    assert_eq!(report.tool.name, "stackvet");
    assert_eq!(report.health_score, 92);
    assert_eq!(report.diagnostics.len(), 1);
    assert!(report.diagnostics[0].related_locations.is_empty());
}

#[test]
fn test_empty_results_and_optional_fields() {
    let reporter = Reporter::new();
    let json_str = reporter
        .render_json(100, "A", &[], &[], None)
        .expect("render json empty");

    let report: FullReport = serde_json::from_str(&json_str).expect("deserialize empty");
    assert_eq!(report.health_score, 100);
    assert_eq!(report.summary.total_issues, 0);
    assert!(report.diagnostics.is_empty());
    assert!(report.root_causes.is_empty());

    // SARIF with empty diagnostics
    let sarif_str = reporter.render_sarif(&[]).expect("render sarif empty");
    let sarif_val: serde_json::Value = serde_json::from_str(&sarif_str).expect("parse sarif empty");
    assert_eq!(sarif_val["version"], "2.1.0");
    assert_eq!(sarif_val["runs"][0]["results"].as_array().unwrap().len(), 0);
}

#[test]
fn test_sarif_v210_spec_compliance() {
    let reporter = Reporter::new();

    let fix = FixAction::new("Declare requests", "pyproject.toml", "requests = '^2.31.0'")
        .with_span(TextSpan::single_line(15, 1, 20));

    let related = DiagnosticLocation::new(r"tests\test_app.py")
        .with_span(TextSpan::new(3, 4, 1, 15))
        .with_message("Also imported in test");

    let diag = Diagnostic::builder("DEP-002", Severity::Error, LanguageTarget::Python)
        .message("Package 'requests' is imported but not declared")
        .location(
            DiagnosticLocation::new(r"src\api\client.py")
                .with_span(TextSpan::new(10, 12, 5, 25))
                .with_snippet("import requests"),
        )
        .fix(fix)
        .related_location(related)
        .build();

    let sarif_str = reporter.render_sarif(&[diag]).expect("render sarif");
    let sarif: serde_json::Value = serde_json::from_str(&sarif_str).expect("parse sarif");

    assert_eq!(sarif["version"], "2.1.0");
    assert_eq!(
        sarif["$schema"],
        "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json"
    );

    let driver = &sarif["runs"][0]["tool"]["driver"];
    assert_eq!(driver["name"], "stackvet");

    // Catalog rules descriptors populated
    let rules = driver["rules"].as_array().expect("rules array");
    let rule_dep002 = rules
        .iter()
        .find(|r| r["id"] == "DEP-002")
        .expect("DEP-002 rule descriptor");
    assert_eq!(rule_dep002["defaultConfiguration"]["level"], "error");
    assert_eq!(rule_dep002["properties"]["category"], "dependency");

    // Results array
    let results = sarif["runs"][0]["results"]
        .as_array()
        .expect("results array");
    assert_eq!(results.len(), 1);
    let r0 = &results[0];
    assert_eq!(r0["ruleId"], "DEP-002");
    assert_eq!(r0["level"], "error");

    // Location region with startLine, startColumn, endLine, endColumn and snippet
    let loc = &r0["locations"][0]["physicalLocation"];
    assert_eq!(loc["artifactLocation"]["uri"], "src/api/client.py");
    assert_eq!(loc["region"]["startLine"], 10);
    assert_eq!(loc["region"]["startColumn"], 5);
    assert_eq!(loc["region"]["endLine"], 12);
    assert_eq!(loc["region"]["endColumn"], 25);
    assert_eq!(loc["region"]["snippet"]["text"], "import requests");

    // Related locations
    let rel = &r0["relatedLocations"][0];
    assert_eq!(rel["id"], 1);
    assert_eq!(
        rel["physicalLocation"]["artifactLocation"]["uri"],
        "tests/test_app.py"
    );
    assert_eq!(rel["message"]["text"], "Also imported in test");

    // Automated fix in SARIF format
    let fixes = r0["fixes"].as_array().expect("fixes array");
    assert_eq!(fixes.len(), 1);
    assert_eq!(fixes[0]["description"]["text"], "Declare requests");
    let file_changes = &fixes[0]["fileChanges"][0];
    assert_eq!(file_changes["artifactLocation"]["uri"], "pyproject.toml");
    let rep = &file_changes["replacements"][0];
    assert_eq!(rep["insertedContent"]["text"], "requests = '^2.31.0'");
    assert_eq!(rep["deletedRegion"]["startLine"], 15);
}

#[test]
fn test_missing_location_explicitly_represented() {
    let reporter = Reporter::new();

    // Diagnostic with missing / unknown location (project-level check)
    let diag = Diagnostic::builder("DOCKER-001", Severity::Warning, LanguageTarget::Docker)
        .message("No Dockerfile found in repository root")
        .build();

    // Check SARIF: locations should be explicitly empty, NOT fabricated as uri: "<unknown>", startLine: 1
    let sarif_str = reporter
        .render_sarif(std::slice::from_ref(&diag))
        .expect("render sarif");
    let sarif: serde_json::Value = serde_json::from_str(&sarif_str).expect("parse sarif");
    let r0 = &sarif["runs"][0]["results"][0];
    assert_eq!(r0["locations"].as_array().unwrap().len(), 0);

    // Check Text: displays explicit "-"
    assert_eq!(diag.location.display_location(), "-");
    let text = reporter.render_text(100, "A", &[], &[diag], None);
    assert!(!text.contains("<unknown>"));
    assert!(text.contains("DOCKER-001") && text.contains(" - "));
}

#[test]
fn test_sarif_omits_fixes_when_fix_has_no_span() {
    let reporter = Reporter::new();

    // Fix without a code span (e.g. general instruction / external action)
    let diag = Diagnostic::builder("DEP-002", Severity::Error, LanguageTarget::Python)
        .message("Undeclared package")
        .location(DiagnosticLocation::new("src/main.py").with_span(TextSpan::single_line(1, 1, 15)))
        .fix(stackvet_core::FixAction::new(
            "Add package to pyproject.toml",
            "pyproject.toml",
            "requests = \"^2.31.0\"",
        ))
        .build();

    let sarif_str = reporter.render_sarif(&[diag]).expect("render sarif");
    let sarif: serde_json::Value = serde_json::from_str(&sarif_str).expect("parse sarif");
    let r0 = &sarif["runs"][0]["results"][0];
    assert!(r0.get("fixes").is_none());
}

#[test]
fn test_formats_location_agreement() {
    let reporter = Reporter::new();

    let diag = Diagnostic::builder("FAP-001", Severity::Critical, LanguageTarget::Python)
        .message("Blocking call inside async handler")
        .location(
            DiagnosticLocation::new(r"app\routers\users.py")
                .with_span(TextSpan::new(42, 42, 5, 20)),
        )
        .build();

    let expected_display = "app/routers/users.py:42:5";
    assert_eq!(diag.location.display_location(), expected_display);

    // Text format contains exact location
    let text_out = reporter.render_text(80, "B", &[], std::slice::from_ref(&diag), None);
    assert!(text_out.contains(expected_display));

    // HTML format contains exact location
    let html_out = reporter.render_html(80, "B", &[], std::slice::from_ref(&diag), None);
    assert!(html_out.contains(expected_display));

    // SARIF format normalized path and matching startLine / startColumn
    let sarif_str = reporter.render_sarif(&[diag]).expect("render sarif");
    let sarif: serde_json::Value = serde_json::from_str(&sarif_str).expect("parse sarif");
    let loc = &sarif["runs"][0]["results"][0]["locations"][0]["physicalLocation"];
    assert_eq!(loc["artifactLocation"]["uri"], "app/routers/users.py");
    assert_eq!(loc["region"]["startLine"], 42);
    assert_eq!(loc["region"]["startColumn"], 5);
}

#[test]
fn test_sarif_relativizes_to_scan_root_and_omits_uribaseid_for_external_path() {
    use std::path::Path;

    let reporter = Reporter::new();
    let root = Path::new(if cfg!(windows) {
        r"C:\project\root"
    } else {
        "/project/root"
    });

    // Diagnostic 1: inside scan root
    let inside_path = if cfg!(windows) {
        Path::new(r"C:\project\root\src\lib.rs")
    } else {
        Path::new("/project/root/src/lib.rs")
    };
    let diag_inside = Diagnostic::builder("DEP-001", Severity::Warning, LanguageTarget::Python)
        .message("Inside file")
        .location(DiagnosticLocation::new(inside_path))
        .build();

    // Diagnostic 2: external path outside scan root
    let external_path = if cfg!(windows) {
        Path::new(r"D:\other\system.py")
    } else {
        Path::new("/var/other/system.py")
    };
    let diag_external = Diagnostic::builder("DEP-002", Severity::Error, LanguageTarget::Python)
        .message("External file")
        .location(DiagnosticLocation::new(external_path))
        .build();

    let sarif_str = reporter
        .render_sarif_with_root(&[diag_inside, diag_external], Some(root))
        .expect("render sarif");

    let sarif: serde_json::Value = serde_json::from_str(&sarif_str).expect("parse sarif");
    let results = &sarif["runs"][0]["results"];

    // First result: relative to root, uriBaseId = %SRCROOT%
    let loc0 = &results[0]["locations"][0]["physicalLocation"]["artifactLocation"];
    assert_eq!(loc0["uri"], "src/lib.rs");
    assert_eq!(loc0["uriBaseId"], "%SRCROOT%");

    // Second result: external, uri starts with file:// and uriBaseId MUST BE OMITTED per SARIF spec
    let loc1 = &results[1]["locations"][0]["physicalLocation"]["artifactLocation"];
    assert!(loc1["uri"].as_str().unwrap().starts_with("file://"));
    assert!(
        loc1.get("uriBaseId").is_none(),
        "uriBaseId must NOT be present when artifact location is an absolute file:// URI"
    );
}
