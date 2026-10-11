use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

pub mod rules_catalog;
pub use rules_catalog::{ALL_RULES, RuleMeta, get_rule_meta};

pub mod baseline;
pub use baseline::BaselineData;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSpan {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl TextSpan {
    pub fn single_line(line: usize, start_col: usize, end_col: usize) -> Self {
        Self {
            start_line: line,
            start_col,
            end_line: line,
            end_col,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub fn new(
        rule_id: impl Into<String>,
        severity: Severity,
        language: LanguageTarget,
    ) -> Self {
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
        let location = self.location.unwrap_or_else(|| DiagnosticLocation::new("<unknown>"));
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
    pub installed_packages: HashMap<String, String>,   // if virtualenv detected
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
    pub tsconfig_paths: HashMap<String, Vec<String>>,
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
