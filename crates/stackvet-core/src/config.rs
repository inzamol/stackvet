use crate::rules_catalog::ALL_RULES;
use crate::{Diagnostic, Severity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Configuration file not found: {0}")]
    NotFound(String),
    #[error("Failed to read configuration: {0}")]
    IoError(String),
    #[error("Configuration syntax error: {0}")]
    ParseError(String),
    #[error("Configuration validation error: {0}")]
    ValidationError(String),
}

use crate::suppression::SuppressionEntry;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StackvetConfig {
    #[serde(default)]
    pub project: ProjectConfig,
    #[serde(default)]
    pub thresholds: ThresholdsConfig,
    #[serde(default)]
    pub ci: CiConfig,
    #[serde(default)]
    pub rules: HashMap<String, RuleSetting>,
    #[serde(default)]
    pub ignore: IgnoreConfig,
    #[serde(default)]
    pub suppressions: Vec<SuppressionEntry>,
    #[serde(default)]
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheConfig {
    #[serde(default)]
    pub enabled: bool,
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum IgnoreConfig {
    List(Vec<String>),
    Table {
        #[serde(default)]
        patterns: Vec<String>,
        #[serde(default = "default_true")]
        use_defaults: bool,
    },
}

impl Default for IgnoreConfig {
    fn default() -> Self {
        Self::Table {
            patterns: Vec::new(),
            use_defaults: true,
        }
    }
}

impl IgnoreConfig {
    pub fn patterns(&self) -> &[String] {
        match self {
            Self::List(p) => p.as_slice(),
            Self::Table { patterns, .. } => patterns.as_slice(),
        }
    }

    pub fn use_defaults(&self) -> bool {
        match self {
            Self::List(_) => true,
            Self::Table { use_defaults, .. } => *use_defaults,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectConfig {
    pub name: Option<String>,
    pub min_score: Option<u32>,
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThresholdsConfig {
    pub fail_on: Option<String>,
    pub min_score: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CiConfig {
    pub baseline: Option<PathBuf>,
    pub fail_on: Option<String>,
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum RuleSetting {
    Simple(String),
    Detailed {
        #[serde(default)]
        severity: Option<String>,
        #[serde(default = "default_true")]
        enabled: bool,
    },
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleOverride {
    Disabled,
    Severity(Severity),
    KeepNative,
}

impl RuleSetting {
    pub fn to_override(&self, rule_id: &str) -> Result<RuleOverride, ConfigError> {
        match self {
            RuleSetting::Simple(s) => parse_rule_action(s, rule_id),
            RuleSetting::Detailed { severity, enabled } => {
                if !*enabled {
                    Ok(RuleOverride::Disabled)
                } else if let Some(s) = severity {
                    parse_rule_action(s, rule_id)
                } else {
                    // Enabled without explicit severity preserves the rule's native severity
                    Ok(RuleOverride::KeepNative)
                }
            }
        }
    }
}

fn parse_rule_action(val: &str, rule_id: &str) -> Result<RuleOverride, ConfigError> {
    match val.trim().to_lowercase().as_str() {
        "disabled" | "off" | "none" | "ignore" => Ok(RuleOverride::Disabled),
        "hint" => Ok(RuleOverride::Severity(Severity::Hint)),
        "info" => Ok(RuleOverride::Severity(Severity::Info)),
        "warn" | "warning" => Ok(RuleOverride::Severity(Severity::Warning)),
        "error" => Ok(RuleOverride::Severity(Severity::Error)),
        "critical" | "fatal" => Ok(RuleOverride::Severity(Severity::Critical)),
        other => Err(ConfigError::ValidationError(format!(
            "Invalid severity '{other}' for rule '{rule_id}'. Expected one of: hint, info, warning, error, critical, disabled, off"
        ))),
    }
}

pub fn parse_severity(val: &str) -> Result<Severity, ConfigError> {
    match val.trim().to_lowercase().as_str() {
        "hint" => Ok(Severity::Hint),
        "info" => Ok(Severity::Info),
        "warn" | "warning" => Ok(Severity::Warning),
        "error" => Ok(Severity::Error),
        "critical" | "fatal" => Ok(Severity::Critical),
        other => Err(ConfigError::ValidationError(format!(
            "Invalid severity '{other}'. Expected one of: hint, info, warning, error, critical"
        ))),
    }
}

impl StackvetConfig {
    /// Validates configuration rules against the catalog and validates severity values.
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (rule_id, setting) in &self.rules {
            let exists = ALL_RULES.iter().any(|r| r.id == rule_id.as_str());
            if !exists {
                return Err(ConfigError::ValidationError(format!(
                    "Unknown rule ID '{rule_id}' in configuration [rules]"
                )));
            }
            setting.to_override(rule_id)?;
        }

        if let Some(fail_on) = &self.thresholds.fail_on {
            parse_severity(fail_on)?;
        }
        if let Some(fail_on) = &self.ci.fail_on {
            parse_severity(fail_on)?;
        }

        for supp in &self.suppressions {
            supp.validate().map_err(ConfigError::ValidationError)?;
        }

        Ok(())
    }

    /// Parses configuration from a TOML string and validates it.
    pub fn from_toml_str(content: &str) -> Result<Self, ConfigError> {
        let config: StackvetConfig =
            toml::from_str(content).map_err(|e| ConfigError::ParseError(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Loads configuration from explicit file path.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        if !path.is_file() {
            return Err(ConfigError::NotFound(path.display().to_string()));
        }
        let content = fs::read_to_string(path)
            .map_err(|e| ConfigError::IoError(format!("{}: {}", path.display(), e)))?;

        let config: StackvetConfig = toml::from_str(&content)
            .map_err(|e| ConfigError::ParseError(format!("{}: {}", path.display(), e)))?;

        config.validate()?;
        Ok(config)
    }

    /// Automatically discovers configuration in a directory:
    /// 1. `stackvet.toml`
    /// 2. `.stackvet.toml`
    /// 3. `pyproject.toml` under `[tool.stackvet]`
    pub fn discover_in_dir(root: &Path) -> Result<Option<Self>, ConfigError> {
        let stackvet_toml = root.join("stackvet.toml");
        if stackvet_toml.is_file() {
            return Self::from_file(&stackvet_toml).map(Some);
        }

        let dot_stackvet_toml = root.join(".stackvet.toml");
        if dot_stackvet_toml.is_file() {
            return Self::from_file(&dot_stackvet_toml).map(Some);
        }

        let pyproject_path = root.join("pyproject.toml");
        if pyproject_path.is_file() {
            let content = fs::read_to_string(&pyproject_path).map_err(|e| {
                ConfigError::IoError(format!("{}: {}", pyproject_path.display(), e))
            })?;
            let parsed: toml::Value = toml::from_str(&content).map_err(|e| {
                ConfigError::ParseError(format!("{}: {}", pyproject_path.display(), e))
            })?;

            if let Some(tool) = parsed.get("tool").and_then(|t| t.get("stackvet")) {
                let mut config: StackvetConfig = tool.clone().try_into().map_err(|e| {
                    ConfigError::ParseError(format!("pyproject.toml [tool.stackvet]: {}", e))
                })?;
                // If min_severity is configured in [tool.stackvet], map it to fail_on if not explicitly set
                if config.thresholds.fail_on.is_none() {
                    if let Some(min_sev) = tool.get("min_severity").and_then(|v| v.as_str()) {
                        config.thresholds.fail_on = Some(min_sev.to_string());
                    }
                }
                config.validate()?;
                return Ok(Some(config));
            }
        }

        Ok(None)
    }

    /// Applies rule overrides to a list of diagnostics:
    /// - Drops diagnostics for disabled rules.
    /// - Updates severity for diagnostics with overridden severity.
    pub fn apply_rule_overrides(
        &self,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<Vec<Diagnostic>, ConfigError> {
        self.validate()?;
        let mut result = Vec::new();

        for mut d in diagnostics {
            if let Some(setting) = self.rules.get(d.rule_id.as_str()) {
                match setting.to_override(d.rule_id.as_str())? {
                    RuleOverride::Disabled => {
                        // Suppressed / disabled: skip
                        continue;
                    }
                    RuleOverride::Severity(sev) => {
                        d.severity = sev;
                        result.push(d);
                    }
                    RuleOverride::KeepNative => {
                        result.push(d);
                    }
                }
            } else {
                result.push(d);
            }
        }

        Ok(result)
    }

    /// Resolves effective failure severity threshold across CLI options, config, and defaults.
    /// Precedence: CLI args > Config file > Defaults
    pub fn resolve_fail_on(
        &self,
        cli_fail_on: Option<Severity>,
        cli_strict: bool,
        default_sev: Severity,
    ) -> Severity {
        if let Some(sev) = cli_fail_on {
            return sev;
        }
        if cli_strict {
            return Severity::Warning;
        }

        if let Some(s) = &self.ci.fail_on {
            if let Ok(sev) = parse_severity(s) {
                return sev;
            }
        }
        if let Some(s) = &self.thresholds.fail_on {
            if let Ok(sev) = parse_severity(s) {
                return sev;
            }
        }
        if self.ci.strict == Some(true) || self.project.strict == Some(true) {
            return Severity::Warning;
        }

        default_sev
    }

    /// Resolves effective minimum health score across CLI options and config.
    pub fn resolve_min_score(&self, cli_min_score: Option<u32>) -> Option<u32> {
        if cli_min_score.is_some() {
            return cli_min_score;
        }
        self.thresholds.min_score.or(self.project.min_score)
    }

    /// Resolves effective baseline file path across CLI options and config.
    pub fn resolve_baseline(&self, cli_baseline: Option<PathBuf>) -> Option<PathBuf> {
        if cli_baseline.is_some() {
            return cli_baseline;
        }
        self.ci.baseline.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagnosticLocation, LanguageTarget, RuleId, TextSpan};

    fn make_diag(rule: &str, sev: Severity) -> Diagnostic {
        Diagnostic {
            rule_id: RuleId::new(rule),
            message: format!("Test {rule}"),
            severity: sev,
            location: DiagnosticLocation::new("test.py").with_span(TextSpan::new(1, 1, 1, 5)),
            explanation: "".to_string(),
            suggested_remediation: "".to_string(),
            fix: None,
            language: LanguageTarget::Python,
            confidence: crate::Confidence::High,
        }
    }

    #[test]
    fn test_config_rule_overrides_and_disabling() {
        let toml_str = r#"
[rules]
DEP-001 = "warning"
CEL-001 = "disabled"
ARC-001 = "error"
SQL-001 = { enabled = true }
"#;
        let config: StackvetConfig = toml::from_str(toml_str).unwrap();
        config.validate().unwrap();

        let diags = vec![
            make_diag("DEP-001", Severity::Error),
            make_diag("CEL-001", Severity::Error),
            make_diag("ARC-001", Severity::Warning),
            make_diag("FAP-001", Severity::Critical),
            make_diag("SQL-001", Severity::Critical),
        ];

        let filtered = config.apply_rule_overrides(diags).unwrap();
        assert_eq!(filtered.len(), 4); // CEL-001 dropped

        let dep = filtered
            .iter()
            .find(|d| d.rule_id.as_str() == "DEP-001")
            .unwrap();
        assert_eq!(dep.severity, Severity::Warning); // Overridden to Warning

        let arc = filtered
            .iter()
            .find(|d| d.rule_id.as_str() == "ARC-001")
            .unwrap();
        assert_eq!(arc.severity, Severity::Error); // Overridden to Error

        let fap = filtered
            .iter()
            .find(|d| d.rule_id.as_str() == "FAP-001")
            .unwrap();
        assert_eq!(fap.severity, Severity::Critical); // Unchanged

        let sql = filtered
            .iter()
            .find(|d| d.rule_id.as_str() == "SQL-001")
            .unwrap();
        assert_eq!(sql.severity, Severity::Critical); // Preserves native severity
    }

    #[test]
    fn test_config_validation_unknown_rule() {
        let toml_str = r#"
[rules]
NONEXISTENT-999 = "warning"
"#;
        let config: StackvetConfig = toml::from_str(toml_str).unwrap();
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::ValidationError(msg) => {
                assert!(msg.contains("Unknown rule ID 'NONEXISTENT-999'"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[test]
    fn test_config_validation_invalid_severity() {
        let toml_str = r#"
[rules]
DEP-001 = "catastrophic"
"#;
        let config: StackvetConfig = toml::from_str(toml_str).unwrap();
        let err = config.validate().unwrap_err();
        match err {
            ConfigError::ValidationError(msg) => {
                assert!(msg.contains("Invalid severity 'catastrophic'"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[test]
    fn test_precedence_resolution() {
        let toml_str = r#"
[thresholds]
fail_on = "warning"
min_score = 75

[ci]
baseline = "custom-baseline.json"
"#;
        let config: StackvetConfig = toml::from_str(toml_str).unwrap();

        // CLI overrides config
        assert_eq!(
            config.resolve_fail_on(Some(Severity::Critical), false, Severity::Error),
            Severity::Critical
        );
        // Config overrides default
        assert_eq!(
            config.resolve_fail_on(None, false, Severity::Error),
            Severity::Warning
        );
        // CLI strict forces warning
        assert_eq!(
            config.resolve_fail_on(None, true, Severity::Error),
            Severity::Warning
        );

        // Min score
        assert_eq!(config.resolve_min_score(Some(90)), Some(90));
        assert_eq!(config.resolve_min_score(None), Some(75));

        // Baseline
        assert_eq!(
            config.resolve_baseline(Some(PathBuf::from("cli-baseline.json"))),
            Some(PathBuf::from("cli-baseline.json"))
        );
        assert_eq!(
            config.resolve_baseline(None),
            Some(PathBuf::from("custom-baseline.json"))
        );
    }
}
