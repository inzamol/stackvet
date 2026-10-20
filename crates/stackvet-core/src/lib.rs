use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub mod rules_catalog;
pub use rules_catalog::{get_rule_meta, RuleMeta, ALL_RULES};

pub mod baseline;
pub use baseline::{BaselineData, BaselineDiff, BaselineIssueEntry};

pub mod config;
pub use config::{ConfigError, StackvetConfig};

pub mod exit_codes;
pub use exit_codes::ExitCode;

pub mod suppression;
pub use suppression::{
    apply_suppressions, parse_inline_suppressions, InlineSuppression, SuppressedDiagnostic,
    SuppressionEntry, SuppressionOutcome, SuppressionSource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    Hint,
    Info,
    Warning,
    Error,
    Critical,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Hint => "HINT",
            Severity::Info => "INFO",
            Severity::Warning => "WARN",
            Severity::Error => "ERROR",
            Severity::Critical => "CRITICAL",
        }
    }

    /// Health score deduction points associated with this severity level.
    ///
    /// StackVet uses an additive penalty model subtracted from a base health score of 100:
    /// - `Critical` (-20 pts): Catastrophic system failures, blocking event loops, fatal runtime breaks.
    /// - `Error` (-8 pts): Definite runtime breakages, missing dependencies, circular imports.
    /// - `Warning` (-2 pts): Transitive dependency risks, unpinned versions, runtime drift, anti-patterns.
    /// - `Info` (0 pts): Non-penalizing informational observations and architecture insights.
    /// - `Hint` (0 pts): Advisory recommendations.
    pub fn penalty_points(&self) -> i32 {
        match self {
            Severity::Critical => 20,
            Severity::Error => 8,
            Severity::Warning => 2,
            Severity::Info | Severity::Hint => 0,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Severity {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "hint" => Ok(Severity::Hint),
            "info" => Ok(Severity::Info),
            "warn" | "warning" => Ok(Severity::Warning),
            "error" => Ok(Severity::Error),
            "critical" | "fatal" => Ok(Severity::Critical),
            other => Err(format!(
                "Unknown severity '{}'. Expected one of: hint, info, warning, error, critical",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuleId(pub String);

impl RuleId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LanguageTarget {
    Python,
    Node,
    Docker,
    Ci,
    General,
}

impl LanguageTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            LanguageTarget::Python => "Python",
            LanguageTarget::Node => "Node.js",
            LanguageTarget::Docker => "Docker",
            LanguageTarget::Ci => "CI/CD",
            LanguageTarget::General => "General",
        }
    }
}

impl fmt::Display for LanguageTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextSpan {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl TextSpan {
    pub fn new(start_line: usize, end_line: usize, start_col: usize, end_col: usize) -> Self {
        Self {
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }

    pub fn single_line(line: usize, start_col: usize, end_col: usize) -> Self {
        Self::new(line, line, start_col, end_col)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DiagnosticLocation {
    pub file_path: PathBuf,
    pub span: Option<TextSpan>,
    pub snippet: Option<String>,
}

impl DiagnosticLocation {
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        Self {
            file_path: file_path.into(),
            span: None,
            snippet: None,
        }
    }

    pub fn with_span(mut self, span: TextSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_snippet(mut self, snippet: impl Into<String>) -> Self {
        self.snippet = Some(snippet.into());
        self
    }
}

impl Default for DiagnosticLocation {
    fn default() -> Self {
        Self {
            file_path: PathBuf::new(),
            span: None,
            snippet: None,
        }
    }
}

impl From<PathBuf> for DiagnosticLocation {
    fn from(file_path: PathBuf) -> Self {
        Self::new(file_path)
    }
}

impl From<&Path> for DiagnosticLocation {
    fn from(path: &Path) -> Self {
        Self::new(path)
    }
}

impl From<String> for DiagnosticLocation {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for DiagnosticLocation {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FixAction {
    pub description: String,
    pub file_path: PathBuf,
    pub replacement: String,
    pub span: Option<TextSpan>,
}

impl FixAction {
    pub fn new(
        description: impl Into<String>,
        file_path: impl Into<PathBuf>,
        replacement: impl Into<String>,
    ) -> Self {
        Self {
            description: description.into(),
            file_path: file_path.into(),
            replacement: replacement.into(),
            span: None,
        }
    }

    pub fn with_span(mut self, span: TextSpan) -> Self {
        self.span = Some(span);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Diagnostic {
    pub rule_id: RuleId,
    pub message: String,
    pub severity: Severity,
    pub location: DiagnosticLocation,
    pub explanation: String,
    pub suggested_remediation: String,
    pub fix: Option<FixAction>,
    pub language: LanguageTarget,
}

/// Contribution of an individual diagnostic to health score deduction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticContribution {
    pub rule_id: String,
    pub severity: Severity,
    pub penalty: u32,
    pub file_path: String,
    pub line: Option<usize>,
    pub message: String,
}

/// Aggregated contribution of a specific rule violation to health score deduction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleScoreContribution {
    pub rule_id: String,
    pub severity: Severity,
    pub penalty_per_finding: u32,
    pub count: usize,
    pub total_penalty: u32,
    pub title: String,
}

/// Comprehensive, explainable breakdown of the health score calculation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthScoreExplanation {
    /// Initial base score before deductions (standard: 100).
    pub base_score: u32,
    /// Final computed health score clamped between 0 and 100.
    pub final_score: u32,
    /// Letter grade representation (A, B, C, F).
    pub grade: String,
    /// Total points deducted from base score.
    pub total_deductions: u32,
    /// Count of unique diagnostics that contributed to the score.
    pub unique_findings_count: usize,
    /// Count of duplicate or redundant findings ignored during scoring.
    pub duplicate_findings_ignored: usize,
    /// Count of findings suppressed via configuration or inline comments.
    #[serde(default)]
    pub suppressed_findings_count: usize,
    /// Deductions grouped by rule ID.
    pub rule_contributions: Vec<RuleScoreContribution>,
    /// Exact diagnostics that incurred deductions.
    pub diagnostic_details: Vec<DiagnosticContribution>,
}

impl Diagnostic {
    pub fn builder(
        rule_id: impl Into<String>,
        severity: Severity,
        language: LanguageTarget,
    ) -> DiagnosticBuilder {
        DiagnosticBuilder::new(rule_id, severity, language)
    }
}

pub struct DiagnosticBuilder {
    rule_id: RuleId,
    message: Option<String>,
    severity: Severity,
    location: Option<DiagnosticLocation>,
    explanation: String,
    suggested_remediation: String,
    fix: Option<FixAction>,
    language: LanguageTarget,
}

impl DiagnosticBuilder {
    pub fn new(rule_id: impl Into<String>, severity: Severity, language: LanguageTarget) -> Self {
        Self {
            rule_id: RuleId::new(rule_id),
            message: None,
            severity,
            location: None,
            explanation: String::new(),
            suggested_remediation: String::new(),
            fix: None,
            language,
        }
    }

    pub fn message(mut self, msg: impl Into<String>) -> Self {
        self.message = Some(msg.into());
        self
    }

    pub fn location(mut self, loc: DiagnosticLocation) -> Self {
        self.location = Some(loc);
        self
    }

    pub fn file(mut self, file: impl Into<PathBuf>) -> Self {
        self.location = Some(DiagnosticLocation::new(file));
        self
    }

    pub fn explanation(mut self, exp: impl Into<String>) -> Self {
        self.explanation = exp.into();
        self
    }

    pub fn remediation(mut self, rem: impl Into<String>) -> Self {
        self.suggested_remediation = rem.into();
        self
    }

    pub fn fix(mut self, fix: FixAction) -> Self {
        self.fix = Some(fix);
        self
    }

    pub fn build(self) -> Diagnostic {
        let location = self
            .location
            .unwrap_or_else(|| DiagnosticLocation::new("<unknown>"));
        Diagnostic {
            rule_id: self.rule_id,
            message: self.message.unwrap_or_default(),
            severity: self.severity,
            location,
            explanation: self.explanation,
            suggested_remediation: self.suggested_remediation,
            fix: self.fix,
            language: self.language,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PythonProjectInfo {
    pub requires_python: Option<String>,
    pub package_manager: String, // uv, poetry, pip, pdm
    pub manifest_file: PathBuf,
    pub lock_file: Option<PathBuf>,
    pub direct_dependencies: HashMap<String, String>, // name -> version requirement
    pub dev_dependencies: HashMap<String, String>,
    pub installed_packages: HashMap<String, String>, // if virtualenv detected
    pub locked_packages: HashMap<String, String>,    // primary locked version per package
    pub locked_versions: HashMap<String, Vec<String>>, // all locked versions per package
    pub workspace_packages: HashMap<String, String>, // workspace package name -> version
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeProjectInfo {
    pub node_version_constraint: Option<String>,
    pub package_manager: String, // npm, pnpm, yarn, bun
    pub package_json_path: PathBuf,
    pub lock_file: Option<PathBuf>,
    pub dependencies: HashMap<String, String>,
    pub dev_dependencies: HashMap<String, String>,
    pub peer_dependencies: HashMap<String, String>,
    pub locked_packages: HashMap<String, String>, // primary locked version per package
    pub locked_versions: HashMap<String, Vec<String>>, // all locked versions per package
    pub workspace_packages: HashMap<String, String>, // workspace package name -> version
    pub tsconfig_paths: HashMap<String, Vec<String>>,
    pub tsconfig_base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    pub root: PathBuf,
    pub python: Option<PythonProjectInfo>,
    pub node: Option<NodeProjectInfo>,
    pub source_files: Vec<PathBuf>,
    pub dockerfiles: Vec<PathBuf>,
    pub ci_workflows: Vec<PathBuf>,
}

impl ProjectContext {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            python: None,
            node: None,
            source_files: Vec::new(),
            dockerfiles: Vec::new(),
            ci_workflows: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCause {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub impact_severity: Severity,
    pub primary_remediation: String,
    pub automated_fix: Option<FixAction>,
    pub symptoms: Vec<Diagnostic>,
}

pub trait Analyzer: Send + Sync {
    fn name(&self) -> &'static str;
    fn language(&self) -> LanguageTarget;
    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>>;
}
