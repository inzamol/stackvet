use stackvet_core::{Diagnostic, RootCause, Severity};
use std::collections::HashMap;

/// Calculates the deterministic 0-100 integer health score
pub fn calculate_health_score(diagnostics: &[Diagnostic]) -> u32 {
    let mut score: i32 = 100;
    for d in diagnostics {
        score -= d.severity.penalty_points();
    }
    score.clamp(0, 100) as u32
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

    /// Correlates raw diagnostics into grouped RootCause clusters
    pub fn correlate(&self, diagnostics: Vec<Diagnostic>) -> Vec<RootCause> {
        let mut root_causes = Vec::new();

        // 1. Cluster circular imports
        let (circular_diags, rest): (Vec<Diagnostic>, Vec<Diagnostic>) = diagnostics
            .into_iter()
            .partition(|d| d.rule_id.0 == "IMP-001");

        if !circular_diags.is_empty() {
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

        // 2. Cluster missing dependencies (DEP-002, NDEP-001) by package
        let mut missing_deps_map: HashMap<String, Vec<Diagnostic>> = HashMap::new();
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

        for (pkg, syms) in missing_deps_map {
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

        // 3. Cluster lockfile & environment drift (DEP-005, NDEP-004, ENV-001, ENV-002, ENV-003)
        let (env_diags, others): (Vec<Diagnostic>, Vec<Diagnostic>) =
            remaining_diags.into_iter().partition(|d| {
                d.rule_id.0.starts_with("ENV-")
                    || d.rule_id.0 == "DEP-005"
                    || d.rule_id.0 == "NDEP-004"
            });

        if !env_diags.is_empty() {
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

        // 4. Cluster Framework-specific issues (FastAPI, React, Next)
        let mut framework_map: HashMap<&str, Vec<Diagnostic>> = HashMap::new();
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

        for (fw_name, syms) in framework_map {
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

        // 5. Remaining individual diagnostics grouped by Rule ID
        let mut rule_map: HashMap<String, Vec<Diagnostic>> = HashMap::new();
        for d in final_remaining {
            rule_map.entry(d.rule_id.0.clone()).or_default().push(d);
        }

        for (rule_id, syms) in rule_map {
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

        root_causes.sort_by(|a, b| b.impact_severity.cmp(&a.impact_severity));
        root_causes
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
            },
        ];

        let score = calculate_health_score(&diags);
        assert_eq!(score, 70); // 100 - 20 - 8 - 2 = 70
        assert_eq!(score_grade(score), "C (Degraded)");
    }
}
