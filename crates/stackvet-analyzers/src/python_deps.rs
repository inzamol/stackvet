use anyhow::Result;
use stackvet_core::{
    Analyzer, Confidence, Diagnostic, DiagnosticLocation, FixAction, LanguageTarget,
    ProjectContext, Severity,
};
use stackvet_python::stdlib::{is_pep594_removed, is_python_stdlib};
use stackvet_python::{
    check_pep_constraints_compatibility, normalize_package_name, parse_python_imports,
    parse_python_string_literals, pep_version_satisfies, ConstraintCompatibility,
};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

pub struct PythonDependencyAnalyzer {
    pub offline: bool,
}

impl PythonDependencyAnalyzer {
    pub fn new(offline: bool) -> Self {
        Self { offline }
    }

    /// Determines if an import root name corresponds to a local module, package, or PEP 420 namespace package in the repository.
    fn is_local_module_or_package(ctx: &ProjectContext, pkg_name: &str) -> bool {
        // 1. Direct file in project root or src/: <pkg_name>.py
        if ctx.root.join(format!("{}.py", pkg_name)).is_file()
            || ctx
                .root
                .join("src")
                .join(format!("{}.py", pkg_name))
                .is_file()
        {
            return true;
        }

        // 2. Package directory with __init__.py or PEP 420 namespace package (directory with .py files)
        let candidate_dirs = [ctx.root.join(pkg_name), ctx.root.join("src").join(pkg_name)];
        for dir in &candidate_dirs {
            if dir.is_dir() {
                if dir.join("__init__.py").is_file() {
                    return true;
                }
                if let Ok(entries) = fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("py")
                        {
                            return true;
                        }
                        if path.is_dir() {
                            return true;
                        }
                    }
                }
            }
        }

        // 3. Check ctx.source_files
        for file in &ctx.source_files {
            if let Ok(rel) = file.strip_prefix(&ctx.root) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                let clean_rel = if let Some(stripped) = rel_str.strip_prefix("src/") {
                    stripped
                } else {
                    &rel_str
                };

                if clean_rel == format!("{}.py", pkg_name)
                    || clean_rel.starts_with(&format!("{}/", pkg_name))
                {
                    return true;
                }
            }
        }

        false
    }
}

impl Analyzer for PythonDependencyAnalyzer {
    fn name(&self) -> &'static str {
        "PythonDependencyAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::Python
    }

    /// Checks Python imports, dependency declarations, and lockfile versions for drift.
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

        type PackageOccurrences<'a> = Vec<(
            &'a std::path::PathBuf,
            usize,
            Option<stackvet_core::TextSpan>,
        )>;

        // 1. Scan all imports in python files
        let mut imported_packages: BTreeMap<String, PackageOccurrences> = BTreeMap::new();
        let mut all_string_literals: HashSet<String> = HashSet::new();

        for file in &py_files {
            if let Ok(content) = fs::read_to_string(file) {
                let imports = parse_python_imports(&content, file);
                for imp in imports {
                    if imp.is_relative {
                        continue;
                    }
                    let root_pkg = imp.module.split('.').next().unwrap_or(&imp.module);
                    if is_python_stdlib(root_pkg) || is_pep594_removed(root_pkg) {
                        continue;
                    }
                    let norm = normalize_package_name(root_pkg);
                    let effective_span = imp.span.map(|s| {
                        if imp.statement_line < s.start_line {
                            stackvet_core::TextSpan::new(
                                imp.statement_line,
                                s.end_line,
                                1,
                                s.end_col,
                            )
                        } else {
                            s
                        }
                    });
                    imported_packages.entry(norm).or_default().push((
                        file,
                        imp.statement_line,
                        effective_span,
                    ));
                }
                all_string_literals.extend(parse_python_string_literals(&content));
            }
        }

        let all_declared_direct: HashSet<String> =
            py_info.direct_dependencies.keys().cloned().collect();
        let all_declared_dev: HashSet<String> = py_info.dev_dependencies.keys().cloned().collect();

        // Check DEP-002: Missing Declaration & DEP-007: Transitive Dependency Import
        for (pkg_name, occurrences) in &imported_packages {
            if is_python_stdlib(pkg_name) || is_pep594_removed(pkg_name) {
                continue;
            }

            // Check if matches a local module, package, or PEP 420 namespace package in the repository
            if Self::is_local_module_or_package(ctx, pkg_name) {
                continue;
            }

            // Check if matches a package directly or mapped via known PyPI distribution names
            let pypi_alts = stackvet_python::stdlib::get_known_pypi_packages(pkg_name);
            let covered_by_member = |file: &std::path::Path| {
                py_info.workspace_members.iter().any(|m| {
                    let declares = |name: &str| {
                        m.dependencies.contains_key(name) || m.dev_dependencies.contains_key(name)
                    };
                    file.starts_with(&m.root_dir)
                        && (declares(pkg_name.as_str())
                            || m.name == *pkg_name
                            || pypi_alts.is_some_and(|alts| alts.iter().any(|alt| declares(alt))))
                })
            };

            let (first_file, first_line, first_span) = occurrences
                .iter()
                .find(|(file, _, _)| !covered_by_member(file))
                .copied()
                .unwrap_or(occurrences[0]);

            let is_member_dep = covered_by_member(first_file);

            let is_declared = is_member_dep
                || all_declared_direct.contains(pkg_name)
                || all_declared_dev.contains(pkg_name)
                || py_info.workspace_packages.contains_key(pkg_name)
                || pypi_alts.is_some_and(|alts| {
                    alts.iter().any(|alt| {
                        all_declared_direct.contains(*alt) || all_declared_dev.contains(*alt)
                    })
                });

            if !is_declared {
                let target_pypi_name = pypi_alts
                    .and_then(|alts| alts.first().copied())
                    .unwrap_or(pkg_name.as_str());

                let loc = if let Some(span) = first_span {
                    DiagnosticLocation::new(first_file.to_path_buf()).with_span(span)
                } else {
                    DiagnosticLocation::new(first_file.to_path_buf())
                        .with_span(stackvet_core::TextSpan::single_line(first_line, 1, 1))
                };

                // Check if in lockfile (transitive dependency) -> DEP-007
                if py_info.installed_packages.contains_key(target_pypi_name) {
                    let mut builder = Diagnostic::builder(
                        "DEP-007",
                        Severity::Warning,
                        LanguageTarget::Python,
                    )
                    .message(format!(
                        "Transitive dependency '{}' imported without direct declaration in pyproject.toml",
                        target_pypi_name
                    ))
                    .location(loc)
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
                    ));

                    for &(occ_file, occ_line, ref occ_span) in occurrences
                        .iter()
                        .filter(|&&(f, l, _)| f != first_file || l != first_line)
                    {
                        let rel_loc = if let Some(s) = occ_span {
                            DiagnosticLocation::new(occ_file.to_path_buf()).with_span(*s)
                        } else {
                            DiagnosticLocation::new(occ_file.to_path_buf())
                                .with_span(stackvet_core::TextSpan::single_line(occ_line, 1, 1))
                        };
                        builder =
                            builder.related_location(rel_loc.with_message("Also imported here"));
                    }

                    diagnostics.push(builder.build());
                } else {
                    // Missing completely -> DEP-002
                    let mut builder = Diagnostic::builder(
                        "DEP-002",
                        Severity::Error,
                        LanguageTarget::Python,
                    )
                    .message(format!(
                        "Package '{}' is imported in source code but not declared in {}",
                        pkg_name,
                        py_info.manifest_file.file_name().unwrap_or_default().to_string_lossy()
                    ))
                    .location(loc)
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
                    ));

                    for &(occ_file, occ_line, ref occ_span) in occurrences
                        .iter()
                        .filter(|&&(f, l, _)| f != first_file || l != first_line)
                    {
                        let rel_loc = if let Some(s) = occ_span {
                            DiagnosticLocation::new(occ_file.to_path_buf()).with_span(*s)
                        } else {
                            DiagnosticLocation::new(occ_file.to_path_buf())
                                .with_span(stackvet_core::TextSpan::single_line(occ_line, 1, 1))
                        };
                        builder =
                            builder.related_location(rel_loc.with_message("Also imported here"));
                    }

                    diagnostics.push(builder.build());
                }
            }
        }

        // Check DEP-003: Unused Dependency
        let mut declared_pkgs: Vec<&String> = py_info.direct_dependencies.keys().collect();
        declared_pkgs.sort();
        for declared_pkg in declared_pkgs {
            let norm_declared = normalize_package_name(declared_pkg);
            let matches_import = imported_packages.contains_key(&norm_declared)
                || stackvet_python::stdlib::KNOWN_IMPORT_TO_PYPI
                    .iter()
                    .any(|(imp, pypis)| {
                        let imp_norm = normalize_package_name(imp);
                        let pypi_matches = pypis
                            .iter()
                            .any(|p| normalize_package_name(p) == norm_declared);
                        pypi_matches && imported_packages.contains_key(&imp_norm)
                    });

            if matches_import {
                continue;
            }

            // Whitelist common CLI / build / runner dependencies
            let cli_deps = [
                "uvicorn",
                "gunicorn",
                "hypercorn",
                "granian",
                "waitress",
                "daphne",
                "pytest",
                "black",
                "ruff",
                "mypy",
                "alembic",
                "celery",
                "pre-commit",
                "watchfiles",
                "flake8",
                "isort",
                "wheel",
                "setuptools",
                "twine",
                "sphinx",
                "coverage",
                "tox",
            ];
            if cli_deps.contains(&norm_declared.as_str()) {
                continue;
            }

            // Check dynamic references in AST string literals (e.g. settings, django apps, database URLs)
            let is_in_string_literals = all_string_literals.iter().any(|lit| {
                lit == declared_pkg
                    || lit == &norm_declared
                    || lit
                        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                        .any(|tok| normalize_package_name(tok) == norm_declared)
            });
            if is_in_string_literals {
                continue;
            }

            // Check if dependency is a known plugin, extension, or database driver loaded dynamically
            let is_plugin_or_driver = norm_declared.starts_with("django-")
                || norm_declared.starts_with("pytest-")
                || norm_declared.starts_with("flake8-")
                || norm_declared.starts_with("sphinx-")
                || norm_declared.starts_with("celery-")
                || norm_declared.starts_with("pydantic-")
                || matches!(
                    norm_declared.as_str(),
                    "asyncpg"
                        | "psycopg"
                        | "psycopg2"
                        | "psycopg2-binary"
                        | "pymysql"
                        | "aiomysql"
                        | "aiosqlite"
                        | "cx-oracle"
                        | "oracledb"
                        | "pymssql"
                        | "redis"
                        | "kombu"
                        | "pika"
                        | "pydantic-settings"
                        | "djangorestframework"
                );

            if is_plugin_or_driver {
                // Ambiguous or framework-loaded dependency: mark with Medium confidence, DO NOT attach removal FixAction
                diagnostics.push(
                    Diagnostic::builder("DEP-003", Severity::Warning, LanguageTarget::Python)
                        .message(format!(
                            "Declared dependency '{}' appears unused but matches dynamic plugin, driver, or framework extension pattern",
                            declared_pkg
                        ))
                        .file(&py_info.manifest_file)
                        .confidence(Confidence::Medium)
                        .explanation(format!(
                            "Dependency '{}' is not directly imported, but belongs to a known plugin or driver ecosystem. Automatic removal is withheld to avoid breaking dynamic configuration.",
                            declared_pkg
                        ))
                        .remediation(format!(
                            "Verify whether '{}' is configured in runtime settings or environment plugins before manual removal.",
                            declared_pkg
                        ))
                        .build(),
                );
            } else {
                // Clear unused dependency: mark with High confidence and attach safe removal FixAction
                diagnostics.push(
                    Diagnostic::builder("DEP-003", Severity::Warning, LanguageTarget::Python)
                        .message(format!(
                            "Declared dependency '{}' is not imported by any Python source files",
                            declared_pkg
                        ))
                        .file(&py_info.manifest_file)
                        .confidence(Confidence::High)
                        .explanation(format!(
                            "Dependency '{}' adds bloat to docker images and build times without being directly referenced.",
                            declared_pkg
                        ))
                        .remediation(format!(
                            "Remove '{}' if it is no longer needed, or move to dev dependencies.",
                            declared_pkg
                        ))
                        .fix(FixAction::new(
                            format!("Remove {} from dependencies", declared_pkg),
                            py_info.manifest_file.clone(),
                            declared_pkg.clone(),
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

            if let Some(lock_file_path) = &py_info.lock_file {
                let lock_name = lock_file_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();

                let mut all_manifest_deps: Vec<(&String, &String)> = Vec::new();
                for (k, v) in &py_info.direct_dependencies {
                    all_manifest_deps.push((k, v));
                }
                for (k, v) in &py_info.dev_dependencies {
                    all_manifest_deps.push((k, v));
                }
                all_manifest_deps.sort_by(|a, b| a.0.cmp(b.0));

                for (pkg_name, req_spec) in all_manifest_deps {
                    let norm = normalize_package_name(pkg_name);
                    let all_versions = py_info.locked_versions.get(&norm);
                    let fallback_ver = py_info.locked_packages.get(&norm);

                    let has_match = if let Some(versions) = all_versions {
                        if !versions.is_empty() {
                            versions.iter().any(|v| pep_version_satisfies(req_spec, v))
                        } else if let Some(ver) = fallback_ver {
                            pep_version_satisfies(req_spec, ver)
                        } else {
                            false
                        }
                    } else if let Some(ver) = fallback_ver {
                        pep_version_satisfies(req_spec, ver)
                    } else {
                        false
                    };

                    let reported_ver =
                        fallback_ver.or_else(|| all_versions.and_then(|v| v.first()));

                    if let Some(locked_ver) = reported_ver {
                        if !has_match {
                            diagnostics.push(
                                Diagnostic::builder("DEP-005", Severity::Warning, LanguageTarget::Python)
                                    .message(format!(
                                        "Lockfile dependency drift: package '{}' is locked at version '{}', which violates manifest requirement '{}'",
                                        pkg_name, locked_ver, req_spec
                                    ))
                                    .file(&py_info.manifest_file)
                                    .explanation(format!(
                                        "The version locked in {} does not satisfy the requirement in {}. Run '{} lock' to update the lockfile.",
                                        lock_name, manifest_name, py_info.package_manager
                                    ))
                                    .remediation(format!(
                                        "Run '{} lock' or '{} sync' to align the lockfile with manifest declarations.",
                                        py_info.package_manager, py_info.package_manager
                                    ))
                                    .build(),
                            );
                        }
                    } else if !py_info.workspace_packages.contains_key(&norm) {
                        diagnostics.push(
                            Diagnostic::builder("DEP-005", Severity::Warning, LanguageTarget::Python)
                                .message(format!(
                                    "Lockfile is out of sync: package '{}' declared in {} is missing from {}",
                                    pkg_name, manifest_name, lock_name
                                ))
                                .file(&py_info.manifest_file)
                                .explanation(
                                    "A dependency is declared in project configuration but does not exist in the lockfile.",
                                )
                                .remediation(format!(
                                    "Run '{} lock' to update and sync your lockfile.",
                                    py_info.package_manager
                                ))
                                .build(),
                        );
                    }
                }
            } else if manifest_name == "pyproject.toml"
                && (!py_info.direct_dependencies.is_empty() || !py_info.dev_dependencies.is_empty())
            {
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

        // Cross-Workspace Dependency Version Conflicts (DEP-001)
        if !py_info.workspace_members.is_empty() {
            let mut cross_deps: BTreeMap<String, Vec<(&str, &Path, &str)>> = BTreeMap::new();

            if !py_info.direct_dependencies.is_empty() {
                let root_name = "root";
                for (dep, constraint) in &py_info.direct_dependencies {
                    if !py_info.workspace_packages.contains_key(dep) {
                        cross_deps.entry(dep.clone()).or_default().push((
                            root_name,
                            &py_info.manifest_file,
                            constraint,
                        ));
                    }
                }
            }

            for member in &py_info.workspace_members {
                for (dep, constraint) in &member.dependencies {
                    if !py_info.workspace_packages.contains_key(dep) {
                        cross_deps.entry(dep.clone()).or_default().push((
                            &member.name,
                            &member.manifest_path,
                            constraint,
                        ));
                    }
                }
            }

            for (pkg_name, decls) in cross_deps {
                let distinct_members: HashSet<&str> = decls.iter().map(|(m, _, _)| *m).collect();
                if distinct_members.len() < 2 {
                    continue;
                }

                let member_pairs: Vec<(&str, &str)> =
                    decls.iter().map(|(m, _, c)| (*m, *c)).collect();
                let compatibility = check_pep_constraints_compatibility(&member_pairs);

                if let ConstraintCompatibility::Incompatible { reason, .. } = compatibility {
                    let first_manifest = decls[0].1;
                    let member_summary = decls
                        .iter()
                        .map(|(m, _, c)| format!("'{}' declares '{}'", m, c))
                        .collect::<Vec<_>>()
                        .join(", ");

                    let breakdown_lines = decls
                        .iter()
                        .map(|(m, path, c)| {
                            let rel = path
                                .strip_prefix(&ctx.root)
                                .unwrap_or(path)
                                .to_string_lossy()
                                .replace('\\', "/");
                            format!("- {} ({}): {}", m, rel, c)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    diagnostics.push(
                        Diagnostic::builder("DEP-001", Severity::Error, LanguageTarget::Python)
                            .message(format!(
                                "Conflicting dependency constraints for '{}' across workspace members: {}",
                                pkg_name, member_summary
                            ))
                            .file(first_manifest)
                            .explanation(format!(
                                "Workspace members declare mutually incompatible version constraints for '{}'. {}\nAffected workspace members:\n{}",
                                pkg_name, reason, breakdown_lines
                            ))
                            .remediation(format!(
                                "Align '{}' version constraints across all workspace members.",
                                pkg_name
                            ))
                            .build(),
                    );
                }
            }
        }

        Ok(diagnostics)
    }
}
