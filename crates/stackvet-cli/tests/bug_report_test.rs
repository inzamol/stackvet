use stackvet_core::ExitCode;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn make_temp_repo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "stackvet_test_bug_report_{}_{}",
        name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_cli_bug_report_json() {
    let repo = make_temp_repo("json_output");
    fs::write(repo.join("main.py"), "import os\n").unwrap();
    fs::write(repo.join("requirements.txt"), "flask==2.0.1\n").unwrap();
    fs::write(
        repo.join(".stackvet.toml"),
        "[project]\nname = \"test-app\"\nmin_score = 75\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_stackvet");
    let output = Command::new(exe)
        .arg("--path")
        .arg(&repo)
        .arg("bug-report")
        .arg("--json")
        .output()
        .expect("Failed to execute stackvet bug-report");

    assert_eq!(
        output.status.code(),
        Some(ExitCode::Success.as_i32()),
        "Expected exit code 0"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("Valid JSON report output");

    assert_eq!(parsed["report_schema_version"], "1.0.0");
    assert!(parsed["environment"]["stackvet_version"].is_string());
    assert!(parsed["environment"]["os"].is_string());
    assert!(parsed["environment"]["detected_runtimes"].is_object());

    // Project summary checks
    assert_eq!(parsed["project"]["has_docker"], false);
    assert!(parsed["project"]["source_files_count"].as_u64().unwrap() >= 1);
    let langs = parsed["project"]["detected_languages"].as_array().unwrap();
    assert!(langs.iter().any(|l| l == "Python"));

    // Sanitized config
    let cfg = parsed["sanitized_config"].as_str().unwrap();
    assert!(cfg.contains("name = \"test-app\""));

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_cli_bug_report_to_file() {
    let repo = make_temp_repo("file_output");
    fs::write(repo.join("package.json"), "{\"name\":\"test-node\"}\n").unwrap();
    fs::write(repo.join("index.js"), "console.log('hi');\n").unwrap();

    let out_file = repo.join("out_report.json");
    let exe = env!("CARGO_BIN_EXE_stackvet");
    let output = Command::new(exe)
        .arg("--path")
        .arg(&repo)
        .arg("bug-report")
        .arg("-o")
        .arg(&out_file)
        .arg("--json")
        .output()
        .expect("Failed to execute stackvet bug-report");

    assert_eq!(output.status.code(), Some(ExitCode::Success.as_i32()));
    assert!(out_file.exists());

    let file_content = fs::read_to_string(&out_file).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&file_content).unwrap();
    assert_eq!(parsed["report_schema_version"], "1.0.0");

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_cli_bug_report_summary_text() {
    let repo = make_temp_repo("text_summary");
    fs::write(repo.join("Dockerfile"), "FROM alpine:3.18\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_stackvet");
    let output = Command::new(exe)
        .arg("--path")
        .arg(&repo)
        .arg("bug-report")
        .output()
        .expect("Failed to execute stackvet bug-report");

    assert_eq!(output.status.code(), Some(ExitCode::Success.as_i32()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("StackVet Reproducible Bug Report Summary"));
    assert!(stdout.contains("StackVet Version"));
    assert!(stdout.contains("Platform (OS / Arch)"));

    let _ = fs::remove_dir_all(&repo);
}

#[test]
fn test_bug_report_sanitizer_strict_allowlist_redaction() {
    let repo = make_temp_repo("secret_leak_test");
    fs::write(repo.join("main.py"), "print('hello')\n").unwrap();

    let raw_config = r#"
[project]
name = "secure-app"
min_score = 85

[tokens]
github_token = "ghp_1234567890abcdef"
aws_secret = """
AKIAIOSFODNN7EXAMPLE
SECRET_KEY_LINE_2
"""

[[custom_rules]]
id = "CR-001"
name = "Disallow secret API"
severity = "error"
pattern = "(?i)super_secret_token_regex_[0-9]+"
message = "Found internal credential with https://internal-vault.corp/api"
files = ["src/**/*.py"]
"#;

    fs::write(repo.join(".stackvet.toml"), raw_config).unwrap();

    let exe = env!("CARGO_BIN_EXE_stackvet");
    let output = Command::new(exe)
        .arg("--path")
        .arg(&repo)
        .arg("bug-report")
        .arg("--json")
        .output()
        .expect("bug report execution");

    assert_eq!(output.status.code(), Some(ExitCode::Success.as_i32()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let cfg = parsed["sanitized_config"].as_str().unwrap();

    // 1. Allowlisted project fields are kept
    assert!(cfg.contains("name = \"secure-app\""));
    assert!(cfg.contains("min_score = 85"));

    // 2. Unallowlisted tables and multi-line secrets are completely dropped
    assert!(!cfg.contains("ghp_1234567890abcdef"));
    assert!(!cfg.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(!cfg.contains("SECRET_KEY_LINE_2"));
    assert!(!cfg.contains("[tokens]"));

    // 3. Custom rules patterns and messages are strictly redacted
    assert!(!cfg.contains("super_secret_token_regex_"));
    assert!(!cfg.contains("https://internal-vault.corp/api"));
    assert!(cfg.contains("pattern = \"[REDACTED]\""));
    assert!(cfg.contains("message = \"[REDACTED]\""));
    assert!(cfg.contains("id = \"CR-001\""));

    let _ = fs::remove_dir_all(&repo);
}
