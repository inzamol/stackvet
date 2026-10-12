use crate::Diagnostic;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineData {
    pub version: String,
    pub created_at: String,
    pub issue_fingerprints: HashSet<String>,
}

impl BaselineData {
    pub fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            created_at: chrono_timestamp(),
            issue_fingerprints: HashSet::new(),
        }
    }

    pub fn compute_fingerprint(d: &Diagnostic) -> String {
        format!(
            "{}:{}:{}",
            d.rule_id.as_str(),
            d.location.file_path.display(),
            d.location.span.as_ref().map(|s| s.start_line).unwrap_or(0)
        )
    }

    pub fn from_diagnostics(diagnostics: &[Diagnostic]) -> Self {
        let mut baseline = Self::new();
        for d in diagnostics {
            baseline
                .issue_fingerprints
                .insert(Self::compute_fingerprint(d));
        }
        baseline
    }

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

fn chrono_timestamp() -> String {
    "2026-10-11T12:00:00Z".to_string()
}
