use anyhow::Result;
use colored::Colorize;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{
    Attribute, Cell, Color, ColumnConstraint, ContentArrangement, Row, Table, Width,
};
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

pub fn mask_user_env(text: &str) -> String {
    let mut masked = text.to_string();

    if let Ok(user) = std::env::var("USERNAME") {
        if !user.is_empty() {
            masked = masked.replace(&user, "<USER>");
        }
    }

    if let Ok(user) = std::env::var("USER") {
        if !user.is_empty() {
            masked = masked.replace(&user, "<USER>");
        }
    }

    masked
}

pub fn sanitize_path(path_str: &str) -> String {
    let sanitized = path_str.replace('\\', "/");
    mask_user_env(&sanitized)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<SanitizedProjectConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thresholds: Option<SanitizedThresholdsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ci: Option<SanitizedCiConfig>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub rules: BTreeMap<String, SanitizedRuleSetting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore: Option<SanitizedIgnoreConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub suppressions: Vec<SanitizedSuppressionEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<SanitizedCacheConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scanner: Option<SanitizedScannerConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub custom_rules: Vec<SanitizedCustomRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedProjectConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_score: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedThresholdsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_score: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedCiConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SanitizedRuleSetting {
    Simple(String),
    Detailed {
        #[serde(skip_serializing_if = "Option::is_none")]
        severity: Option<String>,
        enabled: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedIgnoreConfig {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub patterns: Vec<String>,
    pub use_defaults: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedScannerConfig {
    pub max_file_size: u64,
    pub max_depth: usize,
    pub max_files: usize,
    pub follow_symlinks: bool,
    pub fail_on_incomplete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedCacheConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedSuppressionEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub rules: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedCustomRule {
    pub id: String,
    pub name: String,
    pub severity: String,
    pub message: String,
    pub pattern: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub files: Vec<String>,
}

impl SanitizedConfig {
    pub fn from_config(config: &StackvetConfig) -> Self {
        let project = if config.project != stackvet_core::config::ProjectConfig::default() {
            Some(SanitizedProjectConfig {
                name: config.project.name.clone(),
                min_score: config.project.min_score,
                strict: config.project.strict,
            })
        } else {
            None
        };

        let thresholds = if config.thresholds != stackvet_core::config::ThresholdsConfig::default()
        {
            Some(SanitizedThresholdsConfig {
                fail_on: config.thresholds.fail_on.clone(),
                min_score: config.thresholds.min_score,
            })
        } else {
            None
        };

        let ci = if config.ci != stackvet_core::config::CiConfig::default() {
            Some(SanitizedCiConfig {
                baseline: config
                    .ci
                    .baseline
                    .as_ref()
                    .map(|p| sanitize_path(&p.to_string_lossy())),
                fail_on: config.ci.fail_on.clone(),
                strict: config.ci.strict,
            })
        } else {
            None
        };

        let mut rules = BTreeMap::new();
        for (k, v) in &config.rules {
            let sanitized_rule = match v {
                stackvet_core::config::RuleSetting::Simple(s) => {
                    SanitizedRuleSetting::Simple(s.clone())
                }
                stackvet_core::config::RuleSetting::Detailed { severity, enabled } => {
                    SanitizedRuleSetting::Detailed {
                        severity: severity.clone(),
                        enabled: *enabled,
                    }
                }
            };
            rules.insert(k.clone(), sanitized_rule);
        }

        let ignore = if config.ignore != stackvet_core::config::IgnoreConfig::default() {
            Some(SanitizedIgnoreConfig {
                patterns: config
                    .ignore
                    .patterns()
                    .iter()
                    .map(|p| sanitize_path(p))
                    .collect(),
                use_defaults: config.ignore.use_defaults(),
            })
        } else {
            None
        };

        let suppressions = config
            .suppressions
            .iter()
            .map(|s| SanitizedSuppressionEntry {
                rule: s.rule.clone(),
                rules: s.rules.clone(),
                path: s.path.as_ref().map(|p| sanitize_path(p)),
                line: s.line,
                reason: s.reason.as_ref().map(|_| "[REDACTED]".to_string()),
                until: s.until.clone(),
            })
            .collect();

        let cache = if config.cache != stackvet_core::config::CacheConfig::default() {
            Some(SanitizedCacheConfig {
                enabled: config.cache.enabled,
                dir: config
                    .cache
                    .dir
                    .as_ref()
                    .map(|d| sanitize_path(&d.to_string_lossy())),
            })
        } else {
            None
        };

        let scanner = if config.scanner != stackvet_core::config::ScannerConfig::default() {
            Some(SanitizedScannerConfig {
                max_file_size: config.scanner.max_file_size,
                max_depth: config.scanner.max_depth,
                max_files: config.scanner.max_files,
                follow_symlinks: config.scanner.follow_symlinks,
                fail_on_incomplete: config.scanner.fail_on_incomplete,
            })
        } else {
            None
        };

        let custom_rules = config
            .custom_rules
            .iter()
            .map(|r| SanitizedCustomRule {
                id: r.id.clone(),
                name: r.name.clone(),
                severity: r.severity.to_string(),
                message: "[REDACTED]".to_string(),
                pattern: "[REDACTED]".to_string(),
                files: r.files.iter().map(|f| sanitize_path(f)).collect(),
            })
            .collect();

        Self {
            project,
            thresholds,
            ci,
            rules,
            ignore,
            suppressions,
            cache,
            scanner,
            custom_rules,
        }
    }
}

pub fn sanitize_config(config: &StackvetConfig) -> String {
    let sanitized = SanitizedConfig::from_config(config);
    let s = toml::to_string_pretty(&sanitized).unwrap_or_default();
    mask_user_env(s.trim())
}

pub fn sanitize_config_str(content: &str) -> String {
    if let Ok(config) = toml::from_str::<StackvetConfig>(content) {
        return sanitize_config(&config);
    }

    if let Ok(toml::Value::Table(table)) = toml::from_str::<toml::Value>(content) {
        let mut safe_table = toml::map::Map::new();
        for (k, v) in table {
            match k.as_str() {
                "project" | "thresholds" | "ci" | "rules" | "scanner" | "cache" | "ignore" => {
                    safe_table.insert(k, v);
                }
                "custom_rules" => {
                    if let toml::Value::Array(arr) = v {
                        let redacted_arr = arr
                            .into_iter()
                            .map(|rule_val| {
                                if let toml::Value::Table(mut r_tab) = rule_val {
                                    r_tab.insert(
                                        "pattern".to_string(),
                                        toml::Value::String("[REDACTED]".to_string()),
                                    );
                                    r_tab.insert(
                                        "message".to_string(),
                                        toml::Value::String("[REDACTED]".to_string()),
                                    );
                                    toml::Value::Table(r_tab)
                                } else {
                                    rule_val
                                }
                            })
                            .collect();
                        safe_table.insert(k, toml::Value::Array(redacted_arr));
                    }
                }
                _ => {
                    // All unallowlisted tables and fields (e.g. tokens, secrets, auth) are discarded
                }
            }
        }
        if let Ok(s) = toml::to_string_pretty(&safe_table) {
            return mask_user_env(s.trim());
        }
    }

    "# Configuration omitted (unparseable)".to_string()
}

pub fn generate_bug_report(
    root: &Path,
    ctx: &ProjectContext,
    config: &StackvetConfig,
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
        } else if *config != StackvetConfig::default() {
            Some(sanitize_config(config))
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
            incomplete_reasons.push(format!("{}: discovery condition encountered", d.rule_id.0));
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
        "─── StackVet Reproducible Bug Report Summary ──────────────────────────"
            .cyan()
            .bold()
    );

    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w.saturating_sub(2))
        .unwrap_or(100)
        .clamp(80, 130);

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_width(term_width);
    table.set_header(vec![
        Cell::new("Attribute")
            .fg(Color::Cyan)
            .add_attribute(Attribute::Bold),
        Cell::new("Value")
            .fg(Color::Cyan)
            .add_attribute(Attribute::Bold),
    ]);

    if let Some(col) = table.column_mut(0) {
        col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(24)));
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_user_env_preserves_backslashes() {
        let input = r#"path = "C:\Users\SecretUser\Project\file.txt""#;
        // Even if SecretUser isn't USERNAME/USER, backslashes must be preserved
        let output = mask_user_env(input);
        assert!(output.contains(r#"C:\Users\"#));
        assert!(!output.contains(r#"C:/Users/"#));
    }

    #[test]
    fn test_mask_user_env_replaces_active_user() {
        if let Ok(user) = std::env::var("USERNAME").or_else(|_| std::env::var("USER")) {
            if !user.is_empty() {
                let input = format!(r#"user_path = "C:\Users\{user}\data""#);
                let output = mask_user_env(&input);
                assert!(output.contains(r#"C:\Users\<USER>\data"#));
                assert!(!output.contains(&user));
            }
        }
    }

    #[test]
    fn test_sanitize_config_str_preserves_backslashes() {
        let toml_content = r#"
[project]
name = "app\\with\\escapes"
min_score = 80
"#;
        let sanitized = sanitize_config_str(toml_content);
        assert!(
            sanitized.contains(r#"app\\with\\escapes"#)
                || sanitized.contains(r#"app\with\escapes"#)
        );
        assert!(!sanitized.contains("app//with//escapes"));
    }
}
