use anyhow::Result;
use colored::*;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};
use serde::{Deserialize, Serialize};
use stackvet_core::{Diagnostic, HealthScoreExplanation, RootCause, Severity};

pub const SCHEMA_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

impl Default for ToolInfo {
    fn default() -> Self {
        Self {
            name: "stackvet".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            repository: Some("https://github.com/stackvet/stackvet".to_string()),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSummary {
    pub total_issues: usize,
    pub critical_count: usize,
    pub error_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
    pub hint_count: usize,
    pub fixable_count: usize,
    #[serde(default = "default_true")]
    pub is_complete: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incomplete_reasons: Vec<String>,
}

impl Default for ReportSummary {
    fn default() -> Self {
        Self {
            total_issues: 0,
            critical_count: 0,
            error_count: 0,
            warning_count: 0,
            info_count: 0,
            hint_count: 0,
            fixable_count: 0,
            is_complete: true,
            incomplete_reasons: Vec::new(),
        }
    }
}

impl ReportSummary {
    pub fn from_diagnostics(diagnostics: &[Diagnostic]) -> Self {
        let mut summary = Self {
            total_issues: diagnostics.len(),
            ..Default::default()
        };
        for d in diagnostics {
            match d.severity {
                Severity::Critical => summary.critical_count += 1,
                Severity::Error => summary.error_count += 1,
                Severity::Warning => summary.warning_count += 1,
                Severity::Info => summary.info_count += 1,
                Severity::Hint => summary.hint_count += 1,
            }
            if d.fix.is_some() {
                summary.fixable_count += 1;
            }

            if d.rule_id.0.starts_with("DISC-") {
                summary.is_complete = false;
                let reason = format!("{}: {}", d.rule_id.0, d.message);
                if !summary.incomplete_reasons.contains(&reason) {
                    summary.incomplete_reasons.push(reason);
                }
            }
        }
        summary
    }

    pub fn mark_incomplete(&mut self, reason: impl Into<String>) {
        self.is_complete = false;
        let r = reason.into();
        if !self.incomplete_reasons.contains(&r) {
            self.incomplete_reasons.push(r);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullReport {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    #[serde(default)]
    pub tool: ToolInfo,
    #[serde(default)]
    pub summary: ReportSummary,
    pub health_score: u32,
    pub grade: String,
    pub root_causes: Vec<RootCause>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_breakdown: Option<HealthScoreExplanation>,
}

fn default_schema_version() -> String {
    SCHEMA_VERSION.to_string()
}

pub struct Reporter;

impl Default for Reporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Reporter {
    pub fn new() -> Self {
        Self
    }

    /// Renders text report with scorecard, deduction breakdown, root causes, and diagnostics table
    pub fn render_text(
        &self,
        score: u32,
        grade: &str,
        root_causes: &[RootCause],
        diagnostics: &[Diagnostic],
        explanation: Option<&HealthScoreExplanation>,
    ) -> String {
        let mut out = String::new();

        out.push('\n');
        out.push_str(
            &"=======================================================================\n"
                .cyan()
                .bold()
                .to_string(),
        );
        out.push_str(
            &"                StackVet Universal Project Health Report               \n"
                .cyan()
                .bold()
                .to_string(),
        );
        out.push_str(
            &"=======================================================================\n\n"
                .cyan()
                .bold()
                .to_string(),
        );

        // Score Card
        let score_colored = match score {
            90..=100 => format!("{}", score).green().bold(),
            75..=89 => format!("{}", score).yellow().bold(),
            50..=74 => format!("{}", score).truecolor(255, 140, 0).bold(),
            _ => format!("{}", score).red().bold(),
        };

        out.push_str(&format!("  Health Score: {} / 100\n", score_colored));
        out.push_str(&format!("  Overall Grade: {}\n", grade.bold()));

        if let Some(exp) = explanation {
            out.push_str(&format!(
                "  Calculation:   Base 100 - {} deductions ({} unique finding{}, {} duplicate{} ignored)\n",
                exp.total_deductions,
                exp.unique_findings_count,
                if exp.unique_findings_count == 1 { "" } else { "s" },
                exp.duplicate_findings_ignored,
                if exp.duplicate_findings_ignored == 1 { "" } else { "s" }
            ));
            if exp.suppressed_findings_count > 0 {
                out.push_str(&format!(
                    "  Suppression:   {} finding{} suppressed by configuration or inline comments\n",
                    exp.suppressed_findings_count,
                    if exp.suppressed_findings_count == 1 { "" } else { "s" }
                ));
            }
        }
        out.push('\n');

        // Counts
        let criticals = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Critical)
            .count();
        let errors = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let warnings = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count();

        out.push_str(&format!(
            "  Critical: {}  |  Errors: {}  |  Warnings: {}\n\n",
            if criticals > 0 {
                criticals.to_string().red().bold()
            } else {
                "0".green()
            },
            if errors > 0 {
                errors.to_string().red()
            } else {
                "0".green()
            },
            if warnings > 0 {
                warnings.to_string().yellow()
            } else {
                "0".green()
            }
        ));

        let disc_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.rule_id.0.starts_with("DISC-"))
            .collect();
        if !disc_diags.is_empty() {
            out.push_str(
                &"⚠️  Incomplete Analysis / Scanner Hardening Notices:\n"
                    .yellow()
                    .bold()
                    .to_string(),
            );
            for d in disc_diags {
                out.push_str(&format!("  • [{}] {}\n", d.rule_id.0.yellow(), d.message));
            }
            out.push('\n');
        }

        // Score deduction breakdown
        if let Some(exp) = explanation {
            if !exp.rule_contributions.is_empty() {
                out.push_str(
                    &"--- Health Score Deduction Breakdown ---\n"
                        .bold()
                        .to_string(),
                );
                for contrib in &exp.rule_contributions {
                    let sev_badge = match contrib.severity {
                        Severity::Critical => "[CRITICAL]".red().bold(),
                        Severity::Error => "[ERROR]".red(),
                        Severity::Warning => "[WARN]".yellow(),
                        _ => "[INFO]".cyan(),
                    };
                    out.push_str(&format!(
                        "  • {} {}: -{} pts ({} finding{}) - {}\n",
                        sev_badge,
                        contrib.rule_id.bold(),
                        contrib.total_penalty,
                        contrib.count,
                        if contrib.count == 1 { "" } else { "s" },
                        contrib.title
                    ));
                }
                out.push('\n');
            }
        }

        // Root causes section
        if !root_causes.is_empty() {
            out.push_str(
                &format!("--- Root Cause Clusters ({}) ---\n", root_causes.len())
                    .bold()
                    .to_string(),
            );
            for (idx, rc) in root_causes.iter().enumerate() {
                let sev_badge = match rc.impact_severity {
                    Severity::Critical => "[CRITICAL]".red().bold(),
                    Severity::Error => "[ERROR]".red(),
                    Severity::Warning => "[WARN]".yellow(),
                    _ => "[INFO]".cyan(),
                };
                out.push_str(&format!("{}. {} {}\n", idx + 1, sev_badge, rc.title.bold()));
                out.push_str(&format!("   Summary: {}\n", rc.summary));
                out.push_str(&format!(
                    "   Remediation: {}\n",
                    rc.primary_remediation.cyan()
                ));
                out.push_str(&format!("   Symptoms Count: {}\n\n", rc.symptoms.len()));
            }
        }

        // Diagnostics table
        if !diagnostics.is_empty() {
            out.push_str(&"--- Diagnostic Details ---\n".bold().to_string());
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS)
                .set_header(vec![
                    Cell::new("Rule").fg(Color::Cyan),
                    Cell::new("Severity"),
                    Cell::new("Target"),
                    Cell::new("File Location"),
                    Cell::new("Message"),
                ]);

            for d in diagnostics {
                let sev_cell = match d.severity {
                    Severity::Critical => Cell::new("CRITICAL").fg(Color::DarkRed),
                    Severity::Error => Cell::new("ERROR").fg(Color::Red),
                    Severity::Warning => Cell::new("WARN").fg(Color::Yellow),
                    Severity::Info => Cell::new("INFO").fg(Color::Blue),
                    Severity::Hint => Cell::new("HINT").fg(Color::Grey),
                };

                let loc_str = d.location.display_location();

                table.add_row(Row::from(vec![
                    Cell::new(d.rule_id.as_str()),
                    sev_cell,
                    Cell::new(d.language.as_str()),
                    Cell::new(loc_str),
                    Cell::new(&d.message),
                ]));
            }

            out.push_str(&table.to_string());
            out.push_str("\n\n");
        } else {
            out.push_str(
                &"No issues found! Your project stack is pristine.\n\n"
                    .green()
                    .bold()
                    .to_string(),
            );
        }

        out
    }

    /// Renders JSON format with stable schema versioning, tool info, and summary
    pub fn render_json(
        &self,
        score: u32,
        grade: &str,
        root_causes: &[RootCause],
        diagnostics: &[Diagnostic],
        explanation: Option<&HealthScoreExplanation>,
    ) -> Result<String> {
        let report = FullReport {
            schema_version: SCHEMA_VERSION.to_string(),
            tool: ToolInfo::default(),
            summary: ReportSummary::from_diagnostics(diagnostics),
            health_score: score,
            grade: grade.to_string(),
            root_causes: root_causes.to_vec(),
            diagnostics: diagnostics.to_vec(),
            score_breakdown: explanation.cloned(),
        };
        Ok(serde_json::to_string_pretty(&report)?)
    }

    /// Renders OASIS SARIF v2.1.0 JSON format for GitHub Code Scanning
    pub fn render_sarif(&self, diagnostics: &[Diagnostic]) -> Result<String> {
        self.render_sarif_with_root(diagnostics, None)
    }

    /// Renders OASIS SARIF v2.1.0 JSON format relativizing paths against the provided scan root
    pub fn render_sarif_with_root(
        &self,
        diagnostics: &[Diagnostic],
        scan_root: Option<&std::path::Path>,
    ) -> Result<String> {
        let mut rules_map = std::collections::BTreeMap::new();
        for d in diagnostics {
            let r_id = d.rule_id.as_str();
            rules_map.entry(r_id.to_string()).or_insert_with(|| {
                if let Some(meta) = stackvet_core::get_rule_meta(r_id) {
                    serde_json::json!({
                        "id": meta.id,
                        "name": meta.title,
                        "shortDescription": { "text": meta.title },
                        "fullDescription": { "text": meta.description },
                        "help": { "text": format!("{}\n\nRemediation: {}", meta.description, meta.remediation) },
                        "helpUri": format!("https://github.com/stackvet/stackvet/blob/main/docs/rules/{}.md", meta.id),
                        "defaultConfiguration": {
                            "level": match meta.severity {
                                "CRITICAL" | "ERROR" => "error",
                                "WARN" | "WARNING" => "warning",
                                _ => "note",
                            }
                        },
                        "properties": {
                            "category": meta.category,
                            "tags": [meta.category]
                        }
                    })
                } else {
                    serde_json::json!({
                        "id": r_id,
                        "shortDescription": { "text": r_id }
                    })
                }
            });
        }
        let driver_rules: Vec<serde_json::Value> = rules_map.into_values().collect();

        let results: Vec<serde_json::Value> = diagnostics
            .iter()
            .map(|d| {
                let level = match d.severity {
                    Severity::Critical | Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info | Severity::Hint => "note",
                };

                let mut result_obj = serde_json::json!({
                    "ruleId": d.rule_id.0,
                    "level": level,
                    "message": { "text": d.message }
                });

                // Locations: only emit if physical location exists (explicit, not fabricated)
                if let Some((uri, is_relative)) =
                    relativize_sarif_uri(&d.location.file_path, scan_root)
                {
                    let mut region_obj = serde_json::Map::new();
                    if let Some(span) = &d.location.span {
                        region_obj
                            .insert("startLine".to_string(), serde_json::json!(span.start_line));
                        region_obj
                            .insert("startColumn".to_string(), serde_json::json!(span.start_col));
                        region_obj.insert("endLine".to_string(), serde_json::json!(span.end_line));
                        region_obj.insert("endColumn".to_string(), serde_json::json!(span.end_col));
                    }
                    if let Some(snippet) = &d.location.snippet {
                        region_obj.insert(
                            "snippet".to_string(),
                            serde_json::json!({ "text": snippet }),
                        );
                    }

                    let mut artifact_loc = serde_json::json!({
                        "uri": uri,
                    });
                    if is_relative {
                        artifact_loc["uriBaseId"] = serde_json::json!("%SRCROOT%");
                    }

                    let mut phys_loc = serde_json::json!({
                        "artifactLocation": artifact_loc
                    });
                    if !region_obj.is_empty() {
                        phys_loc["region"] = serde_json::Value::Object(region_obj);
                    }

                    result_obj["locations"] = serde_json::json!([{
                        "physicalLocation": phys_loc
                    }]);
                } else {
                    result_obj["locations"] = serde_json::json!([]);
                }

                // Related locations (e.g. cycle hops, multiple import sites)
                if !d.related_locations.is_empty() {
                    let related: Vec<serde_json::Value> = d
                        .related_locations
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, rel)| {
                            let (uri, is_relative) =
                                relativize_sarif_uri(&rel.file_path, scan_root)?;
                            let mut artifact_loc = serde_json::json!({
                                "uri": uri,
                            });
                            if is_relative {
                                artifact_loc["uriBaseId"] = serde_json::json!("%SRCROOT%");
                            }
                            let mut phys = serde_json::json!({
                                "artifactLocation": artifact_loc
                            });
                            if let Some(span) = &rel.span {
                                phys["region"] = serde_json::json!({
                                    "startLine": span.start_line,
                                    "startColumn": span.start_col,
                                    "endLine": span.end_line,
                                    "endColumn": span.end_col
                                });
                            }
                            let mut loc_val = serde_json::json!({
                                "id": idx + 1,
                                "physicalLocation": phys
                            });
                            if let Some(msg) = &rel.message {
                                loc_val["message"] = serde_json::json!({ "text": msg });
                            }
                            Some(loc_val)
                        })
                        .collect();
                    if !related.is_empty() {
                        result_obj["relatedLocations"] = serde_json::Value::Array(related);
                    }
                }

                // Suggested fixes: emit only when fix contains a specific code span
                if let Some(fix) = &d.fix {
                    if let Some(span) = &fix.span {
                        let (fix_uri, is_relative) =
                            relativize_sarif_uri(&fix.file_path, scan_root).unwrap_or_else(|| {
                                (
                                    fix.file_path
                                        .to_string_lossy()
                                        .replace(r"\\?\", "")
                                        .replace('\\', "/"),
                                    true,
                                )
                            });
                        let mut artifact_loc = serde_json::json!({
                            "uri": fix_uri,
                        });
                        if is_relative {
                            artifact_loc["uriBaseId"] = serde_json::json!("%SRCROOT%");
                        }
                        result_obj["fixes"] = serde_json::json!([{
                            "description": { "text": fix.description },
                            "fileChanges": [{
                                "artifactLocation": artifact_loc,
                                "replacements": [{
                                    "deletedRegion": {
                                        "startLine": span.start_line,
                                        "startColumn": span.start_col,
                                        "endLine": span.end_line,
                                        "endColumn": span.end_col
                                    },
                                    "insertedContent": { "text": fix.replacement }
                                }]
                            }]
                        }]);
                    }
                }

                result_obj
            })
            .collect();

        let mut driver_obj = serde_json::json!({
            "name": "stackvet",
            "informationUri": "https://github.com/stackvet/stackvet",
            "version": env!("CARGO_PKG_VERSION")
        });
        if !driver_rules.is_empty() {
            driver_obj["rules"] = serde_json::Value::Array(driver_rules);
        }

        let sarif = serde_json::json!({
            "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
            "version": "2.1.0",
            "runs": [{
                "tool": {
                    "driver": driver_obj
                },
                "results": results
            }]
        });

        Ok(serde_json::to_string_pretty(&sarif)?)
    }

    /// Renders standalone responsive HTML report with dark mode and score breakdown
    pub fn render_html(
        &self,
        score: u32,
        grade: &str,
        root_causes: &[RootCause],
        diagnostics: &[Diagnostic],
        explanation: Option<&HealthScoreExplanation>,
    ) -> String {
        let mut rows = String::new();
        for d in diagnostics {
            let sev_color = match d.severity {
                Severity::Critical => "#ff4d4f",
                Severity::Error => "#ff7875",
                Severity::Warning => "#ffa940",
                _ => "#4096ff",
            };
            let loc_str = d.location.display_location();
            rows.push_str(&format!(
                "<tr><td><code>{}</code></td><td><span style=\"color:{};font-weight:bold;\">{}</span></td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
                d.rule_id.as_str(), sev_color, d.severity.as_str(), d.language.as_str(), loc_str, d.message
            ));
        }

        let breakdown_html = if let Some(exp) = explanation {
            format!(
                "<p style=\"color: #94a3b8; font-size: 0.95rem; margin-top: 0.5rem;\">Base 100 &minus; {} deductions ({} findings evaluated, {} duplicates ignored)</p>",
                exp.total_deductions,
                exp.unique_findings_count,
                exp.duplicate_findings_ignored
            )
        } else {
            String::new()
        };

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>StackVet Project Health Report</title>
<style>
  :root {{ --bg: #0f172a; --card: #1e293b; --text: #f8fafc; --accent: #38bdf8; --border: #334155; }}
  body {{ margin: 0; padding: 2rem; background: var(--bg); color: var(--text); font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }}
  .container {{ max-width: 1100px; margin: 0 auto; }}
  .header {{ display: flex; justify-content: space-between; align-items: center; border-bottom: 2px solid var(--border); padding-bottom: 1rem; margin-bottom: 2rem; }}
  .score-card {{ background: var(--card); border-radius: 12px; padding: 1.5rem; display: flex; gap: 2rem; align-items: center; margin-bottom: 2rem; }}
  .score-val {{ font-size: 3.5rem; font-weight: 800; color: var(--accent); }}
  table {{ width: 100%; border-collapse: collapse; background: var(--card); border-radius: 8px; overflow: hidden; }}
  th, td {{ padding: 12px 16px; text-align: left; border-bottom: 1px solid var(--border); }}
  th {{ background: #182234; color: var(--accent); font-weight: 600; }}
  code {{ background: rgba(255,255,255,0.08); padding: 2px 6px; border-radius: 4px; font-family: monospace; }}
</style>
</head>
<body>
<div class="container">
  <div class="header">
    <h1>StackVet Project Health Report</h1>
    <span>Universal Diagnostics</span>
  </div>
  <div class="score-card">
    <div class="score-val">{score}</div>
    <div>
      <h2>Grade: {grade}</h2>
      <p>Total Diagnostics: {total} | Root Cause Clusters: {clusters}</p>
      {breakdown_html}
    </div>
  </div>
  <h2>Diagnostics Details</h2>
  <table>
    <thead>
      <tr><th>Rule</th><th>Severity</th><th>Target</th><th>Location</th><th>Message</th></tr>
    </thead>
    <tbody>
      {rows}
    </tbody>
  </table>
</div>
</body>
</html>"#,
            score = score,
            grade = grade,
            total = diagnostics.len(),
            clusters = root_causes.len(),
            breakdown_html = breakdown_html,
            rows = rows
        )
    }
}

fn relativize_sarif_uri(
    file_path: &std::path::Path,
    scan_root: Option<&std::path::Path>,
) -> Option<(String, bool)> {
    let raw = file_path.to_string_lossy();
    if raw.is_empty() || raw == "<unknown>" || raw == "-" {
        return None;
    }
    let p_clean = raw.replace(r"\\?\", "").replace('\\', "/");
    let p = std::path::Path::new(&p_clean);

    if !p.is_absolute() {
        // Already relative to scan root
        return Some((p_clean, true));
    }

    let root_buf = scan_root
        .map(|r| r.canonicalize().unwrap_or_else(|_| r.to_path_buf()))
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|c| c.canonicalize().unwrap_or(c))
        });

    if let Some(root_path) = root_buf {
        let root_raw = root_path.to_string_lossy();
        let root_clean = root_raw.replace(r"\\?\", "").replace('\\', "/");
        let root_trimmed = root_clean.trim_end_matches('/');

        let matches_prefix = if cfg!(windows) {
            let p_lower = p_clean.to_lowercase();
            let root_lower = root_trimmed.to_lowercase();
            p_lower.starts_with(&format!("{}/", root_lower)) || p_lower == root_lower
        } else {
            p_clean.starts_with(&format!("{}/", root_trimmed)) || p_clean == root_trimmed
        };

        if matches_prefix {
            let rel = if p_clean.len() > root_trimmed.len() {
                p_clean[root_trimmed.len()..].trim_start_matches('/')
            } else {
                ""
            };
            return Some((rel.to_string(), true));
        }

        // Try canonicalized paths as fallback if paths exist on filesystem
        if let Ok(canonical_p) = file_path.canonicalize() {
            if let Ok(canonical_root) = root_path.canonicalize() {
                if let Ok(stripped) = canonical_p.strip_prefix(&canonical_root) {
                    let rel_str = stripped
                        .to_string_lossy()
                        .replace(r"\\?\", "")
                        .replace('\\', "/");
                    return Some((rel_str, true));
                }
            }
        }
    }

    // Could not relativize to root: emit absolute file:// URI and omit uriBaseId
    let abs_str = if p_clean.starts_with('/') {
        format!("file://{}", p_clean)
    } else {
        format!("file:///{}", p_clean)
    };
    Some((abs_str, false))
}
