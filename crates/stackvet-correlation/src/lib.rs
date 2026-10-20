use stackvet_core::{
    Diagnostic, DiagnosticContribution, HealthScoreExplanation, RootCause, RuleScoreContribution,
    Severity,
};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DiagnosticDedupeKey {
    rule_id: String,
    file_path: PathBuf,
    line: Option<usize>,
    message: String,
}

/// Deduplicates diagnostics to prevent identical findings or duplicate occurrences
/// at the same file and line from distorting health scoring or reporting.
pub fn deduplicate_diagnostics(diagnostics: &[Diagnostic]) -> (Vec<Diagnostic>, usize) {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    let mut duplicates_count = 0;

    for d in diagnostics {
        let key = DiagnosticDedupeKey {
            rule_id: d.rule_id.0.clone(),
            file_path: d.location.file_path.clone(),
            line: d.location.span.as_ref().map(|s| s.start_line),
            message: d.message.clone(),
        };

        if seen.insert(key) {
            unique.push(d.clone());
        } else {
            duplicates_count += 1;
        }
    }

    (unique, duplicates_count)
}

/// Calculates the deterministic, explainable health score with a comprehensive attribution breakdown.
pub fn calculate_health_score_explained(diagnostics: &[Diagnostic]) -> HealthScoreExplanation {
    let (unique_diagnostics, duplicates_ignored) = deduplicate_diagnostics(diagnostics);

    let base_score: u32 = 100;
    let mut diagnostic_details: Vec<DiagnosticContribution> = Vec::new();
    // Maps rule_id -> (max_severity, max_penalty, count, total_penalty, title)
    let mut rule_map: BTreeMap<String, (Severity, u32, usize, u32, String)> = BTreeMap::new();
    let mut total_deductions: u32 = 0;

    for d in &unique_diagnostics {
        let penalty = d.severity.penalty_points() as u32;
        if penalty > 0 {
            total_deductions = total_deductions.saturating_add(penalty);

            let line = d.location.span.as_ref().map(|s| s.start_line);
            let file_str = d
                .location
                .file_path
                .to_string_lossy()
                .replace(r"\\?\", "")
                .replace('\\', "/");

            diagnostic_details.push(DiagnosticContribution {
                rule_id: d.rule_id.0.clone(),
                severity: d.severity,
                penalty,
                file_path: file_str,
                line,
                message: d.message.clone(),
            });

            let rule_title = stackvet_core::get_rule_meta(&d.rule_id.0)
                .map(|m| m.title.to_string())
                .unwrap_or_else(|| {
                    if !d.explanation.is_empty() {
                        d.explanation.clone()
                    } else {
                        d.message.clone()
                    }
                });

            let entry = rule_map
                .entry(d.rule_id.0.clone())
                .or_insert((d.severity, penalty, 0, 0, rule_title));
            if d.severity > entry.0 {
                entry.0 = d.severity;
            }
            if penalty > entry.1 {
                entry.1 = penalty;
            }
            entry.2 += 1;
            entry.3 = entry.3.saturating_add(penalty);
        }
    }

    let mut rule_contributions: Vec<RuleScoreContribution> = rule_map
        .into_iter()
        .map(
            |(rule_id, (severity, penalty_per_finding, count, total_penalty, title))| {
                RuleScoreContribution {
                    rule_id,
                    severity,
                    penalty_per_finding,
                    count,
                    total_penalty,
                    title,
                }
            },
        )
        .collect();

    // Deterministic sort for rule contributions:
    // Primary: total_penalty descending, Secondary: severity descending, Tertiary: rule_id ascending
    rule_contributions.sort_by(|a, b| {
        b.total_penalty
            .cmp(&a.total_penalty)
            .then_with(|| b.severity.cmp(&a.severity))
            .then_with(|| a.rule_id.cmp(&b.rule_id))
    });

    // Deterministic sort for diagnostic details:
    // Primary: severity descending, Secondary: rule_id ascending, Tertiary: file_path ascending, Quaternary: line ascending
    diagnostic_details.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.rule_id.cmp(&b.rule_id))
            .then_with(|| a.file_path.cmp(&b.file_path))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.message.cmp(&b.message))
    });

    let final_score = (base_score as i32 - total_deductions as i32).clamp(0, 100) as u32;
    let grade = score_grade(final_score).to_string();

    HealthScoreExplanation {
        base_score,
        final_score,
        grade,
        total_deductions,
        unique_findings_count: unique_diagnostics.len(),
        duplicate_findings_ignored: duplicates_ignored,
        suppressed_findings_count: 0,
        rule_contributions,
        diagnostic_details,
    }
}

/// Calculates the deterministic 0-100 integer health score
pub fn calculate_health_score(diagnostics: &[Diagnostic]) -> u32 {
    calculate_health_score_explained(diagnostics).final_score
}

/// Computes letter grade based on health score
pub fn score_grade(score: u32) -> &'static str {
    match score {
        90..=100 => "A (Healthy)",
        75..=89 => "B (Fair)",
        50..=74 => "C (Degraded)",
        _ => "F (Critical Issues)",
    }
}

pub struct CorrelationEngine;

impl CorrelationEngine {
    pub fn new() -> Self {
        Self
    }

    /// Correlates raw diagnostics into grouped RootCause clusters with deterministic ordering
    pub fn correlate(&self, diagnostics: Vec<Diagnostic>) -> Vec<RootCause> {
        let mut root_causes = Vec::new();

        // 1. Cluster circular imports
        let (mut circular_diags, rest): (Vec<Diagnostic>, Vec<Diagnostic>) = diagnostics
            .into_iter()
            .partition(|d| d.rule_id.0 == "IMP-001");

        if !circular_diags.is_empty() {
            Self::sort_diagnostics(&mut circular_diags);
            root_causes.push(RootCause {
                id: "RC-CIRCULAR-IMPORTS".to_string(),
                title: "Architectural Dependency Cycles Detected".to_string(),
                summary: format!(
                    "Found {} circular dependency cycle(s) affecting module initialization and coupling.",
                    circular_diags.len()
                ),
                impact_severity: Severity::Error,
                primary_remediation:
                    "Extract shared models/interfaces into a common module to break import cycles.".to_string(),
                automated_fix: None,
                symptoms: circular_diags,
            });
        }

        // 2. Cluster missing dependencies (DEP-002, NDEP-001) by package using BTreeMap for deterministic key order
        let mut missing_deps_map: BTreeMap<String, Vec<Diagnostic>> = BTreeMap::new();
        let mut remaining_diags = Vec::new();

        for d in rest {
            if d.rule_id.0 == "DEP-002" || d.rule_id.0 == "NDEP-001" {
                let pkg_name = d
                    .message
                    .split('\'')
                    .nth(1)
                    .unwrap_or("unknown")
                    .to_string();
                missing_deps_map.entry(pkg_name).or_default().push(d);
            } else {
                remaining_diags.push(d);
            }
        }

        for (pkg, mut syms) in missing_deps_map {
            Self::sort_diagnostics(&mut syms);
            let auto_fix = syms.iter().find_map(|s| s.fix.clone());
            root_causes.push(RootCause {
                id: format!("RC-MISSING-DEP-{}", pkg.to_uppercase()),
                title: format!("Missing Project Dependency: '{}'", pkg),
                summary: format!(
                    "Package '{}' is imported across {} file location(s) but undeclared in manifests.",
                    pkg,
                    syms.len()
                ),
                impact_severity: Severity::Error,
                primary_remediation: format!("Add '{}' to project manifest dependencies.", pkg),
                automated_fix: auto_fix,
                symptoms: syms,
            });
        }

        // 3. Cluster lockfile & environment drift (DEP-005, NDEP-004, NDEP-006, ENV-001, ENV-002, ENV-003)
        let (mut env_diags, others): (Vec<Diagnostic>, Vec<Diagnostic>) =
            remaining_diags.into_iter().partition(|d| {
                d.rule_id.0.starts_with("ENV-")
                    || d.rule_id.0 == "DEP-005"
                    || d.rule_id.0 == "NDEP-004"
                    || d.rule_id.0 == "NDEP-006"
            });

        if !env_diags.is_empty() {
            Self::sort_diagnostics(&mut env_diags);
            let max_sev = env_diags
                .iter()
                .map(|d| d.severity)
                .max()
                .unwrap_or(Severity::Warning);
            root_causes.push(RootCause {
                id: "RC-ENVIRONMENT-DRIFT".to_string(),
                title: "Environment, Lockfile & Runtime Drift".to_string(),
                summary: format!(
                    "Detected {} discrepancy(ies) between local manifests, Docker, CI matrix, or lockfiles.",
                    env_diags.len()
                ),
                impact_severity: max_sev,
                primary_remediation:
                    "Align runtime version declarations across pyproject, package.json, Dockerfile, and CI workflows.".to_string(),
                automated_fix: None,
                symptoms: env_diags,
            });
        }

        // 4. Cluster Framework-specific issues (FastAPI, React, Next) using BTreeMap
        let mut framework_map: BTreeMap<&str, Vec<Diagnostic>> = BTreeMap::new();
        let mut final_remaining = Vec::new();

        for d in others {
            if d.rule_id.0.starts_with("FAP-") {
                framework_map.entry("FastAPI").or_default().push(d);
            } else if d.rule_id.0.starts_with("RCT-") || d.rule_id.0.starts_with("NXT-") {
                framework_map.entry("React / Next.js").or_default().push(d);
            } else {
                final_remaining.push(d);
            }
        }

        for (fw_name, mut syms) in framework_map {
            Self::sort_diagnostics(&mut syms);
            let max_sev = syms
                .iter()
                .map(|d| d.severity)
                .max()
                .unwrap_or(Severity::Warning);
            root_causes.push(RootCause {
                id: format!("RC-FRAMEWORK-{}", fw_name.to_uppercase().replace([' ', '/'], "-")),
                title: format!("{} Anti-Patterns & Concurrency Hazards", fw_name),
                summary: format!(
                    "Identified {} framework anti-pattern(s) or runtime correctness issue(s).",
                    syms.len()
                ),
                impact_severity: max_sev,
                primary_remediation: format!("Review and update {} handlers and components according to recommended practices.", fw_name),
                automated_fix: None,
                symptoms: syms,
            });
        }

        // 5. Remaining individual diagnostics grouped by Rule ID using BTreeMap
        let mut rule_map: BTreeMap<String, Vec<Diagnostic>> = BTreeMap::new();
        for d in final_remaining {
            rule_map.entry(d.rule_id.0.clone()).or_default().push(d);
        }

        for (rule_id, mut syms) in rule_map {
            Self::sort_diagnostics(&mut syms);
            let max_sev = syms
                .iter()
                .map(|d| d.severity)
                .max()
                .unwrap_or(Severity::Info);
            let first_explanation = syms[0].explanation.clone();
            let first_remediation = syms[0].suggested_remediation.clone();
            let first_fix = syms.iter().find_map(|s| s.fix.clone());

            root_causes.push(RootCause {
                id: format!("RC-{}", rule_id),
                title: format!("Rule Violation: {}", rule_id),
                summary: first_explanation,
                impact_severity: max_sev,
                primary_remediation: first_remediation,
                automated_fix: first_fix,
                symptoms: syms,
            });
        }

        // Deterministic sort for root causes:
        // Primary: impact_severity descending, Secondary: id ascending
        root_causes.sort_by(|a, b| {
            b.impact_severity
                .cmp(&a.impact_severity)
                .then_with(|| a.id.cmp(&b.id))
        });
        root_causes
    }

    /// Sorts symptoms deterministically:
    /// Primary: severity descending, Secondary: rule_id ascending, Tertiary: file_path ascending, Quaternary: line ascending
    fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
        diagnostics.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.rule_id.0.cmp(&b.rule_id.0))
                .then_with(|| a.location.file_path.cmp(&b.location.file_path))
                .then_with(|| {
                    let a_line = a.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);
                    let b_line = b.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);
                    a_line.cmp(&b_line)
                })
                .then_with(|| a.message.cmp(&b.message))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stackvet_core::{DiagnosticLocation, LanguageTarget, RuleId};
    use std::path::PathBuf;

    #[test]
    fn test_calculate_health_score() {
        let diags = vec![
            Diagnostic {
                rule_id: RuleId::new("FAP-001"),
                message: "Blocking call".to_string(),
                severity: Severity::Critical, // -20
                location: DiagnosticLocation::new(PathBuf::from("a.py")),
                explanation: "".to_string(),
                suggested_remediation: "".to_string(),
                fix: None,
                language: LanguageTarget::Python,
                ..Default::default()
            },
            Diagnostic {
                rule_id: RuleId::new("DEP-002"),
                message: "Missing dep".to_string(),
                severity: Severity::Error, // -8
                location: DiagnosticLocation::new(PathBuf::from("b.py")),
                explanation: "".to_string(),
                suggested_remediation: "".to_string(),
                fix: None,
                language: LanguageTarget::Python,
                ..Default::default()
            },
            Diagnostic {
                rule_id: RuleId::new("DEP-003"),
                message: "Unused dep".to_string(),
                severity: Severity::Warning, // -2
                location: DiagnosticLocation::new(PathBuf::from("c.py")),
                explanation: "".to_string(),
                suggested_remediation: "".to_string(),
                fix: None,
                language: LanguageTarget::Python,
                ..Default::default()
            },
        ];

        let score = calculate_health_score(&diags);
        assert_eq!(score, 70); // 100 - 20 - 8 - 2 = 70
        assert_eq!(score_grade(score), "C (Degraded)");
    }

    #[test]
    fn test_deduplicate_diagnostics_prevents_distortion() {
        let diag = Diagnostic {
            rule_id: RuleId::new("FAP-001"),
            message: "Blocking synchronous call".to_string(),
            severity: Severity::Critical, // -20
            location: DiagnosticLocation::new(PathBuf::from("main.py")),
            explanation: "Do not block the event loop".to_string(),
            suggested_remediation: "Use async def or anyio".to_string(),
            fix: None,
            language: LanguageTarget::Python,
            ..Default::default()
        };

        // Pass 5 identical duplicate diagnostics
        let diags = vec![
            diag.clone(),
            diag.clone(),
            diag.clone(),
            diag.clone(),
            diag.clone(),
        ];

        let explanation = calculate_health_score_explained(&diags);
        assert_eq!(explanation.base_score, 100);
        assert_eq!(explanation.final_score, 80); // 100 - 20 (deduplicated, NOT 100 - 100 = 0)
        assert_eq!(explanation.grade, "B (Fair)");
        assert_eq!(explanation.total_deductions, 20);
        assert_eq!(explanation.unique_findings_count, 1);
        assert_eq!(explanation.duplicate_findings_ignored, 4);

        assert_eq!(explanation.rule_contributions.len(), 1);
        assert_eq!(explanation.rule_contributions[0].rule_id, "FAP-001");
        assert_eq!(explanation.rule_contributions[0].count, 1);
        assert_eq!(explanation.rule_contributions[0].total_penalty, 20);

        // calculate_health_score should match explanation.final_score
        assert_eq!(calculate_health_score(&diags), 80);
    }

    #[test]
    fn test_calculate_health_score_explained_details() {
        let diags = vec![
            Diagnostic {
                rule_id: RuleId::new("DEP-002"),
                message: "Package 'requests' missing".to_string(),
                severity: Severity::Error, // -8
                location: DiagnosticLocation::new(PathBuf::from("a.py")),
                explanation: "".to_string(),
                suggested_remediation: "".to_string(),
                fix: None,
                language: LanguageTarget::Python,
                ..Default::default()
            },
            Diagnostic {
                rule_id: RuleId::new("DEP-002"),
                message: "Package 'httpx' missing".to_string(),
                severity: Severity::Error, // -8
                location: DiagnosticLocation::new(PathBuf::from("b.py")),
                explanation: "".to_string(),
                suggested_remediation: "".to_string(),
                fix: None,
                language: LanguageTarget::Python,
                ..Default::default()
            },
            Diagnostic {
                rule_id: RuleId::new("DEP-003"),
                message: "Unused dep 'pytest'".to_string(),
                severity: Severity::Warning, // -2
                location: DiagnosticLocation::new(PathBuf::from("pyproject.toml")),
                explanation: "".to_string(),
                suggested_remediation: "".to_string(),
                fix: None,
                language: LanguageTarget::Python,
                ..Default::default()
            },
        ];

        let exp = calculate_health_score_explained(&diags);
        assert_eq!(exp.base_score, 100);
        assert_eq!(exp.total_deductions, 18); // 8 + 8 + 2
        assert_eq!(exp.final_score, 82);
        assert_eq!(exp.grade, "B (Fair)");
        assert_eq!(exp.unique_findings_count, 3);
        assert_eq!(exp.duplicate_findings_ignored, 0);

        // Rule contributions sorted by total_penalty descending
        assert_eq!(exp.rule_contributions.len(), 2);
        assert_eq!(exp.rule_contributions[0].rule_id, "DEP-002");
        assert_eq!(exp.rule_contributions[0].count, 2);
        assert_eq!(exp.rule_contributions[0].total_penalty, 16);
        assert_eq!(exp.rule_contributions[1].rule_id, "DEP-003");
        assert_eq!(exp.rule_contributions[1].count, 1);
        assert_eq!(exp.rule_contributions[1].total_penalty, 2);
    }

    #[test]
    fn test_score_clamping_at_zero() {
        // 6 Criticals = 6 * 20 = 120 points deduction
        let mut diags = Vec::new();
        for i in 0..6 {
            diags.push(Diagnostic {
                rule_id: RuleId::new(format!("CRIT-{:03}", i)),
                message: format!("Fatal crash {}", i),
                severity: Severity::Critical,
                location: DiagnosticLocation::new(PathBuf::from(format!("{}.py", i))),
                explanation: "".to_string(),
                suggested_remediation: "".to_string(),
                fix: None,
                language: LanguageTarget::Python,
                ..Default::default()
            });
        }

        let exp = calculate_health_score_explained(&diags);
        assert_eq!(exp.total_deductions, 120);
        assert_eq!(exp.final_score, 0); // clamped to 0
        assert_eq!(exp.grade, "F (Critical Issues)");
    }

    #[test]
    fn test_letter_grade_thresholds() {
        assert_eq!(score_grade(100), "A (Healthy)");
        assert_eq!(score_grade(90), "A (Healthy)");
        assert_eq!(score_grade(89), "B (Fair)");
        assert_eq!(score_grade(75), "B (Fair)");
        assert_eq!(score_grade(74), "C (Degraded)");
        assert_eq!(score_grade(50), "C (Degraded)");
        assert_eq!(score_grade(49), "F (Critical Issues)");
        assert_eq!(score_grade(0), "F (Critical Issues)");
    }

    #[test]
    fn test_correlation_engine_deterministic_order() {
        let engine = CorrelationEngine::new();
        let diag1 = Diagnostic {
            rule_id: RuleId::new("DEP-002"),
            message: "Package 'b_pkg' missing".to_string(),
            severity: Severity::Error,
            location: DiagnosticLocation::new(PathBuf::from("file_b.py")),
            explanation: "".to_string(),
            suggested_remediation: "".to_string(),
            fix: None,
            language: LanguageTarget::Python,
            ..Default::default()
        };
        let diag2 = Diagnostic {
            rule_id: RuleId::new("DEP-002"),
            message: "Package 'a_pkg' missing".to_string(),
            severity: Severity::Error,
            location: DiagnosticLocation::new(PathBuf::from("file_a.py")),
            explanation: "".to_string(),
            suggested_remediation: "".to_string(),
            fix: None,
            language: LanguageTarget::Python,
            ..Default::default()
        };

        // Correlate in order [diag1, diag2]
        let rc_first = engine.correlate(vec![diag1.clone(), diag2.clone()]);
        // Correlate in reverse order [diag2, diag1]
        let rc_second = engine.correlate(vec![diag2, diag1]);

        assert_eq!(rc_first.len(), 2);
        assert_eq!(rc_second.len(), 2);
        // Both should yield the exact same deterministic root cause IDs in the same order
        assert_eq!(rc_first[0].id, rc_second[0].id);
        assert_eq!(rc_first[1].id, rc_second[1].id);
        assert_eq!(rc_first[0].id, "RC-MISSING-DEP-A_PKG");
        assert_eq!(rc_first[1].id, "RC-MISSING-DEP-B_PKG");
    }

    #[test]
    fn test_rule_contribution_accumulated_sum_and_max_severity() {
        let diags = vec![
            Diagnostic {
                rule_id: RuleId::new("FAP-001"),
                message: "Finding with Warning severity".to_string(),
                severity: Severity::Warning, // 2 pts
                location: DiagnosticLocation::new(PathBuf::from("a.py")),
                explanation: "".to_string(),
                suggested_remediation: "".to_string(),
                fix: None,
                language: LanguageTarget::Python,
                ..Default::default()
            },
            Diagnostic {
                rule_id: RuleId::new("FAP-001"),
                message: "Finding with Critical severity".to_string(),
                severity: Severity::Critical, // 20 pts
                location: DiagnosticLocation::new(PathBuf::from("b.py")),
                explanation: "".to_string(),
                suggested_remediation: "".to_string(),
                fix: None,
                language: LanguageTarget::Python,
                ..Default::default()
            },
        ];

        let exp = calculate_health_score_explained(&diags);
        assert_eq!(exp.total_deductions, 22);
        assert_eq!(exp.final_score, 78);
        assert_eq!(exp.rule_contributions.len(), 1);

        let contrib = &exp.rule_contributions[0];
        assert_eq!(contrib.rule_id, "FAP-001");
        assert_eq!(contrib.severity, Severity::Critical); // max severity
        assert_eq!(contrib.penalty_per_finding, 20); // max penalty
        assert_eq!(contrib.count, 2);
        assert_eq!(contrib.total_penalty, 22); // accumulated sum: 2 + 20 = 22
    }
}
