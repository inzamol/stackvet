use stackvet_core::{
    Confidence, Diagnostic, DiagnosticLocation, FixAction, LanguageTarget, Severity, TextSpan,
};
use stackvet_reporter::{
    FullReport, ReportSummary, Reporter, ToolInfo, VulnerabilityCheckStatus,
    VulnerabilityCheckSummary, SCHEMA_VERSION,
};

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
    let reporter = Reporter::new().with_vulnerability_check(VulnerabilityCheckSummary {
        status: VulnerabilityCheckStatus::Completed,
        message: "Vulnerability check completed: 0 known vulnerabilities detected (OSV.dev)"
            .to_string(),
        advisory_database: Some("OSV.dev".to_string()),
        cache_status: Some("fresh".to_string()),
        vulnerabilities_found: Some(0),
    });

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
    let check: &VulnerabilityCheckSummary = report.summary.vulnerability_check.as_ref().unwrap();
    assert_eq!(check.status, VulnerabilityCheckStatus::Completed);
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

    assert_eq!(sarif["runs"][0]["columnKind"], "unicodeCodePoints");

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
    assert_eq!(loc["artifactLocation"]["uriBaseId"], "%SRCROOT%");
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

#[test]
fn test_sarif_uri_handling_with_root_and_external_paths() {
    let scan_root = std::path::PathBuf::from("/project/root");
    let reporter = Reporter::with_root(&scan_root);

    // 1. Inside root: should be relativized and retain uriBaseId = %SRCROOT%
    let inside_diag = Diagnostic::builder("DEP-001", Severity::Error, LanguageTarget::Python)
        .message("Inside root issue")
        .location(DiagnosticLocation::new(
            "/project/root/src/my module/foo#1.py",
        ))
        .build();

    // 2. Outside root: should be preserved as file:/// URI and omit uriBaseId
    let outside_diag = Diagnostic::builder("DEP-002", Severity::Warning, LanguageTarget::Python)
        .message("Outside root issue")
        .location(DiagnosticLocation::new("/opt/external/lib.py"))
        .build();

    // 3. Sibling prefix /project/root2: boundary check must NOT match /project/root
    let sibling_diag = Diagnostic::builder("DEP-003", Severity::Warning, LanguageTarget::Python)
        .message("Sibling root issue")
        .location(DiagnosticLocation::new("/project/root2/other.py"))
        .build();

    // 4. Diagnostic with snippet but NO span: must NOT emit a snippet-only region
    let snippet_only_diag = Diagnostic::builder("DEP-004", Severity::Info, LanguageTarget::Python)
        .message("Snippet without span")
        .location(
            DiagnosticLocation::new("/project/root/src/main.py").with_snippet("some code snippet"),
        )
        .build();

    let sarif_str = reporter
        .render_sarif(&[inside_diag, outside_diag, sibling_diag, snippet_only_diag])
        .expect("render sarif");
    let sarif: serde_json::Value = serde_json::from_str(&sarif_str).expect("parse sarif");

    // originalUriBaseIds definition
    let base_ids = &sarif["runs"][0]["originalUriBaseIds"];
    assert_eq!(base_ids["%SRCROOT%"]["uri"], "file:///project/root/");

    let results = &sarif["runs"][0]["results"];

    // 1. Inside root with URI encoding
    let loc_inside = &results[0]["locations"][0]["physicalLocation"]["artifactLocation"];
    assert_eq!(loc_inside["uri"], "src/my%20module/foo%231.py");
    assert_eq!(loc_inside["uriBaseId"], "%SRCROOT%");

    // 2. Outside root
    let loc_outside = &results[1]["locations"][0]["physicalLocation"]["artifactLocation"];
    assert_eq!(loc_outside["uri"], "file:///opt/external/lib.py");
    assert!(loc_outside.get("uriBaseId").is_none());

    // 3. Sibling root: outside root
    let loc_sibling = &results[2]["locations"][0]["physicalLocation"]["artifactLocation"];
    assert_eq!(loc_sibling["uri"], "file:///project/root2/other.py");
    assert!(loc_sibling.get("uriBaseId").is_none());

    // 4. Snippet without span: physicalLocation has no region
    let phys_snippet = &results[3]["locations"][0]["physicalLocation"];
    assert!(phys_snippet.get("region").is_none());
}

#[test]
fn test_sarif_edge_cases_unicode_and_special_paths() {
    let scan_root = std::path::PathBuf::from("/workspace/test");
    let reporter = Reporter::with_root(&scan_root);

    // Unicode path and message with symbols and emojis
    let unicode_diag = Diagnostic::builder("DEP-001", Severity::Error, LanguageTarget::Python)
        .message("Found syntax conflict in 🚀 module: 'utf8_üñîçødé' & \"symbols\"")
        .location(
            DiagnosticLocation::new("/workspace/test/src/üñîçødé_тест/ファイル.py")
                .with_span(TextSpan::new(10, 25, 4, 18))
                .with_snippet("import 測試"),
        )
        .related_location(
            DiagnosticLocation::new("/workspace/test/docs/notes (draft)/#section 1.md")
                .with_span(TextSpan::single_line(42, 1, 30))
                .with_message("Referenced here 💡"),
        )
        .fix(
            FixAction::new(
                "Update import line",
                "/workspace/test/src/üñîçødé_тест/ファイル.py",
                "import standard_lib",
            )
            .with_span(TextSpan::new(10, 10, 4, 15)),
        )
        .build();

    let sarif_str = reporter
        .render_sarif(&[unicode_diag])
        .expect("render sarif unicode");

    let sarif: serde_json::Value = serde_json::from_str(&sarif_str).expect("parse sarif unicode");

    assert_eq!(sarif["version"], "2.1.0");
    let result = &sarif["runs"][0]["results"][0];

    // Message preserved with Unicode & emojis
    assert!(result["message"]["text"]
        .as_str()
        .unwrap()
        .contains("Found syntax conflict in 🚀 module"));

    // Physical location URI encoding
    let loc = &result["locations"][0]["physicalLocation"];
    let loc_uri = loc["artifactLocation"]["uri"].as_str().unwrap();
    assert!(loc_uri.starts_with("src/"));
    assert_eq!(loc["region"]["startLine"], 10);
    assert_eq!(loc["region"]["endLine"], 25);
    assert_eq!(loc["region"]["startColumn"], 4);
    assert_eq!(loc["region"]["endColumn"], 18);
    assert_eq!(loc["region"]["snippet"]["text"], "import 測試");

    // Related location path with spaces and hash encoded
    let rel_loc = &result["relatedLocations"][0]["physicalLocation"];
    let rel_uri = rel_loc["artifactLocation"]["uri"].as_str().unwrap();
    assert_eq!(rel_uri, "docs/notes%20(draft)/%23section%201.md");
    assert_eq!(
        result["relatedLocations"][0]["message"]["text"],
        "Referenced here 💡"
    );

    // Fix replacement deletedRegion
    let fix = &result["fixes"][0];
    assert_eq!(fix["description"]["text"], "Update import line");
    let rep = &fix["fileChanges"][0]["replacements"][0];
    assert_eq!(rep["deletedRegion"]["startLine"], 10);
    assert_eq!(rep["deletedRegion"]["endLine"], 10);
    assert_eq!(rep["insertedContent"]["text"], "import standard_lib");
}

#[test]
fn test_sarif_multiline_spans_and_severities() {
    let reporter = Reporter::new();

    let diags = vec![
        Diagnostic::builder("SEC-001", Severity::Critical, LanguageTarget::General)
            .message("Critical vulnerability")
            .location(
                DiagnosticLocation::new("security.json").with_span(TextSpan::new(1, 50, 1, 80)),
            )
            .build(),
        Diagnostic::builder("ENV-001", Severity::Warning, LanguageTarget::General)
            .message("Warning drift")
            .location(DiagnosticLocation::new("env.txt").with_span(TextSpan::single_line(5, 2, 10)))
            .build(),
        Diagnostic::builder("DOC-001", Severity::Info, LanguageTarget::General)
            .message("Info documentation")
            .location(
                DiagnosticLocation::new("README.md").with_span(TextSpan::single_line(1, 1, 1)),
            )
            .build(),
        Diagnostic::builder("HINT-001", Severity::Hint, LanguageTarget::General)
            .message("Hint optimization")
            .location(DiagnosticLocation::new("opt.py").with_span(TextSpan::single_line(12, 4, 8)))
            .build(),
    ];

    let sarif_str = reporter.render_sarif(&diags).expect("render sarif levels");
    let sarif: serde_json::Value = serde_json::from_str(&sarif_str).expect("parse sarif levels");

    let results = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 4);

    // Critical maps to error
    assert_eq!(results[0]["level"], "error");
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["region"]["startLine"],
        1
    );
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["region"]["endLine"],
        50
    );

    // Warning maps to warning
    assert_eq!(results[1]["level"], "warning");

    // Info maps to note
    assert_eq!(results[2]["level"], "note");

    // Hint maps to note
    assert_eq!(results[3]["level"], "note");
}

#[test]
fn test_vulnerability_check_status_distinguishes_clean_from_skipped_and_failed() {
    // 1. Clean scan (empty diagnostics do not infer a completed check)
    let clean_diags: Vec<Diagnostic> = vec![];
    let clean_summary = ReportSummary::from_diagnostics(&clean_diags);
    assert!(clean_summary.is_complete);
    assert_eq!(clean_summary.vulnerability_check, None);

    let explicit_clean =
        clean_summary
            .clone()
            .with_vulnerability_check(VulnerabilityCheckSummary {
                status: VulnerabilityCheckStatus::Completed,
                message:
                    "Vulnerability check completed: 0 known vulnerabilities detected (OSV.dev)"
                        .to_string(),
                advisory_database: Some("OSV.dev".to_string()),
                cache_status: Some("fresh".to_string()),
                vulnerabilities_found: Some(0),
            });
    let v_clean = explicit_clean
        .vulnerability_check
        .expect("vulnerability_check present");
    assert_eq!(v_clean.status, VulnerabilityCheckStatus::Completed);
    assert_eq!(v_clean.cache_status.as_deref(), Some("fresh"));
    assert_eq!(v_clean.advisory_database.as_deref(), Some("OSV.dev"));
    assert_eq!(v_clean.vulnerabilities_found, Some(0));

    // 2. Offline skipped check
    let skipped_diag = Diagnostic::builder("SEC-001", Severity::Warning, LanguageTarget::General)
        .message("Vulnerability checks skipped: offline mode is active and no local vulnerability cache was found")
        .build();
    let skipped_summary = ReportSummary::from_diagnostics(&[skipped_diag]);
    assert!(!skipped_summary.is_complete);
    assert!(skipped_summary
        .incomplete_reasons
        .iter()
        .any(|r| r.contains("SEC-001")));
    let v_skipped = skipped_summary
        .vulnerability_check
        .expect("vulnerability_check present");
    assert_eq!(v_skipped.status, VulnerabilityCheckStatus::Skipped);
    assert_eq!(v_skipped.cache_status.as_deref(), Some("missing"));
    assert_eq!(v_skipped.vulnerabilities_found, None);

    // 3. Failed check due to network failure
    let failed_diag = Diagnostic::builder("SEC-001", Severity::Warning, LanguageTarget::General)
        .message("The vulnerability advisory database (OSV.dev) is unavailable and no local cache was found: Network error connecting to OSV.dev advisory database: connection timed out")
        .build();
    let failed_summary = ReportSummary::from_diagnostics(&[failed_diag]);
    assert!(!failed_summary.is_complete);
    let v_failed = failed_summary
        .vulnerability_check
        .expect("vulnerability_check present");
    assert_eq!(v_failed.status, VulnerabilityCheckStatus::Failed);
    assert_eq!(v_failed.cache_status.as_deref(), Some("missing"));
    assert_eq!(v_failed.vulnerabilities_found, None);
    assert!(v_failed.message.contains("connection timed out"));

    // 4. Completed with warnings due to refresh failure fallback (SEC-003)
    let sec003_diag = Diagnostic::builder("SEC-003", Severity::Warning, LanguageTarget::General)
        .message("Vulnerability advisory database refresh failed: network error. Falling back to existing local cache.")
        .build();
    let warn_summary = ReportSummary::from_diagnostics(&[sec003_diag]);
    assert!(warn_summary.is_complete);
    let v_warn = warn_summary
        .vulnerability_check
        .expect("vulnerability_check present");
    assert_eq!(
        v_warn.status,
        VulnerabilityCheckStatus::CompletedWithWarnings
    );
    assert_eq!(v_warn.cache_status.as_deref(), Some("cached"));
    assert_eq!(v_warn.vulnerabilities_found, Some(0));

    // 5. Completed with findings (DEP-006)
    let dep006_diag = Diagnostic::builder("DEP-006", Severity::Error, LanguageTarget::General)
        .message("Vulnerable dependency 'requests==2.25.0' (GHSA-j8r2-6x86-q33q)")
        .build();
    let finding_summary = ReportSummary::from_diagnostics(&[dep006_diag]);
    assert!(finding_summary.is_complete);
    let v_finding = finding_summary
        .vulnerability_check
        .expect("vulnerability_check present");
    assert_eq!(v_finding.status, VulnerabilityCheckStatus::Completed);
    assert_eq!(v_finding.vulnerabilities_found, Some(1));

    // 6. Explicit Failed/Skipped via with_vulnerability_check marks is_complete = false even when SEC-001 absent
    let failed_override_summary =
        clean_summary
            .clone()
            .with_vulnerability_check(VulnerabilityCheckSummary {
                status: VulnerabilityCheckStatus::Failed,
                message: "Network failure querying OSV.dev".to_string(),
                advisory_database: Some("OSV.dev".to_string()),
                cache_status: Some("missing".to_string()),
                vulnerabilities_found: None,
            });
    assert!(!failed_override_summary.is_complete);
    assert!(failed_override_summary
        .incomplete_reasons
        .iter()
        .any(|r| r.contains("Network failure")));

    let reporter = Reporter::new().with_vulnerability_check(VulnerabilityCheckSummary {
        status: VulnerabilityCheckStatus::Skipped,
        message: "Offline mode active".to_string(),
        advisory_database: Some("OSV.dev".to_string()),
        cache_status: Some("missing".to_string()),
        vulnerabilities_found: None,
    });
    let json_out = reporter
        .render_json(100, "A+", &[], &[], None)
        .expect("render json");
    let json_val: serde_json::Value = serde_json::from_str(&json_out).expect("parse json");
    assert_eq!(json_val["summary"]["is_complete"], false);
    assert!(json_val["summary"]["incomplete_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r.as_str().unwrap().contains("Offline mode active")));
}

#[test]
fn test_text_and_sarif_vulnerability_status_consistency() {
    colored::control::set_override(false);
    let reporter = Reporter::new();

    // Case A: Unavailable security checks
    let unavailable_diag = Diagnostic::builder("SEC-001", Severity::Warning, LanguageTarget::General)
        .message("The vulnerability advisory database (OSV.dev) is unavailable and no local cache was found: Network error")
        .build();

    let text_output =
        reporter.render_text(80, "B", &[], std::slice::from_ref(&unavailable_diag), None);
    assert!(text_output.contains("Security:      ✗ Unavailable"));
    assert!(text_output.contains("Incomplete Analysis / Scanner Hardening Notices"));
    assert!(text_output.contains("SEC-001"));

    let sarif_output = reporter
        .render_sarif(&[unavailable_diag])
        .expect("render sarif");
    let sarif_val: serde_json::Value = serde_json::from_str(&sarif_output).expect("parse sarif");
    let invocations = sarif_val["runs"][0]["invocations"]
        .as_array()
        .expect("invocations array");
    assert_eq!(invocations[0]["executionSuccessful"], false);
    let notifications = invocations[0]["toolExecutionNotifications"]
        .as_array()
        .expect("notifications");
    assert_eq!(notifications[0]["descriptor"]["id"], "SEC-001");
    let run_props = &sarif_val["runs"][0]["properties"];
    assert_eq!(run_props["vulnerability_check"]["status"], "failed");
    assert_eq!(run_props["vulnerabilityCheck"]["status"], "failed");

    // Case B: Clean scan
    let clean_reporter = Reporter::new().with_vulnerability_check(VulnerabilityCheckSummary {
        status: VulnerabilityCheckStatus::Completed,
        message: "Vulnerability check completed: 0 known vulnerabilities detected (OSV.dev)"
            .to_string(),
        advisory_database: Some("OSV.dev".to_string()),
        cache_status: Some("fresh".to_string()),
        vulnerabilities_found: Some(0),
    });
    let clean_text = clean_reporter.render_text(100, "A+", &[], &[], None);
    assert!(clean_text.contains("Security:      ✓ Clean (0 known vulnerabilities via OSV.dev)"));

    let clean_sarif_output = clean_reporter
        .render_sarif(&[])
        .expect("render clean sarif");
    let clean_sarif: serde_json::Value =
        serde_json::from_str(&clean_sarif_output).expect("parse clean sarif");
    let clean_invocations = clean_sarif["runs"][0]["invocations"]
        .as_array()
        .expect("clean invocations array");
    assert_eq!(clean_invocations[0]["executionSuccessful"], true);
    let clean_run_props = &clean_sarif["runs"][0]["properties"];
    assert_eq!(
        clean_run_props["vulnerability_check"]["status"],
        "completed"
    );
    assert_eq!(
        clean_run_props["vulnerability_check"]["vulnerabilities_found"],
        0
    );
    assert_eq!(clean_run_props["vulnerabilityCheck"]["status"], "completed");

    colored::control::unset_override();
}
