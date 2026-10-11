use anyhow::Result;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, FixAction, LanguageTarget, ProjectContext, Severity,
};
use stackvet_python::stdlib::{is_pep594_removed, is_python_stdlib};
use stackvet_python::{normalize_package_name, parse_python_imports};
use std::collections::{HashMap, HashSet};
use std::fs;

pub struct PythonDependencyAnalyzer {
    pub offline: bool,
}

impl PythonDependencyAnalyzer {
    pub fn new(offline: bool) -> Self {
        Self { offline }
    }
}

impl Analyzer for PythonDependencyAnalyzer {
    fn name(&self) -> &'static str {
        "PythonDependencyAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::Python
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        let py_info = match &ctx.python {
            Some(info) => info,
            None => return Ok(diagnostics),
        };

        // Collect all python files
        let py_files: Vec<&std::path::PathBuf> = ctx
            .source_files
            .iter()
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("py"))
            .collect();

        // 1. Scan all imports in python files
        let mut imported_packages: HashMap<String, Vec<(&std::path::PathBuf, usize)>> = HashMap::new();

        for file in &py_files {
            if let Ok(content) = fs::read_to_string(file) {
                let imports = parse_python_imports(&content, file);
                for imp in imports {
                    if imp.is_relative {
                        continue;
                    }
                    let root_pkg = imp.module.split('.').next().unwrap_or(&imp.module);
                    let norm = normalize_package_name(root_pkg);
                    imported_packages
                        .entry(norm)
                        .or_default()
                        .push((file, imp.line));
                }
            }
        }

        // Mapping of popular import names to PyPI package names
        let import_to_pypi: HashMap<&str, &str> = [
            ("yaml", "pyyaml"),
            ("dotenv", "python-dotenv"),
            ("jwt", "pyjwt"),
            ("cv2", "opencv-python"),
            ("dateutil", "python-dateutil"),
            ("bs4", "beautifulsoup4"),
            ("pil", "pillow"),
            ("serial", "pyserial"),
            ("magic", "python-magic"),
        ]
        .into_iter()
        .collect();

        let all_declared_direct: HashSet<String> = py_info
            .direct_dependencies
            .keys()
            .cloned()
            .collect();
        let all_declared_dev: HashSet<String> = py_info
            .dev_dependencies
            .keys()
            .cloned()
            .collect();

        // Check DEP-002: Missing Declaration & DEP-007: Transitive Dependency Import
        for (pkg_name, occurrences) in &imported_packages {
            if is_python_stdlib(pkg_name) || is_pep594_removed(pkg_name) {
                continue;
            }

            // Check if matches a package directly or mapped
            let pypi_alt = import_to_pypi.get(pkg_name.as_str()).copied();
            let is_declared = all_declared_direct.contains(pkg_name)
                || all_declared_dev.contains(pkg_name)
                || pypi_alt.map_or(false, |alt| {
                    all_declared_direct.contains(alt) || all_declared_dev.contains(alt)
                });

            if !is_declared {
                let target_pypi_name = pypi_alt.unwrap_or(pkg_name.as_str());
                let (first_file, first_line) = occurrences[0];

                // Check if in lockfile (transitive dependency) -> DEP-007
                if py_info.installed_packages.contains_key(target_pypi_name) {
                    diagnostics.push(
                        Diagnostic::builder(
                            "DEP-007",
                            Severity::Warning,
                            LanguageTarget::Python,
                        )
                        .message(format!(
                            "Transitive dependency '{}' imported without direct declaration in pyproject.toml",
                            target_pypi_name
                        ))
                        .location(
                            DiagnosticLocation::new(first_file.to_path_buf())
                                .with_span(stackvet_core::TextSpan::single_line(first_line, 1, 1)),
                        )
                        .explanation(format!(
                            "The package '{}' is resolved via lockfile/transitive dependencies but is missing from direct dependencies. Upgrading transitive dependencies can break production.",
                            target_pypi_name
                        ))
                        .remediation(format!(
                            "Add '{}' to [project.dependencies] or requirements.txt.",
                            target_pypi_name
                        ))
                        .fix(FixAction::new(
                            format!("Declare direct dependency on {}", target_pypi_name),
                            py_info.manifest_file.clone(),
                            target_pypi_name,
                        ))
                        .build(),
                    );
                } else {
                    // Missing completely -> DEP-002
                    diagnostics.push(
                        Diagnostic::builder(
                            "DEP-002",
                            Severity::Error,
                            LanguageTarget::Python,
                        )
                        .message(format!(
                            "Package '{}' is imported in source code but not declared in {}",
                            pkg_name,
                            py_info.manifest_file.file_name().unwrap_or_default().to_string_lossy()
                        ))
                        .location(
                            DiagnosticLocation::new(first_file.to_path_buf())
                                .with_span(stackvet_core::TextSpan::single_line(first_line, 1, 1)),
                        )
                        .explanation(format!(
                            "Import of '{}' will fail at runtime with ModuleNotFoundError unless installed.",
                            pkg_name
                        ))
                        .remediation(format!(
                            "Run 'uv add {}' or add '{}' to pyproject.toml dependencies.",
                            target_pypi_name, target_pypi_name
                        ))
                        .fix(FixAction::new(
                            format!("Add {} to pyproject.toml", target_pypi_name),
                            py_info.manifest_file.clone(),
                            target_pypi_name,
                        ))
                        .build(),
                    );
                }
            }
        }

        // Check DEP-003: Unused Dependency
        for (declared_pkg, _) in &py_info.direct_dependencies {
            let matches_import = imported_packages.contains_key(declared_pkg)
                || import_to_pypi
                    .iter()
                    .any(|(imp, pypi)| *pypi == declared_pkg && imported_packages.contains_key(*imp));

            // Whitelist common CLI / build / runner dependencies
            let cli_deps = [
                "uvicorn", "gunicorn", "pytest", "black", "ruff", "mypy", "alembic",
                "celery", "pre-commit", "watchfiles",
            ];
            if !matches_import && !cli_deps.contains(&declared_pkg.as_str()) {
                diagnostics.push(
                    Diagnostic::builder("DEP-003", Severity::Warning, LanguageTarget::Python)
                        .message(format!(
                            "Declared dependency '{}' is not imported by any Python source files",
                            declared_pkg
                        ))
                        .file(&py_info.manifest_file)
                        .explanation(format!(
                            "Dependency '{}' adds bloat to docker images and build times without being directly referenced.",
                            declared_pkg
                        ))
                        .remediation(format!(
                            "Remove '{}' if it is no longer needed, or move to dev dependencies.",
                            declared_pkg
                        ))
                        .build(),
                );
            }
        }

        // Check DEP-005: Lockfile Drift
        if py_info.manifest_file.is_file() {
            let manifest_name = py_info
                .manifest_file
                .file_name()
                .map(|s| s.to_string_lossy())
                .unwrap_or_default();
            if manifest_name == "pyproject.toml" && py_info.lock_file.is_none() {
                diagnostics.push(
                    Diagnostic::builder("DEP-005", Severity::Warning, LanguageTarget::Python)
                        .message("Missing lockfile (uv.lock / poetry.lock) for pyproject.toml")
                        .file(&py_info.manifest_file)
                        .explanation(
                            "Building without a lockfile risks non-deterministic builds and supply-chain drift.",
                        )
                        .remediation("Generate a lockfile using 'uv lock' or 'poetry lock'.")
                        .build(),
                );
            }
        }

        // Check DEP-004: Python Version Incompatible
        if let Some(req_py) = &py_info.requires_python {
            if req_py.contains("<3.10") || req_py.contains("<=3.9") {
                diagnostics.push(
                    Diagnostic::builder("DEP-004", Severity::Error, LanguageTarget::Python)
                        .message(format!(
                            "Python version constraint '{}' targets end-of-life Python (< 3.10)",
                            req_py
                        ))
                        .file(&py_info.manifest_file)
                        .explanation(
                            "Python 3.9 and older versions are end-of-life and no longer receive security updates.",
                        )
                        .remediation("Upgrade python version requirement to '>=3.10' or '>=3.11'.")
                        .build(),
                );
            }
        }

        Ok(diagnostics)
    }
}
