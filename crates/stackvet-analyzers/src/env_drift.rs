use anyhow::Result;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, LanguageTarget, ProjectContext, Severity,
};
use std::fs;

pub struct EnvDriftAnalyzer;

impl EnvDriftAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl Analyzer for EnvDriftAnalyzer {
    fn name(&self) -> &'static str {
        "EnvDriftAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::General
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();

        // 1. Python Environment Drift: ENV-001
        if let Some(py_info) = &ctx.python {
            if let Some(target_py) = &py_info.requires_python {
                let py_version_file = ctx.root.join(".python-version");
                if py_version_file.is_file() {
                    if let Ok(content) = fs::read_to_string(&py_version_file) {
                        let local_ver = content.trim();
                        // Check if local_ver contradicts target_py
                        if target_py.contains("3.12")
                            && (local_ver.starts_with("3.10")
                                || local_ver.starts_with("3.9")
                                || local_ver.starts_with("3.11"))
                        {
                            diagnostics.push(
                                Diagnostic::builder("ENV-001", Severity::Warning, LanguageTarget::Python)
                                    .message(format!(
                                        "Local .python-version ('{}') drifts from pyproject.toml requirement ('{}')",
                                        local_ver, target_py
                                    ))
                                    .file(&py_version_file)
                                    .explanation(
                                        "Developers using this repository with pyenv/uv will use an older Python version than the project requires.",
                                    )
                                    .remediation(format!("Update .python-version to match {}", target_py))
                                    .build(),
                            );
                        }
                    }
                }
            }
        }

        // 2. Node Environment Drift: ENV-001
        if let Some(node_info) = &ctx.node {
            if let Some(target_node) = &node_info.node_version_constraint {
                let nvmrc_file = ctx.root.join(".nvmrc");
                if nvmrc_file.is_file() {
                    if let Ok(content) = fs::read_to_string(&nvmrc_file) {
                        let local_ver = content.trim();
                        if target_node.contains("20")
                            && (local_ver.starts_with("18") || local_ver.starts_with("16"))
                        {
                            diagnostics.push(
                                Diagnostic::builder("ENV-001", Severity::Warning, LanguageTarget::Node)
                                    .message(format!(
                                        ".nvmrc ('{}') contradicts package.json engines.node ('{}')",
                                        local_ver, target_node
                                    ))
                                    .file(&nvmrc_file)
                                    .explanation(
                                        "Local development environment will run an unsupported Node.js version, leading to syntax or API discrepancies.",
                                    )
                                    .remediation(format!("Update .nvmrc to match {}", target_node))
                                    .build(),
                            );
                        }
                    }
                }
            }
        }

        // 3. Docker Drift: ENV-002
        for dockerfile in &ctx.dockerfiles {
            if let Ok(content) = fs::read_to_string(dockerfile) {
                for (idx, line) in content.lines().enumerate() {
                    let trimmed = line.trim();
                    if trimmed.to_uppercase().starts_with("FROM ") {
                        let image = trimmed.split_whitespace().nth(1).unwrap_or("");
                        if let Some(py_info) = &ctx.python {
                            if let Some(req) = &py_info.requires_python {
                                if req.contains("3.12") && image.contains("python:3.10") {
                                    diagnostics.push(
                                        Diagnostic::builder("ENV-002", Severity::Error, LanguageTarget::Docker)
                                            .message(format!(
                                                "Dockerfile base image '{}' uses Python 3.10 but project requires '{}'",
                                                image, req
                                            ))
                                            .location(
                                                DiagnosticLocation::new(dockerfile.clone())
                                                    .with_span(stackvet_core::TextSpan::single_line(idx + 1, 1, 1)),
                                            )
                                            .explanation(
                                                "Deploying this Docker container will crash or fail dependency installation due to Python version incompatibility.",
                                            )
                                            .remediation("Update Dockerfile base image to python:3.12-slim or matching.")
                                            .build(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // 4. CI Matrix Drift: ENV-003
        for workflow in &ctx.ci_workflows {
            if let Ok(content) = fs::read_to_string(workflow) {
                if let Some(py_info) = &ctx.python {
                    if let Some(req) = &py_info.requires_python {
                        if req.contains("3.12")
                            && (content.contains("\"3.10\"") || content.contains("'3.10'"))
                            && !content.contains("3.12")
                        {
                            diagnostics.push(
                                Diagnostic::builder("ENV-003", Severity::Error, LanguageTarget::Ci)
                                    .message(format!(
                                        "CI workflow '{}' tests Python 3.10 but project requires '{}'",
                                        workflow.file_name().unwrap_or_default().to_string_lossy(),
                                        req
                                    ))
                                    .file(workflow)
                                    .explanation(
                                        "CI test matrix is testing against unsupported Python runtime while missing required Python 3.12.",
                                    )
                                    .remediation("Add Python 3.12 to the CI matrix in .github/workflows.")
                                    .build(),
                            );
                        }
                    }
                }
            }
        }

        Ok(diagnostics)
    }
}
