use anyhow::Result;
use colored::*;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{
    Attribute, Cell, Color, ColumnConstraint, ContentArrangement, Row, Table, Width,
};
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

pub use stackvet_core::{VulnerabilityCheckStatus, VulnerabilityCheckSummary};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vulnerability_check: Option<VulnerabilityCheckSummary>,
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
            vulnerability_check: None,
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

        // Evaluate vulnerability scan status from diagnostics
        let sec_001 = diagnostics.iter().find(|d| d.rule_id.0 == "SEC-001");
        let sec_002 = diagnostics.iter().find(|d| d.rule_id.0 == "SEC-002");
        let sec_003 = diagnostics.iter().find(|d| d.rule_id.0 == "SEC-003");
        let dep_006_count = diagnostics
            .iter()
            .filter(|d| d.rule_id.0 == "DEP-006")
            .count();

        if let Some(d) = sec_001 {
            summary.is_complete = false;
            let reason = format!("{}: {}", d.rule_id.0, d.message);
            if !summary.incomplete_reasons.contains(&reason) {
                summary.incomplete_reasons.push(reason);
            }
            let is_offline = d.message.to_lowercase().contains("offline mode");
            let is_corrupted = d.message.to_lowercase().contains("corrupt");
            let (status, cache_status) = if is_offline {
                (VulnerabilityCheckStatus::Skipped, "missing")
            } else if is_corrupted {
                (VulnerabilityCheckStatus::Failed, "corrupted")
            } else {
                (VulnerabilityCheckStatus::Failed, "missing")
            };
            summary.vulnerability_check = Some(VulnerabilityCheckSummary {
                status,
                message: d.message.clone(),
                advisory_database: Some("OSV.dev".to_string()),
                cache_status: Some(cache_status.to_string()),
                vulnerabilities_found: None,
            });
        } else if sec_003.is_some() || sec_002.is_some() {
            let cache_status = if sec_002.is_some() { "stale" } else { "cached" };
            let msg = if let Some(s3) = sec_003 {
                s3.message.clone()
            } else {
                sec_002.unwrap().message.clone()
            };
            summary.vulnerability_check = Some(VulnerabilityCheckSummary {
                status: VulnerabilityCheckStatus::CompletedWithWarnings,
                message: msg,
                advisory_database: Some("OSV.dev".to_string()),
                cache_status: Some(cache_status.to_string()),
                vulnerabilities_found: Some(dep_006_count),
            });
        } else if dep_006_count > 0 {
            // DEP-006 findings are present without SEC- warnings
            summary.vulnerability_check = Some(VulnerabilityCheckSummary {
                status: VulnerabilityCheckStatus::Completed,
                message: format!(
                    "Vulnerability check completed: {} finding(s) detected via OSV.dev",
                    dep_006_count
                ),
                advisory_database: Some("OSV.dev".to_string()),
                cache_status: None,
                vulnerabilities_found: Some(dep_006_count),
            });
        } else {
            // Do NOT infer a completed fresh vulnerability check simply from the absence of SEC-* diagnostics.
            // Scans that were not run or without observed outcome remain None here.
            summary.vulnerability_check = None;
        }

        summary
    }

    pub fn with_vulnerability_check(mut self, check: VulnerabilityCheckSummary) -> Self {
        self.vulnerability_check = Some(check);
        self
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

#[derive(Debug, Clone, Default)]
pub struct Reporter {
    scan_root: Option<std::path::PathBuf>,
    vulnerability_check: Option<VulnerabilityCheckSummary>,
}

impl Reporter {
    pub fn new() -> Self {
        Self {
            scan_root: None,
            vulnerability_check: None,
        }
    }

    pub fn with_root(scan_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            scan_root: Some(scan_root.into()),
            vulnerability_check: None,
        }
    }

    pub fn with_vulnerability_check(mut self, check: VulnerabilityCheckSummary) -> Self {
        self.vulnerability_check = Some(check);
        self
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
        let banner_title = "StackVet Universal Project Health Report";
        let banner_box = format!(
            "╭─────────────────────────────────────────────────────────────────────╮\n\
             │              {}               │\n\
             ╰─────────────────────────────────────────────────────────────────────╯\n\n",
            banner_title
        );
        out.push_str(&banner_box.cyan().bold().to_string());

        // Score Card with visual progress bar and grade badge
        let score_colored = match score {
            90..=100 => format!("{}", score).green().bold(),
            75..=89 => format!("{}", score).yellow().bold(),
            50..=74 => format!("{}", score).truecolor(255, 140, 0).bold(),
            _ => format!("{}", score).red().bold(),
        };

        let grade_badge = match grade.chars().next().unwrap_or('F') {
            'A' => format!("[ Grade: {} ]", grade).green().bold(),
            'B' => format!("[ Grade: {} ]", grade).yellow().bold(),
            'C' => format!("[ Grade: {} ]", grade)
                .truecolor(255, 140, 0)
                .bold(),
            _ => format!("[ Grade: {} ]", grade).red().bold(),
        };

        let filled_blocks = ((score as usize * 20) + 50) / 100;
        let empty_blocks = 20usize.saturating_sub(filled_blocks);
        let bar_filled = "█".repeat(filled_blocks);
        let bar_empty = "░".repeat(empty_blocks);
        let bar_colored = match score {
            90..=100 => format!("{}{}", bar_filled.green(), bar_empty.dimmed()),
            75..=89 => format!("{}{}", bar_filled.yellow(), bar_empty.dimmed()),
            50..=74 => format!(
                "{}{}",
                bar_filled.truecolor(255, 140, 0),
                bar_empty.dimmed()
            ),
            _ => format!("{}{}", bar_filled.red(), bar_empty.dimmed()),
        };

        out.push_str(&format!(
            "  Health Score:  [{}] {} / 100   {}\n",
            bar_colored, score_colored, grade_badge
        ));
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

        // Counts with color indicators
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
        let infos = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Info || d.severity == Severity::Hint)
            .count();

        let crit_str = if criticals > 0 {
            format!("● {} Critical", criticals).red().bold()
        } else {
            "● 0 Critical".green()
        };
        let err_str = if errors > 0 {
            format!("● {} Errors", errors).red().bold()
        } else {
            "● 0 Errors".green()
        };
        let warn_str = if warnings > 0 {
            format!("▲ {} Warnings", warnings).yellow().bold()
        } else {
            "▲ 0 Warnings".green()
        };
        let info_str = if infos > 0 {
            format!("ℹ {} Info", infos).cyan()
        } else {
            "ℹ 0 Info".dimmed()
        };

        out.push_str(&format!(
            "  Metrics:       {}  │  {}  │  {}  │  {}\n",
            crit_str, err_str, warn_str, info_str
        ));

        let summary_model = ReportSummary::from_diagnostics(diagnostics);
        let v_check_opt = self
            .vulnerability_check
            .as_ref()
            .or(summary_model.vulnerability_check.as_ref());
        if let Some(v_check) = v_check_opt {
            let sec_str = match v_check.status {
                VulnerabilityCheckStatus::Completed => {
                    let count = v_check.vulnerabilities_found.unwrap_or(0);
                    if count == 0 {
                        "✓ Clean (0 known vulnerabilities via OSV.dev)"
                            .green()
                            .to_string()
                    } else {
                        format!(
                            "✗ {} known vulnerability finding(s) detected via OSV.dev",
                            count
                        )
                        .red()
                        .bold()
                        .to_string()
                    }
                }
                VulnerabilityCheckStatus::CompletedWithWarnings => {
                    format!("▲ Completed with fallback ({})", v_check.message)
                        .yellow()
                        .bold()
                        .to_string()
                }
                VulnerabilityCheckStatus::Skipped => format!("⚠ Skipped ({})", v_check.message)
                    .yellow()
                    .bold()
                    .to_string(),
                VulnerabilityCheckStatus::Failed => format!("✗ Unavailable ({})", v_check.message)
                    .red()
                    .bold()
                    .to_string(),
                VulnerabilityCheckStatus::NotRun => format!("- Not run ({})", v_check.message)
                    .dimmed()
                    .to_string(),
            };
            out.push_str(&format!("  Security:      {}\n", sec_str));
        }
        out.push('\n');

        let disc_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.rule_id.0.starts_with("DISC-") || d.rule_id.0 == "SEC-001")
            .collect();
        if !disc_diags.is_empty() {
            out.push_str(
                &"⚠️  Incomplete Analysis / Scanner Hardening Notices:\n"
                    .yellow()
                    .bold()
                    .to_string(),
            );
            for d in disc_diags {
                let clean_msg = d.message.replace(r"\\?\", "");
                out.push_str(&format!(
                    "  • [{}] {}\n",
                    d.rule_id.0.yellow().bold(),
                    clean_msg
                ));
            }
            out.push('\n');
        }

        // Score deduction breakdown
        if let Some(exp) = explanation {
            if !exp.rule_contributions.is_empty() {
                out.push_str(&format!(
                    "{}\n",
                    "─── Health Score Deduction Breakdown ──────────────────────────────────"
                        .cyan()
                        .bold()
                ));
                for contrib in &exp.rule_contributions {
                    let sev_badge = match contrib.severity {
                        Severity::Critical => "[CRITICAL]".red().bold(),
                        Severity::Error => "[ERROR]".red(),
                        Severity::Warning => "[WARN]".yellow(),
                        _ => "[INFO]".cyan(),
                    };
                    out.push_str(&format!(
                        "  • {:<10} {:<10} {:<10} ({} finding{}) ── {}\n",
                        sev_badge,
                        contrib.rule_id.bold().cyan(),
                        format!("-{} pts", contrib.total_penalty).red(),
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
            out.push_str(&format!(
                "{}\n",
                format!(
                    "─── Root Cause Clusters ({}) ───────────────────────────────────────────",
                    root_causes.len()
                )
                .cyan()
                .bold()
            ));
            for (idx, rc) in root_causes.iter().enumerate() {
                let sev_badge = match rc.impact_severity {
                    Severity::Critical => "[CRITICAL]".red().bold(),
                    Severity::Error => "[ERROR]".red(),
                    Severity::Warning => "[WARN]".yellow(),
                    _ => "[INFO]".cyan(),
                };
                let rel_str = format!("[{}]", rc.relationship);
                let rel_colored = match rc.relationship {
                    stackvet_core::CauseRelationship::Verified => rel_str.green(),
                    stackvet_core::CauseRelationship::Inferred => rel_str.magenta(),
                };
                let symptoms_count = rc.symptoms.len();
                let symptoms_label = if symptoms_count == 1 {
                    "finding"
                } else {
                    "findings"
                };
                out.push_str(&format!(
                    "  {}. {} {} {}  ({} {})\n",
                    idx + 1,
                    sev_badge,
                    rc.title.bold(),
                    rel_colored,
                    symptoms_count,
                    symptoms_label
                ));
                out.push_str(&format!("     Summary:      {}\n", rc.summary));
                out.push_str(&format!(
                    "     Remediation:  {}\n",
                    rc.primary_remediation.cyan()
                ));
                if !rc.remediation_plan.is_empty() {
                    out.push_str("     Action Plan:\n");
                    for step in &rc.remediation_plan {
                        out.push_str(&format!("       {}. {}\n", step.order, step.action));
                    }
                }
                out.push('\n');
            }
        }

        // Diagnostics table
        if !diagnostics.is_empty() {
            out.push_str(&format!(
                "{}\n",
                "─── Diagnostic Details ────────────────────────────────────────────────"
                    .cyan()
                    .bold()
            ));
            let term_width = crossterm::terminal::size()
                .map(|(w, _)| w.saturating_sub(2))
                .unwrap_or(100)
                .min(130);

            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_width(term_width)
                .set_header(vec![
                    Cell::new("Rule")
                        .fg(Color::Cyan)
                        .add_attribute(Attribute::Bold),
                    Cell::new("Severity")
                        .fg(Color::Cyan)
                        .add_attribute(Attribute::Bold),
                    Cell::new("Target")
                        .fg(Color::Cyan)
                        .add_attribute(Attribute::Bold),
                    Cell::new("File Location")
                        .fg(Color::Cyan)
                        .add_attribute(Attribute::Bold),
                    Cell::new("Message")
                        .fg(Color::Cyan)
                        .add_attribute(Attribute::Bold),
                ]);

            let (rule_w, sev_w, target_w, loc_w) = if term_width < 80 {
                (7, 6, 6, 20)
            } else {
                (9, 9, 8, 35)
            };

            if let Some(col) = table.column_mut(0) {
                col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(rule_w)));
            }
            if let Some(col) = table.column_mut(1) {
                col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(sev_w)));
            }
            if let Some(col) = table.column_mut(2) {
                col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(target_w)));
            }
            if let Some(col) = table.column_mut(3) {
                col.set_constraint(ColumnConstraint::UpperBoundary(Width::Fixed(loc_w)));
            }

            for d in diagnostics {
                let sev_cell = match d.severity {
                    Severity::Critical => Cell::new("CRITICAL")
                        .fg(Color::DarkRed)
                        .add_attribute(Attribute::Bold),
                    Severity::Error => Cell::new("ERROR").fg(Color::Red),
                    Severity::Warning => Cell::new("WARN").fg(Color::Yellow),
                    Severity::Info => Cell::new("INFO").fg(Color::Blue),
                    Severity::Hint => Cell::new("HINT").fg(Color::Grey),
                };

                let loc_str = d.location.display_location();

                table.add_row(Row::from(vec![
                    Cell::new(d.rule_id.as_str()).fg(Color::Cyan),
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
        let mut summary = ReportSummary::from_diagnostics(diagnostics);
        if let Some(ref vc) = self.vulnerability_check {
            summary.vulnerability_check = Some(vc.clone());
        }
        let report = FullReport {
            schema_version: SCHEMA_VERSION.to_string(),
            tool: ToolInfo::default(),
            summary,
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

        let effective_scan_root = scan_root.or(self.scan_root.as_deref());

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
                if let Some(sarif_uri) =
                    relativize_sarif_uri(&d.location.file_path, effective_scan_root)
                {
                    let mut region_obj = serde_json::Map::new();
                    if let Some(span) = &d.location.span {
                        region_obj
                            .insert("startLine".to_string(), serde_json::json!(span.start_line));
                        region_obj
                            .insert("startColumn".to_string(), serde_json::json!(span.start_col));
                        region_obj.insert("endLine".to_string(), serde_json::json!(span.end_line));
                        region_obj.insert("endColumn".to_string(), serde_json::json!(span.end_col));
                        if let Some(snippet) = &d.location.snippet {
                            region_obj.insert(
                                "snippet".to_string(),
                                serde_json::json!({ "text": snippet }),
                            );
                        }
                    }

                    let mut artifact_loc = serde_json::json!({
                        "uri": sarif_uri.uri
                    });
                    if sarif_uri.is_relative {
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
                            let sarif_uri =
                                relativize_sarif_uri(&rel.file_path, effective_scan_root)?;
                            let mut artifact_loc = serde_json::json!({
                                "uri": sarif_uri.uri
                            });
                            if sarif_uri.is_relative {
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
                        let sarif_uri =
                            relativize_sarif_uri(&fix.file_path, effective_scan_root);
                        let artifact_loc = if let Some(su) = sarif_uri {
                            let mut loc = serde_json::json!({ "uri": su.uri });
                            if su.is_relative {
                                loc["uriBaseId"] = serde_json::json!("%SRCROOT%");
                            }
                            loc
                        } else {
                            serde_json::json!({
                                "uri": fix.file_path.to_string_lossy().replace(r"\\?\", "").replace('\\', "/")
                            })
                        };
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

        let execution_successful = !diagnostics
            .iter()
            .any(|d| d.rule_id.0 == "SEC-001" || d.rule_id.0.starts_with("DISC-"));
        let mut notifications = Vec::new();
        for d in diagnostics
            .iter()
            .filter(|d| d.rule_id.0 == "SEC-001" || d.rule_id.0.starts_with("DISC-"))
        {
            notifications.push(serde_json::json!({
                "descriptor": { "id": d.rule_id.0 },
                "level": match d.severity {
                    Severity::Critical | Severity::Error => "error",
                    _ => "warning",
                },
                "message": { "text": d.message }
            }));
        }
        let mut invocation_obj = serde_json::json!({
            "executionSuccessful": execution_successful,
        });
        if !notifications.is_empty() {
            invocation_obj["toolExecutionNotifications"] = serde_json::Value::Array(notifications);
        }

        let mut run_obj = serde_json::json!({
            "tool": {
                "driver": driver_obj
            },
            "invocations": [invocation_obj],
            "columnKind": "unicodeCodePoints",
            "results": results
        });

        let effective_root = scan_root
            .map(|r| r.to_path_buf())
            .or_else(|| self.scan_root.clone())
            .or_else(|| std::env::current_dir().ok());

        if let Some(ref root) = effective_root {
            let clean_root = root
                .to_string_lossy()
                .replace(r"\\?\", "")
                .replace('\\', "/");
            let root_uri_path = if clean_root.ends_with('/') {
                clean_root
            } else {
                format!("{clean_root}/")
            };
            let root_file_uri = if root_uri_path.starts_with('/') {
                format!("file://{}", encode_uri_path(&root_uri_path))
            } else {
                format!("file:///{}", encode_uri_path(&root_uri_path))
            };
            run_obj["originalUriBaseIds"] = serde_json::json!({
                "%SRCROOT%": {
                    "uri": root_file_uri
                }
            });
        }

        let sarif = serde_json::json!({
            "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
            "version": "2.1.0",
            "runs": [run_obj]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SarifUri {
    pub uri: String,
    pub is_relative: bool,
}

fn encode_uri_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'a'..=b'z'
            | b'A'..=b'Z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'@' => {
                out.push(byte as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{:02X}", byte);
            }
        }
    }
    out
}

pub fn encode_uri_path(path: &str) -> String {
    path.split('/')
        .map(encode_uri_segment)
        .collect::<Vec<_>>()
        .join("/")
}

pub fn relativize_sarif_uri(
    file_path: &std::path::Path,
    scan_root: Option<&std::path::Path>,
) -> Option<SarifUri> {
    let raw = file_path.to_string_lossy();
    if raw.is_empty() || raw == "<unknown>" || raw == "-" {
        return None;
    }
    let p_clean = raw.replace(r"\\?\", "").replace('\\', "/");
    let p = std::path::Path::new(&p_clean);

    let effective_root = scan_root
        .map(|r| r.to_path_buf())
        .or_else(|| std::env::current_dir().ok());

    let is_rooted = file_path.is_absolute()
        || file_path.has_root()
        || p.is_absolute()
        || p.has_root()
        || p_clean.starts_with('/')
        || (p_clean.len() >= 2 && p_clean.as_bytes()[1] == b':');

    if !is_rooted && !p_clean.starts_with("../") {
        return Some(SarifUri {
            uri: encode_uri_path(p_clean.trim_start_matches("./")),
            is_relative: true,
        });
    }

    // For absolute or ../ paths, try relativizing against effective_root
    let mut stripped_rel: Option<std::path::PathBuf> = None;
    if let Some(ref root) = effective_root {
        // 1. Try canonicalized comparison
        if let (Ok(canonical_root), Ok(canonical_p)) =
            (root.canonicalize(), file_path.canonicalize())
        {
            if let Ok(rel) = canonical_p.strip_prefix(&canonical_root) {
                stripped_rel = Some(rel.to_path_buf());
            }
        }

        // 2. Uncanonicalized fallback: compare uncanonicalized path with uncanonicalized root
        if stripped_rel.is_none() {
            let clean_root = root
                .to_string_lossy()
                .replace(r"\\?\", "")
                .replace('\\', "/");
            let norm_root = clean_root.trim_end_matches('/');

            let has_boundary = p_clean.len() == norm_root.len()
                || p_clean.as_bytes().get(norm_root.len()) == Some(&b'/');

            #[cfg(windows)]
            let matches_prefix = p_clean
                .to_lowercase()
                .starts_with(&norm_root.to_lowercase())
                && has_boundary;
            #[cfg(not(windows))]
            let matches_prefix = p_clean.starts_with(norm_root) && has_boundary;

            if matches_prefix {
                let remainder = &p_clean[norm_root.len()..];
                let rel = remainder.trim_start_matches('/');
                if !rel.is_empty() {
                    stripped_rel = Some(std::path::PathBuf::from(rel));
                }
            }
        }
    }

    if let Some(rel) = stripped_rel {
        let rel_str = rel
            .to_string_lossy()
            .replace(r"\\?\", "")
            .replace('\\', "/");
        Some(SarifUri {
            uri: encode_uri_path(&rel_str),
            is_relative: true,
        })
    } else {
        // Absolute path outside the root: preserve as file:/// URI and omit uriBaseId
        let encoded_p = encode_uri_path(&p_clean);
        let file_uri = if encoded_p.starts_with('/') {
            format!("file://{encoded_p}")
        } else {
            format!("file:///{encoded_p}")
        };
        Some(SarifUri {
            uri: file_uri,
            is_relative: false,
        })
    }
}
