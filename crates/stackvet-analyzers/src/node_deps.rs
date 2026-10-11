use anyhow::Result;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, FixAction, LanguageTarget, ProjectContext, Severity,
};
use stackvet_node::builtins::is_node_builtin;
use stackvet_node::{extract_package_name, parse_node_imports};
use std::collections::{HashMap, HashSet};
use std::fs;

pub struct NodeDependencyAnalyzer;

impl NodeDependencyAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl Analyzer for NodeDependencyAnalyzer {
    fn name(&self) -> &'static str {
        "NodeDependencyAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::Node
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        let node_info = match &ctx.node {
            Some(info) => info,
            None => return Ok(diagnostics),
        };

        let node_files: Vec<&std::path::PathBuf> = ctx
            .source_files
            .iter()
            .filter(|p| {
                if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                    matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs")
                } else {
                    false
                }
            })
            .collect();

        // 1. Collect all imports
        let mut imported_packages: HashMap<String, Vec<(&std::path::PathBuf, usize)>> = HashMap::new();

        for file in &node_files {
            if let Ok(content) = fs::read_to_string(file) {
                let imports = parse_node_imports(&content, file);
                for imp in imports {
                    if is_node_builtin(&imp.specifier) {
                        continue;
                    }
                    if let Some(pkg_name) = extract_package_name(&imp.specifier) {
                        imported_packages
                            .entry(pkg_name)
                            .or_default()
                            .push((file, imp.line));
                    }
                }
            }
        }

        let all_deps: HashSet<&str> = node_info.dependencies.keys().map(|s| s.as_str()).collect();
        let all_dev_deps: HashSet<&str> = node_info
            .dev_dependencies
            .keys()
            .map(|s| s.as_str())
            .collect();

        // NDEP-001: Missing Node Package
        for (pkg_name, occurrences) in &imported_packages {
            if !all_deps.contains(pkg_name.as_str()) && !all_dev_deps.contains(pkg_name.as_str()) {
                let (first_file, first_line) = occurrences[0];
                diagnostics.push(
                    Diagnostic::builder("NDEP-001", Severity::Error, LanguageTarget::Node)
                        .message(format!(
                            "Package '{}' is imported in source code but not declared in package.json",
                            pkg_name
                        ))
                        .location(
                            DiagnosticLocation::new(first_file.to_path_buf())
                                .with_span(stackvet_core::TextSpan::single_line(first_line, 1, 1)),
                        )
                        .explanation(format!(
                            "Importing undeclared package '{}' will fail at runtime or during production build.",
                            pkg_name
                        ))
                        .remediation(format!(
                            "Run '{} add {}' or add to package.json dependencies.",
                            node_info.package_manager, pkg_name
                        ))
                        .fix(FixAction::new(
                            format!("Add {} to package.json", pkg_name),
                            node_info.package_json_path.clone(),
                            pkg_name.clone(),
                        ))
                        .build(),
                );
            }
        }

        // NDEP-002: Unused Node Package
        for (dep_name, _) in &node_info.dependencies {
            let matches_import = imported_packages.contains_key(dep_name);
            // Ignore build tools, type definitions or runtime plugins often in dependencies
            let whitelist = [
                "react", "react-dom", "next", "tailwindcss", "postcss", "autoprefixer",
                "typescript",
            ];
            if !matches_import && !whitelist.contains(&dep_name.as_str()) {
                diagnostics.push(
                    Diagnostic::builder("NDEP-002", Severity::Warning, LanguageTarget::Node)
                        .message(format!(
                            "Declared production dependency '{}' is not imported in any source files",
                            dep_name
                        ))
                        .file(&node_info.package_json_path)
                        .explanation(format!(
                            "Package '{}' increases bundle size and node_modules installation overhead without active usage.",
                            dep_name
                        ))
                        .remediation(format!(
                            "Remove '{}' from package.json or move to devDependencies.",
                            dep_name
                        ))
                        .build(),
                );
            }
        }

        // NDEP-005: DevDependency in Production Source
        for (dev_pkg, _) in &node_info.dev_dependencies {
            if let Some(occurrences) = imported_packages.get(dev_pkg) {
                for (file, line) in occurrences {
                    let file_str = file.to_string_lossy().replace('\\', "/");
                    let is_test_file = file_str.contains(".test.")
                        || file_str.contains(".spec.")
                        || file_str.contains("/tests/")
                        || file_str.contains("/__tests__/")
                        || file_str.ends_with(".config.ts")
                        || file_str.ends_with(".config.js");

                    if !is_test_file {
                        diagnostics.push(
                            Diagnostic::builder("NDEP-005", Severity::Error, LanguageTarget::Node)
                                .message(format!(
                                    "DevDependency '{}' is imported in production file '{}'",
                                    dev_pkg,
                                    file.file_name().unwrap_or_default().to_string_lossy()
                                ))
                                .location(
                                    DiagnosticLocation::new((*file).clone())
                                        .with_span(stackvet_core::TextSpan::single_line(*line, 1, 1)),
                                )
                                .explanation(format!(
                                    "Packages in devDependencies are not installed in production environments (NODE_ENV=production), causing runtime crash when '{}' is imported.",
                                    dev_pkg
                                ))
                                .remediation(format!(
                                    "Move '{}' to dependencies in package.json.",
                                    dev_pkg
                                ))
                                .build(),
                        );
                        break; // report once per package
                    }
                }
            }
        }

        // NDEP-004: Lockfile Out of Sync / Missing
        if node_info.lock_file.is_none() {
            diagnostics.push(
                Diagnostic::builder("NDEP-004", Severity::Warning, LanguageTarget::Node)
                    .message("No lockfile detected (pnpm-lock.yaml / package-lock.json / yarn.lock)")
                    .file(&node_info.package_json_path)
                    .explanation(
                        "Lockfiles ensure deterministic, reproducible builds and protect against untracked dependency changes.",
                    )
                    .remediation(format!("Run '{} install' to generate a lockfile.", node_info.package_manager))
                    .build(),
            );
        }

        Ok(diagnostics)
    }
}
