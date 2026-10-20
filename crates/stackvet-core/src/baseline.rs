use crate::{Diagnostic, Severity};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineIssueEntry {
    pub rule_id: String,
    pub file_path: String,
    pub line: usize,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineData {
    pub version: String,
    pub created_at: String,
    pub issue_fingerprints: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<BaselineIssueEntry>,
}

#[derive(Debug, Clone)]
pub struct BaselineDiff<'a> {
    pub new_issues: Vec<&'a Diagnostic>,
    pub tolerated_issues: Vec<&'a Diagnostic>,
    pub resolved_fingerprints: Vec<String>,
}

impl<'a> BaselineDiff<'a> {
    pub fn new_issues_above_threshold(&self, threshold: Severity) -> Vec<&'a Diagnostic> {
        self.new_issues
            .iter()
            .copied()
            .filter(|d| d.severity >= threshold)
            .collect()
    }
}

impl BaselineData {
    pub fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            created_at: current_iso_timestamp(),
            issue_fingerprints: BTreeSet::new(),
            issues: Vec::new(),
        }
    }

    /// Computes a platform-independent deterministic fingerprint for a diagnostic.
    /// Path separators are normalized to `/`.
    pub fn compute_fingerprint(d: &Diagnostic) -> String {
        let norm_path = d.location.file_path.to_string_lossy().replace('\\', "/");
        let line = d.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);
        format!("{}:{}:{}", d.rule_id.as_str(), norm_path, line)
    }

    pub fn from_diagnostics(diagnostics: &[Diagnostic]) -> Self {
        let mut baseline = Self::new();
        for d in diagnostics {
            baseline
                .issue_fingerprints
                .insert(Self::compute_fingerprint(d));

            let norm_path = d.location.file_path.to_string_lossy().replace('\\', "/");
            let line = d.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);
            baseline.issues.push(BaselineIssueEntry {
                rule_id: d.rule_id.0.clone(),
                file_path: norm_path,
                line,
                severity: d.severity.as_str().to_string(),
                message: d.message.clone(),
            });
        }
        baseline.issues.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then_with(|| a.line.cmp(&b.line))
        });
        baseline
    }

    /// Evaluates current diagnostics against baseline, returning new, tolerated, and resolved issues.
    pub fn diff_diagnostics<'a>(&self, diagnostics: &'a [Diagnostic]) -> BaselineDiff<'a> {
        let current_fingerprints: HashSet<String> =
            diagnostics.iter().map(Self::compute_fingerprint).collect();

        let mut new_issues = Vec::new();
        let mut tolerated_issues = Vec::new();

        for d in diagnostics {
            let fp = Self::compute_fingerprint(d);
            if self.issue_fingerprints.contains(&fp) {
                tolerated_issues.push(d);
            } else {
                new_issues.push(d);
            }
        }

        let mut resolved_fingerprints: Vec<String> = self
            .issue_fingerprints
            .iter()
            .filter(|fp| !current_fingerprints.contains(*fp))
            .cloned()
            .collect();
        resolved_fingerprints.sort();

        BaselineDiff {
            new_issues,
            tolerated_issues,
            resolved_fingerprints,
        }
    }

    /// Legacy compatibility helper: filters diagnostics to only those not in baseline.
    pub fn filter_new_issues(&self, diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
        diagnostics
            .into_iter()
            .filter(|d| {
                !self
                    .issue_fingerprints
                    .contains(&Self::compute_fingerprint(d))
            })
            .collect()
    }

    /// Updates this baseline with current diagnostics, refreshing fingerprints and timestamp.
    pub fn update_from_diagnostics(&mut self, diagnostics: &[Diagnostic]) {
        self.issue_fingerprints.clear();
        self.issues.clear();
        self.created_at = current_iso_timestamp();

        for d in diagnostics {
            self.issue_fingerprints.insert(Self::compute_fingerprint(d));
            let norm_path = d.location.file_path.to_string_lossy().replace('\\', "/");
            let line = d.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);
            self.issues.push(BaselineIssueEntry {
                rule_id: d.rule_id.0.clone(),
                file_path: norm_path,
                line,
                severity: d.severity.as_str().to_string(),
                message: d.message.clone(),
            });
        }
        self.issues.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then_with(|| a.line.cmp(&b.line))
        });
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let data: Self = serde_json::from_str(&content)?;
        Ok(data)
    }
}

impl Default for BaselineData {
    fn default() -> Self {
        Self::new()
    }
}

fn current_iso_timestamp() -> String {
    // Generate deterministic standard ISO 8601 representation
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagnosticLocation, LanguageTarget, RuleId, TextSpan};

    fn make_diag(rule: &str, file: &str, line: usize, sev: Severity) -> Diagnostic {
        Diagnostic {
            rule_id: RuleId::new(rule),
            message: format!("Test message for {rule}"),
            severity: sev,
            location: DiagnosticLocation::new(file).with_span(TextSpan::new(line, line, 1, 10)),
            explanation: "".to_string(),
            suggested_remediation: "".to_string(),
            fix: None,
            language: LanguageTarget::Python,
            confidence: crate::Confidence::High,
            ..Default::default()
        }
    }

    #[test]
    fn test_baseline_diff_and_resolved() {
        let d1 = make_diag("DEP-001", "reqs.txt", 5, Severity::Error);
        let d2 = make_diag("DEP-002", "reqs.txt", 10, Severity::Warning);

        let baseline = BaselineData::from_diagnostics(&[d1.clone(), d2.clone()]);
        assert_eq!(baseline.issue_fingerprints.len(), 2);

        // Current run: d1 still present, d2 resolved, d3 introduced
        let d3 = make_diag("FAP-001", "app.py", 15, Severity::Critical);
        let current = vec![d1.clone(), d3.clone()];

        let diff = baseline.diff_diagnostics(&current);
        assert_eq!(diff.tolerated_issues.len(), 1);
        assert_eq!(diff.tolerated_issues[0].rule_id.as_str(), "DEP-001");

        assert_eq!(diff.new_issues.len(), 1);
        assert_eq!(diff.new_issues[0].rule_id.as_str(), "FAP-001");

        assert_eq!(diff.resolved_fingerprints.len(), 1);
        assert!(diff.resolved_fingerprints[0].starts_with("DEP-002:reqs.txt:10"));

        let violating = diff.new_issues_above_threshold(Severity::Error);
        assert_eq!(violating.len(), 1);

        let non_violating = diff.new_issues_above_threshold(Severity::Critical);
        // FAP-001 is Critical, so it meets Critical threshold
        assert_eq!(non_violating.len(), 1);
    }

    #[test]
    fn test_baseline_normalization() {
        let d_win = make_diag("DEP-001", "src\\routes\\users.py", 20, Severity::Warning);
        let fp = BaselineData::compute_fingerprint(&d_win);
        assert_eq!(fp, "DEP-001:src/routes/users.py:20");
    }
}
