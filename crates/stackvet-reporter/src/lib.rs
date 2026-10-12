use anyhow::Result;
use colored::*;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};
use serde::{Deserialize, Serialize};
use stackvet_core::{Diagnostic, RootCause, Severity};

#[derive(Debug, Serialize, Deserialize)]
pub struct FullReport {
    pub health_score: u32,
    pub grade: String,
    pub root_causes: Vec<RootCause>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Reporter;

impl Reporter {
    pub fn new() -> Self {
        Self
    }

    /// Renders text report with scorecard, root causes, and diagnostics table
    pub fn render_text(
        &self,
        score: u32,
        grade: &str,
        root_causes: &[RootCause],
        diagnostics: &[Diagnostic],
    ) -> String {
        let mut out = String::new();

        out.push_str("\n");
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
        out.push_str(&format!("  Overall Grade: {}\n\n", grade.bold()));

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

                let raw_loc = if let Some(span) = &d.location.span {
                    format!("{}:{}", d.location.file_path.display(), span.start_line)
                } else {
                    format!("{}", d.location.file_path.display())
                };
                let loc_str = raw_loc.replace(r"\\?\", "").replace('\\', "/");

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

    /// Renders JSON format
    pub fn render_json(
        &self,
        score: u32,
        grade: &str,
        root_causes: &[RootCause],
        diagnostics: &[Diagnostic],
    ) -> Result<String> {
        let report = FullReport {
            health_score: score,
            grade: grade.to_string(),
            root_causes: root_causes.to_vec(),
            diagnostics: diagnostics.to_vec(),
        };
        Ok(serde_json::to_string_pretty(&report)?)
    }

    /// Renders OASIS SARIF v2.1.0 JSON format for GitHub Code Scanning
    pub fn render_sarif(&self, diagnostics: &[Diagnostic]) -> Result<String> {
        let results: Vec<serde_json::Value> = diagnostics
            .iter()
            .map(|d| {
                let level = match d.severity {
                    Severity::Critical | Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info | Severity::Hint => "note",
                };

                let start_line = d.location.span.as_ref().map(|s| s.start_line).unwrap_or(1);
                let raw_uri = d.location.file_path.to_string_lossy();
                let uri = raw_uri.replace(r"\\?\", "").replace('\\', "/");

                serde_json::json!({
                    "ruleId": d.rule_id.0,
                    "level": level,
                    "message": { "text": d.message },
                    "locations": [{
                        "physicalLocation": {
                            "artifactLocation": {
                                "uri": uri,
                                "uriBaseId": "%SRCROOT%"
                            },
                            "region": {
                                "startLine": start_line,
                                "startColumn": 1
                            }
                        }
                    }]
                })
            })
            .collect();

        let sarif = serde_json::json!({
            "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
            "version": "2.1.0",
            "runs": [{
                "tool": {
                    "driver": {
                        "name": "stackvet",
                        "informationUri": "https://github.com/stackvet/stackvet",
                        "version": "0.1.0"
                    }
                },
                "results": results
            }]
        });

        Ok(serde_json::to_string_pretty(&sarif)?)
    }

    /// Renders standalone responsive HTML report with dark mode
    pub fn render_html(
        &self,
        score: u32,
        grade: &str,
        root_causes: &[RootCause],
        diagnostics: &[Diagnostic],
    ) -> String {
        let mut rows = String::new();
        for d in diagnostics {
            let sev_color = match d.severity {
                Severity::Critical => "#ff4d4f",
                Severity::Error => "#ff7875",
                Severity::Warning => "#ffa940",
                _ => "#4096ff",
            };
            let raw_loc = if let Some(span) = &d.location.span {
                format!("{}:{}", d.location.file_path.display(), span.start_line)
            } else {
                format!("{}", d.location.file_path.display())
            };
            let loc_str = raw_loc.replace(r"\\?\", "").replace('\\', "/");
            rows.push_str(&format!(
                "<tr><td><code>{}</code></td><td><span style=\"color:{};font-weight:bold;\">{}</span></td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
                d.rule_id.as_str(), sev_color, d.severity.as_str(), d.language.as_str(), loc_str, d.message
            ));
        }

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
            rows = rows
        )
    }
}
