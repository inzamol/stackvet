use stackvet_core::{evaluate_custom_rules, CustomRuleDefinition, Severity, StackvetConfig};
use std::fs;
use std::path::PathBuf;

fn temp_test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("stackvet_test_custom_rules")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_custom_rule_parsing_from_toml() {
    let toml_str = r#"
[[custom_rules]]
id = "CUST-001"
name = "Ban console.log"
severity = "error"
category = "hygiene"
pattern = "console\\.log\\("
files = ["src/**/*.ts", "src/**/*.js"]
message = "Disallowed console.log detected in production source"
remediation = "Remove console.log calls before pushing to production."
"#;

    let config: StackvetConfig = toml::from_str(toml_str).expect("parse toml");
    assert_eq!(config.custom_rules.len(), 1);

    let rule = &config.custom_rules[0];
    assert_eq!(rule.id, "CUST-001");
    assert_eq!(rule.name, "Ban console.log");
    assert_eq!(rule.severity, Severity::Error);
    assert_eq!(rule.category, "hygiene");
    assert_eq!(rule.pattern, "console\\.log\\(");
    assert_eq!(rule.files.len(), 2);
}

#[test]
fn test_custom_rule_evaluation_and_span() {
    let dir = temp_test_dir("eval_span");
    let src_file = dir.join("main.ts");
    fs::write(
        &src_file,
        "const x = 42;\nconsole.log(\"debug\", x);\nconst y = 100;\n",
    )
    .unwrap();

    let rule = CustomRuleDefinition {
        id: "CUST-001".to_string(),
        name: "Ban console.log".to_string(),
        severity: Severity::Error,
        category: "custom".to_string(),
        pattern: "console\\.log\\(".to_string(),
        files: vec!["**/*.ts".to_string()],
        message: "No console.log allowed".to_string(),
        remediation: "Remove console.log call".to_string(),
    };

    let diags = evaluate_custom_rules(&dir, std::slice::from_ref(&src_file), &[rule]);
    assert_eq!(diags.len(), 1);

    let d = &diags[0];
    assert_eq!(d.rule_id.0, "CUST-001");
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.message, "No console.log allowed");

    // Line 2, column 1
    let span = d.location.span.expect("span present");
    assert_eq!(span.start_line, 2);
    assert_eq!(span.start_col, 1);
    assert_eq!(
        d.location.snippet.as_deref(),
        Some("console.log(\"debug\", x);")
    );
}

#[test]
fn test_custom_rule_file_pattern_filter() {
    let dir = temp_test_dir("file_filter");
    let py_file = dir.join("app.py");
    fs::write(&py_file, "print(\"hello\")\nconsole.log('hi')\n").unwrap();

    let rule = CustomRuleDefinition {
        id: "CUST-001".to_string(),
        name: "Ban console.log in TS only".to_string(),
        severity: Severity::Warning,
        category: "custom".to_string(),
        pattern: "console\\.log".to_string(),
        files: vec!["**/*.ts".to_string()], // only .ts files!
        message: "No console.log in TS".to_string(),
        remediation: "Remove".to_string(),
    };

    // py_file should be ignored by the file filter
    let diags = evaluate_custom_rules(&dir, &[py_file], &[rule]);
    assert!(diags.is_empty());
}
