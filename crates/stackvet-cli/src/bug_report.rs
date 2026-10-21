use anyhow::Result;
use colored::Colorize;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};
use serde::{Deserialize, Serialize};
use stackvet_core::{Diagnostic, ProjectContext, StackvetConfig};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugReportEnvironment {
    pub stackvet_version: String,
    pub os: String,
    pub arch: String,
    pub detected_runtimes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugReportProjectSummary {
    pub root_name: String,
    pub detected_languages: Vec<String>,
    pub manifest_files: Vec<String>,
    pub source_files_count: usize,
    pub has_docker: bool,
    pub has_ci: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugReportData {
    pub report_schema_version: String,
    pub timestamp: String,
    pub environment: BugReportEnvironment,
    pub project: BugReportProjectSummary,
    pub sanitized_config: Option<String>,
    pub diagnostic_counts_by_rule: BTreeMap<String, usize>,
    pub incomplete_reasons: Vec<String>,
}

pub fn check_runtime_version(cmd: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(cmd);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        command.creation_flags(0x08000000);
    }
    match command.output() {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                Some(s.lines().next().unwrap_or(&s).to_string())
            } else {
                let err_s = String::from_utf8_lossy(&out.stderr).trim().to_string();
                if !err_s.is_empty() {
                    Some(err_s.lines().next().unwrap_or(&err_s).to_string())
                } else {
                    None
                }
            }
        }
        _ => None,
    }
}

pub fn sanitize_path(path_str: &str) -> String {
    let mut sanitized = path_str.replace('\\', "/");

    // Mask Windows home directories: C:/Users/<username>/... -> ~/...
    if let Ok(user) = std::env::var("USERNAME") {
        if !user.is_empty() {
            let win_user_pattern = format!("Users/{}", user);
            sanitized = sanitized.replace(&win_user_pattern, "Users/<USER>");
            sanitized = sanitized.replace(&user, "<USER>");
        }
    }

    // Mask Unix home directories: /home/<username>/... -> ~/...
    if let Ok(user) = std::env::var("USER") {
        if !user.is_empty() {
            let unix_user_pattern = format!("home/{}", user);
            sanitized = sanitized.replace(&unix_user_pattern, "home/<USER>");
            sanitized = sanitized.replace(&user, "<USER>");
        }
    }

    sanitized
}

pub fn sanitize_config_str(content: &str) -> String {
    let mut output = Vec::new();
    for line in content.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("token")
            || lower.contains("secret")
            || lower.contains("key")
            || lower.contains("password")
            || lower.contains("auth")
        {
            if let Some((k, _)) = line.split_once('=') {
                output.push(format!("{}= \"[REDACTED]\"", k.trim()));
                continue;
            }
        }
        output.push(sanitize_path(line));
    }
    output.join("\n")
}

pub fn generate_bug_report(
    root: &Path,
    ctx: &ProjectContext,
    _config: &StackvetConfig,
    diagnostics: &[Diagnostic],
    config_path: Option<&Path>,
) -> Result<BugReportData> {
    // 1. Detect runtimes
    let mut runtimes = BTreeMap::new();
    let runtime_checks: &[(&str, &[&str])] = &[
        ("node", &["--version"]),
        ("npm", &["--version"]),
        ("pnpm", &["--version"]),
        ("yarn", &["--version"]),
        ("bun", &["--version"]),
        ("python", &["--version"]),
        ("python3", &["--version"]),
        ("uv", &["--version"]),
        ("poetry", &["--version"]),
        ("cargo", &["--version"]),
        ("git", &["--version"]),
        ("docker", &["--version"]),
    ];

    for (cmd, args) in runtime_checks {
        if let Some(ver) = check_runtime_version(cmd, args) {
            runtimes.insert(cmd.to_string(), ver);
        }
    }

    // 2. Project summary
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project_root".to_string());

    let mut detected_languages = Vec::new();
    if ctx.python.is_some() {
        detected_languages.push("Python".to_string());
    }
    if ctx.node.is_some() {
        detected_languages.push("Node.js".to_string());
    }
    if !ctx.dockerfiles.is_empty() {
        detected_languages.push("Docker".to_string());
    }
    if !ctx.ci_workflows.is_empty() {
        detected_languages.push("CI/CD".to_string());
    }

    let manifest_files = ctx
        .manifest_files
        .iter()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string())
        })
        .collect();

    // 3. Sanitized config
    let sanitized_config = if let Some(cp) = config_path {
        if cp.is_file() {
            fs::read_to_string(cp).ok().map(|s| sanitize_config_str(&s))
        } else {
            None
        }
    } else {
        let default_cfg = root.join(".stackvet.toml");
        if default_cfg.is_file() {
            fs::read_to_string(&default_cfg)
                .ok()
                .map(|s| sanitize_config_str(&s))
        } else {
            None
        }
    };

    // 4. Diagnostic counts
    let mut counts_by_rule = BTreeMap::new();
    let mut incomplete_reasons = Vec::new();

    for d in diagnostics {
        *counts_by_rule.entry(d.rule_id.0.clone()).or_insert(0) += 1;
    }
    for d in &ctx.discovery_diagnostics {
        if d.rule_id.0.starts_with("DISC-") {
            incomplete_reasons.push(sanitize_path(&d.message));
        }
    }

    let now_iso = {
        let duration = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("{duration} (unix epoch)")
    };

    Ok(BugReportData {
        report_schema_version: "1.0.0".to_string(),
        timestamp: now_iso,
        environment: BugReportEnvironment {
            stackvet_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            detected_runtimes: runtimes,
        },
        project: BugReportProjectSummary {
            root_name,
            detected_languages,
            manifest_files,
            source_files_count: ctx.source_files.len(),
            has_docker: !ctx.dockerfiles.is_empty(),
            has_ci: !ctx.ci_workflows.is_empty(),
        },
        sanitized_config,
        diagnostic_counts_by_rule: counts_by_rule,
        incomplete_reasons,
    })
}

pub fn print_bug_report_summary(report: &BugReportData) {
    println!(
        "\n{}",
        "=== StackVet Reproducible Bug Report Summary ==="
            .cyan()
            .bold()
    );

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        Cell::new("Attribute").fg(Color::Cyan),
        Cell::new("Value").fg(Color::Cyan),
    ]);

    table.add_row(Row::from(vec![
        Cell::new("StackVet Version"),
        Cell::new(&report.environment.stackvet_version),
    ]));
    table.add_row(Row::from(vec![
        Cell::new("Platform (OS / Arch)"),
        Cell::new(format!(
            "{}/{}",
            report.environment.os, report.environment.arch
        )),
    ]));
    table.add_row(Row::from(vec![
        Cell::new("Target Project"),
        Cell::new(&report.project.root_name),
    ]));
    table.add_row(Row::from(vec![
        Cell::new("Source Files Count"),
        Cell::new(report.project.source_files_count.to_string()),
    ]));
    table.add_row(Row::from(vec![
        Cell::new("Detected Languages"),
        Cell::new(report.project.detected_languages.join(", ")),
    ]));
    table.add_row(Row::from(vec![
        Cell::new("Detected Manifests"),
        Cell::new(report.project.manifest_files.join(", ")),
    ]));

    let runtimes_summary = if report.environment.detected_runtimes.is_empty() {
        "None detected".to_string()
    } else {
        report
            .environment
            .detected_runtimes
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    table.add_row(Row::from(vec![
        Cell::new("Installed Runtimes"),
        Cell::new(runtimes_summary),
    ]));

    println!("{table}\n");

    if !report.incomplete_reasons.is_empty() {
        println!("{}", "Discovery / Hardening Warnings:".yellow().bold());
        for reason in &report.incomplete_reasons {
            println!("  • {reason}");
        }
        println!();
    }
}
