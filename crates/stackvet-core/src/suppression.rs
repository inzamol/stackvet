use crate::{Diagnostic, ALL_RULES};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Configured diagnostic suppression entry from `stackvet.toml` or `pyproject.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SuppressionEntry {
    /// Target rule ID (e.g. "DEP-001")
    pub rule: Option<String>,
    /// Multiple target rule IDs (e.g. ["DEP-001", "CEL-001"])
    #[serde(default)]
    pub rules: Vec<String>,
    /// File path or glob pattern (e.g. "tests/**", "legacy/*", "backend/app.py")
    pub path: Option<String>,
    /// Specific 1-indexed line number (optional)
    pub line: Option<usize>,
    /// Reason or justification for the suppression
    pub reason: Option<String>,
    /// Expiration date in ISO format YYYY-MM-DD (e.g. "2026-12-31")
    pub until: Option<String>,
}

impl SuppressionEntry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all rule IDs specified by this suppression entry.
    pub fn all_rules(&self) -> Vec<&str> {
        let mut list = Vec::new();
        if let Some(r) = &self.rule {
            list.push(r.as_str());
        }
        for r in &self.rules {
            if !list.contains(&r.as_str()) {
                list.push(r.as_str());
            }
        }
        list
    }

    /// Checks if this suppression entry has expired.
    pub fn is_expired(&self, current_date_opt: Option<&str>) -> bool {
        if let Some(until) = &self.until {
            let today = current_date_opt
                .map(|s| s.to_string())
                .unwrap_or_else(current_utc_date_str);
            today.as_str() > until.as_str()
        } else {
            false
        }
    }

    /// Validates this suppression entry against catalog rules and checks date format.
    pub fn validate(&self) -> Result<(), String> {
        let rules = self.all_rules();
        if rules.is_empty() && self.path.is_none() {
            return Err(
                "Suppression entry must specify at least 'rule', 'rules', or 'path'".to_string(),
            );
        }

        for rule_id in rules {
            let valid = rule_id.eq_ignore_ascii_case("all")
                || ALL_RULES.iter().any(|r| r.id.eq_ignore_ascii_case(rule_id));
            if !valid {
                return Err(format!(
                    "Unknown rule ID '{rule_id}' in configuration [[suppressions]]"
                ));
            }
        }

        if let Some(until) = &self.until {
            if !is_valid_iso_date(until) {
                return Err(format!(
                    "Invalid date format '{until}' in suppression 'until'. Expected YYYY-MM-DD"
                ));
            }
        }

        Ok(())
    }

    /// Matches this suppression entry against a specific diagnostic.
    pub fn matches(&self, d: &Diagnostic, root: &Path, current_date: Option<&str>) -> bool {
        if self.is_expired(current_date) {
            return false;
        }

        // Rule match check
        let rules = self.all_rules();
        if !rules.is_empty() {
            let rule_matches = rules.iter().any(|r| {
                r.eq_ignore_ascii_case("all") || r.eq_ignore_ascii_case(d.rule_id.as_str())
            });
            if !rule_matches {
                return false;
            }
        }

        // Line match check
        if let Some(target_line) = self.line {
            let matches_line = d.location.span.as_ref().is_some_and(|s| {
                target_line >= s.start_line && target_line <= s.end_line
            });
            if !matches_line {
                return false;
            }
        }

        // Path pattern match check
        if let Some(pattern) = &self.path {
            if !path_matches_pattern(&d.location.file_path, pattern, root) {
                return false;
            }
        }

        true
    }
}

/// Tests whether `file_path` matches the gitignore-style glob `pattern`.
pub fn path_matches_pattern(file_path: &Path, pattern: &str, root: &Path) -> bool {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    let _ = builder.add_line(None, pattern);
    let matcher = builder
        .build()
        .unwrap_or_else(|_| ignore::gitignore::Gitignore::empty());

    let abs_path = if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        root.join(file_path)
    };

    // Try absolute path
    if matcher.matched(&abs_path, false).is_ignore() {
        return true;
    }

    // Try relative path normalized to forward slashes
    let rel_str = file_path.to_string_lossy().replace('\\', "/");
    let rel_path = Path::new(&rel_str);
    if matcher.matched(rel_path, false).is_ignore() {
        return true;
    }

    // Direct string fallback for simple patterns (e.g. tests/**, *.py)
    let clean_pat = pattern.replace('\\', "/");
    if clean_pat == "*" || clean_pat == "**" {
        return true;
    }
    if let Some(ext) = clean_pat.strip_prefix("*.") {
        if rel_str.ends_with(&format!(".{ext}")) {
            return true;
        }
    }
    if clean_pat.ends_with("/**") {
        let prefix = &clean_pat[..clean_pat.len() - 3];
        if rel_str.starts_with(&format!("{prefix}/")) || rel_str.contains(&format!("/{prefix}/")) {
            return true;
        }
    }

    rel_str == clean_pat
}

/// An inline code comment suppression found within a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSuppression {
    pub rule_id: String,
    pub line: usize,
    pub is_file_level: bool,
    pub reason: Option<String>,
}

/// Parses inline comment suppressions from a source file's text content.
///
/// Supported formats:
/// - Python: `# stackvet:disable=DEP-001`, `# stackvet:ignore[DEP-001]: reason`
/// - JS/TS: `// stackvet:disable=NDEP-001`, `/* stackvet:disable=NDEP-001 */`
/// - Line-specific: `# stackvet:disable-line=RULE`, `# stackvet:disable-next-line=RULE`
/// - File-level: `# stackvet:disable=RULE` placed in the top 15 lines of a file
pub fn parse_inline_suppressions(source: &str) -> Vec<InlineSuppression> {
    let mut suppressions = Vec::new();

    for (idx, line) in source.lines().enumerate() {
        let line_num = idx + 1;
        let trimmed = line.trim();

        // Extract comment segment if present
        let comment_text = if let Some(pos) = trimmed.find("//") {
            &trimmed[pos + 2..]
        } else if let Some(pos) = trimmed.find('#') {
            &trimmed[pos + 1..]
        } else if let Some(pos) = trimmed.find("/*") {
            let after = &trimmed[pos + 2..];
            if let Some(end) = after.find("*/") {
                &after[..end]
            } else {
                after
            }
        } else {
            continue;
        };

        let comment = comment_text.trim();

        // Check for stackvet prefixes
        // 1. disable-next-line: stackvet:disable-next-line=RULE1,RULE2
        // 2. ignore-next-line: stackvet:ignore-next-line[RULE]: reason
        // 3. disable-line: stackvet:disable-line=RULE
        // 4. ignore: stackvet:ignore[RULE]: reason
        // 5. disable: stackvet:disable=RULE
        // 6. noqa: noqa: stackvet[RULE]
        let full_line = is_full_line_comment(trimmed);
        if let Some(rest) = comment.strip_prefix("stackvet:") {
            parse_stackvet_directive(
                rest.trim(),
                line_num,
                line_num <= 15 && full_line,
                full_line,
                &mut suppressions,
            );
        } else if let Some(pos) = comment.find("stackvet:") {
            let rest = &comment[pos + 9..];
            parse_stackvet_directive(rest.trim(), line_num, false, false, &mut suppressions);
        } else if let Some(pos) = comment.find("noqa: stackvet[") {
            let rest = &comment[pos + 15..];
            if let Some(end) = rest.find(']') {
                let rule_str = &rest[..end];
                for r in rule_str.split(',') {
                    let r_clean = r.trim();
                    if !r_clean.is_empty() {
                        suppressions.push(InlineSuppression {
                            rule_id: r_clean.to_string(),
                            line: line_num,
                            is_file_level: false,
                            reason: None,
                        });
                    }
                }
            }
        }
    }

    suppressions
}

fn is_full_line_comment(line: &str) -> bool {
    line.starts_with('#') || line.starts_with("//") || line.starts_with("/*")
}

fn parse_stackvet_directive(
    directive: &str,
    line_num: usize,
    can_be_file_level: bool,
    is_full_line: bool,
    out: &mut Vec<InlineSuppression>,
) {
    if let Some(rest) = directive.strip_prefix("disable-next-line=") {
        let (rules, reason) = split_rules_and_reason(rest);
        for r in rules {
            out.push(InlineSuppression {
                rule_id: r,
                line: line_num + 1,
                is_file_level: false,
                reason: reason.clone(),
            });
        }
    } else if let Some(rest) = directive.strip_prefix("ignore-file[") {
        if let Some(end) = rest.find(']') {
            let rule_str = &rest[..end];
            let after = rest[end + 1..].trim();
            let reason = after.strip_prefix(':').map(|s| s.trim().to_string());
            for r in rule_str.split(',') {
                let r_clean = r.trim();
                if !r_clean.is_empty() {
                    out.push(InlineSuppression {
                        rule_id: r_clean.to_string(),
                        line: line_num,
                        is_file_level: true,
                        reason: reason.clone(),
                    });
                }
            }
        }
    } else if let Some(rest) = directive.strip_prefix("disable-file=") {
        let (rules, reason) = split_rules_and_reason(rest);
        for r in rules {
            out.push(InlineSuppression {
                rule_id: r,
                line: line_num,
                is_file_level: true,
                reason: reason.clone(),
            });
        }
    } else if let Some(rest) = directive.strip_prefix("ignore-next-line[") {
        if let Some(end) = rest.find(']') {
            let rule_str = &rest[..end];
            let after = rest[end + 1..].trim();
            let reason = after.strip_prefix(':').map(|s| s.trim().to_string());
            for r in rule_str.split(',') {
                let r_clean = r.trim();
                if !r_clean.is_empty() {
                    out.push(InlineSuppression {
                        rule_id: r_clean.to_string(),
                        line: line_num + 1,
                        is_file_level: false,
                        reason: reason.clone(),
                    });
                }
            }
        }
    } else if let Some(rest) = directive.strip_prefix("disable-line=") {
        let (rules, reason) = split_rules_and_reason(rest);
        for r in rules {
            out.push(InlineSuppression {
                rule_id: r,
                line: line_num,
                is_file_level: false,
                reason: reason.clone(),
            });
        }
    } else if let Some(rest) = directive.strip_prefix("ignore[") {
        if let Some(end) = rest.find(']') {
            let rule_str = &rest[..end];
            let after = rest[end + 1..].trim();
            let reason = after.strip_prefix(':').map(|s| s.trim().to_string());
            let target_line = if is_full_line { line_num + 1 } else { line_num };
            for r in rule_str.split(',') {
                let r_clean = r.trim();
                if !r_clean.is_empty() {
                    out.push(InlineSuppression {
                        rule_id: r_clean.to_string(),
                        line: target_line,
                        is_file_level: false,
                        reason: reason.clone(),
                    });
                }
            }
        }
    } else if let Some(rest) = directive.strip_prefix("disable=") {
        let (rules, reason) = split_rules_and_reason(rest);
        for r in rules {
            out.push(InlineSuppression {
                rule_id: r,
                line: line_num,
                is_file_level: can_be_file_level,
                reason: reason.clone(),
            });
        }
    }
}

fn split_rules_and_reason(val: &str) -> (Vec<String>, Option<String>) {
    let mut parts = val.splitn(2, ':');
    let rules_part = parts.next().unwrap_or("").trim();
    let reason_part = parts.next().map(|s| s.trim().to_string());

    let rules: Vec<String> = rules_part
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    (rules, reason_part)
}

/// The origin of a diagnostic suppression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuppressionSource {
    Config(String),
    Inline { line: usize },
}

/// A diagnostic that was suppressed, including the reason and suppression origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressedDiagnostic {
    pub diagnostic: Diagnostic,
    pub reason: Option<String>,
    pub source: SuppressionSource,
}

/// Comprehensive outcome of evaluating suppressions against a diagnostic set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressionOutcome {
    pub active_diagnostics: Vec<Diagnostic>,
    pub suppressed: Vec<SuppressedDiagnostic>,
    pub stale_configured: Vec<SuppressionEntry>,
    pub expired_configured: Vec<SuppressionEntry>,
}

/// Applies configured suppressions and source inline comment suppressions to diagnostics.
pub fn apply_suppressions(
    root: &Path,
    suppressions: &[SuppressionEntry],
    diagnostics: Vec<Diagnostic>,
    current_date: Option<&str>,
) -> SuppressionOutcome {
    let mut active = Vec::new();
    let mut suppressed = Vec::new();
    let mut matched_configured_indices = HashSet::new();

    // Cache inline suppressions per file to prevent redundant I/O
    let mut inline_cache: std::collections::HashMap<std::path::PathBuf, Vec<InlineSuppression>> =
        std::collections::HashMap::new();

    for d in diagnostics {
        let mut was_suppressed = false;

        // 1. Check configured suppressions
        for (idx, entry) in suppressions.iter().enumerate() {
            if entry.matches(&d, root, current_date) {
                matched_configured_indices.insert(idx);
                let rule_desc = entry.rule.clone().unwrap_or_else(|| entry.rules.join(","));
                let desc = format!(
                    "config: rule='{}', path='{}'",
                    if rule_desc.is_empty() {
                        "all"
                    } else {
                        &rule_desc
                    },
                    entry.path.as_deref().unwrap_or("*")
                );
                suppressed.push(SuppressedDiagnostic {
                    diagnostic: d.clone(),
                    reason: entry.reason.clone(),
                    source: SuppressionSource::Config(desc),
                });
                was_suppressed = true;
                break;
            }
        }

        if was_suppressed {
            continue;
        }

        // 2. Check inline suppressions
        let target_path = if d.location.file_path.is_absolute() {
            d.location.file_path.clone()
        } else {
            root.join(&d.location.file_path)
        };

        let inline_list = inline_cache.entry(target_path.clone()).or_insert_with(|| {
            if target_path.is_file() {
                if let Ok(content) = fs::read_to_string(&target_path) {
                    parse_inline_suppressions(&content)
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        });

        let d_line = d.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);

        for inline in inline_list {
            let rule_matches = inline.rule_id.eq_ignore_ascii_case("all")
                || inline.rule_id.eq_ignore_ascii_case(d.rule_id.as_str());

            if !rule_matches {
                continue;
            }

            let matches_location = inline.is_file_level
                || inline.line == d_line
                || d.location.span.as_ref().is_some_and(|s| {
                    inline.line >= s.start_line && inline.line <= s.end_line
                });
            if matches_location {
                suppressed.push(SuppressedDiagnostic {
                    diagnostic: d.clone(),
                    reason: inline.reason.clone(),
                    source: SuppressionSource::Inline { line: inline.line },
                });
                was_suppressed = true;
                break;
            }
        }

        if !was_suppressed {
            active.push(d);
        }
    }

    // Determine stale and expired configured suppressions
    let mut stale_configured = Vec::new();
    let mut expired_configured = Vec::new();

    for (idx, entry) in suppressions.iter().enumerate() {
        if entry.is_expired(current_date) {
            expired_configured.push(entry.clone());
        } else if !matched_configured_indices.contains(&idx) {
            stale_configured.push(entry.clone());
        }
    }

    SuppressionOutcome {
        active_diagnostics: active,
        suppressed,
        stale_configured,
        expired_configured,
    }
}

/// Checks if string is a valid ISO 8601 date format YYYY-MM-DD.
pub fn is_valid_iso_date(s: &str) -> bool {
    if s.len() != 10 {
        return false;
    }
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    let (y, m, d) = match (
        parts[0].parse::<u32>(),
        parts[1].parse::<u32>(),
        parts[2].parse::<u32>(),
    ) {
        (Ok(y), Ok(m), Ok(d)) => (y, m, d),
        _ => return false,
    };

    if !(1970..=2200).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return false;
    }
    true
}

/// Returns current UTC date string in YYYY-MM-DD format.
pub fn current_utc_date_str() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    epoch_days_to_date((now / 86400) as i64)
}

fn epoch_days_to_date(days: i64) -> String {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagnosticLocation, LanguageTarget, RuleId, Severity, TextSpan};

    fn make_diag(rule: &str, file: &str, line: usize) -> Diagnostic {
        Diagnostic {
            rule_id: RuleId::new(rule),
            message: format!("Test message for {rule}"),
            severity: Severity::Error,
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
    fn test_date_utils() {
        assert!(is_valid_iso_date("2026-10-20"));
        assert!(!is_valid_iso_date("2026/10/20"));
        assert!(!is_valid_iso_date("invalid-date"));
        assert!(!is_valid_iso_date("2026-13-01"));

        let today = current_utc_date_str();
        assert!(is_valid_iso_date(&today));

        assert_eq!(epoch_days_to_date(0), "1970-01-01");
        assert_eq!(epoch_days_to_date(20877), "2027-02-28");
        assert_eq!(epoch_days_to_date(20878), "2027-03-01");
        assert_eq!(epoch_days_to_date(11016), "2000-02-29");
    }

    #[test]
    fn test_suppression_entry_deny_unknown_fields() {
        let toml_str = r#"
rule = "DEP-001"
unknown_key = "value"
"#;
        let res: Result<SuppressionEntry, _> = toml::from_str(toml_str);
        assert!(res.is_err());
    }

    #[test]
    fn test_configured_suppression_matching_and_expiration() {
        let root = Path::new(".");
        let mut entry = SuppressionEntry {
            rule: Some("DEP-001".to_string()),
            path: Some("tests/**".to_string()),
            reason: Some("Test fixtures permitted".to_string()),
            until: Some("2026-12-31".to_string()),
            ..Default::default()
        };

        let d_match = make_diag("DEP-001", "tests/fixtures/app.py", 10);
        let d_wrong_rule = make_diag("FAP-001", "tests/fixtures/app.py", 10);
        let d_wrong_path = make_diag("DEP-001", "src/main.py", 10);

        // Active before expiration date
        assert!(entry.matches(&d_match, root, Some("2026-10-20")));
        assert!(!entry.matches(&d_wrong_rule, root, Some("2026-10-20")));
        assert!(!entry.matches(&d_wrong_path, root, Some("2026-10-20")));

        // Inactive after expiration date
        assert!(!entry.matches(&d_match, root, Some("2027-01-01")));

        // Test outcome
        let outcome = apply_suppressions(
            root,
            &[entry.clone()],
            vec![d_match.clone(), d_wrong_path.clone()],
            Some("2026-10-20"),
        );
        assert_eq!(outcome.active_diagnostics.len(), 1);
        assert_eq!(
            outcome.active_diagnostics[0].location.file_path,
            Path::new("src/main.py")
        );
        assert_eq!(outcome.suppressed.len(), 1);
        assert_eq!(
            outcome.suppressed[0].reason,
            Some("Test fixtures permitted".to_string())
        );
        assert!(outcome.stale_configured.is_empty());

        // Expired entry detection
        entry.until = Some("2025-01-01".to_string());
        let outcome_expired = apply_suppressions(root, &[entry], vec![d_match], Some("2026-10-20"));
        assert_eq!(outcome_expired.active_diagnostics.len(), 1);
        assert_eq!(outcome_expired.expired_configured.len(), 1);
    }

    #[test]
    fn test_inline_comment_suppression_parsing() {
        let py_code = r#"
# stackvet:disable=DEP-001: File-level dependency suppression
import os
import requests

def foo():
    # stackvet:disable-next-line=FAP-001: Intentional blocking call
    time.sleep(1)
    x = 42 # stackvet:disable=CEL-001
    # stackvet:ignore[SQL-001]: Legacy unindexed query
    run_query()
"#;

        let inlines = parse_inline_suppressions(py_code);
        assert_eq!(inlines.len(), 4);

        let dep = inlines.iter().find(|i| i.rule_id == "DEP-001").unwrap();
        assert!(dep.is_file_level);
        assert_eq!(
            dep.reason,
            Some("File-level dependency suppression".to_string())
        );

        let fap = inlines.iter().find(|i| i.rule_id == "FAP-001").unwrap();
        assert_eq!(fap.line, 8); // disable-next-line on line 7 -> applies to line 8

        let cel = inlines.iter().find(|i| i.rule_id == "CEL-001").unwrap();
        assert_eq!(cel.line, 9); // same-line comment

        let sql = inlines.iter().find(|i| i.rule_id == "SQL-001").unwrap();
        assert_eq!(sql.line, 11);
        assert_eq!(sql.reason, Some("Legacy unindexed query".to_string()));
    }
}
