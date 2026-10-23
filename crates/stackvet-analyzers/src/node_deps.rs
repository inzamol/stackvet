use anyhow::Result;
use stackvet_core::{
    Analyzer, Confidence, Diagnostic, DiagnosticLocation, FixAction, LanguageTarget,
    ProjectContext, Severity,
};
use stackvet_node::builtins::is_node_builtin;
use stackvet_node::{
    check_npm_constraints_compatibility, extract_package_name, is_tsconfig_alias,
    npm_version_satisfies, parse_node_imports, resolve_aliased_import, ConstraintCompatibility,
};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

pub struct NodeDependencyAnalyzer;

impl NodeDependencyAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NodeDependencyAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer for NodeDependencyAnalyzer {
    fn name(&self) -> &'static str {
        "NodeDependencyAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::Node
    }

    /// Checks Node imports, dependency declarations, and lockfile versions for drift.
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

        type PackageOccurrences<'a> = Vec<(
            &'a std::path::PathBuf,
            usize,
            Option<stackvet_core::TextSpan>,
        )>;

        // 1. Collect all imports
        let mut imported_packages: BTreeMap<String, PackageOccurrences> = BTreeMap::new();

        for file in &node_files {
            if let Ok(content) = fs::read_to_string(file) {
                let imports = parse_node_imports(&content, file);
                for imp in imports {
                    if is_node_builtin(&imp.specifier) {
                        continue;
                    }
                    if imp.specifier.starts_with('#') {
                        if let Some(target_pkg) = stackvet_node::resolve_package_subpath_package(
                            &node_info.package_imports,
                            &imp.specifier,
                        ) {
                            if let Some(pkg_name) = extract_package_name(&target_pkg) {
                                imported_packages
                                    .entry(pkg_name)
                                    .or_default()
                                    .push((file, imp.line, imp.span));
                            }
                        }
                        continue;
                    }
                    if is_tsconfig_alias(&imp.specifier, &node_info.tsconfig_paths) {
                        continue;
                    }
                    if resolve_aliased_import(
                        &ctx.root,
                        &node_info.tsconfig_paths,
                        node_info.tsconfig_base_url.as_deref(),
                        &imp.specifier,
                    )
                    .is_some()
                    {
                        continue;
                    }
                    if let Some(pkg_name) = extract_package_name(&imp.specifier) {
                        imported_packages
                            .entry(pkg_name)
                            .or_default()
                            .push((file, imp.line, imp.span));
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
            // A package is declared if in root dependencies/devDependencies
            let is_root =
                all_deps.contains(pkg_name.as_str()) || all_dev_deps.contains(pkg_name.as_str());

            if is_root {
                continue;
            }

            // Find the first occurrence not covered by its enclosing member's dependencies or self-reference
            let covered_by_member = |file: &Path| {
                node_info.workspace_members.iter().any(|m| {
                    file.starts_with(&m.root_dir)
                        && (m.dependencies.contains_key(pkg_name.as_str())
                            || m.dev_dependencies.contains_key(pkg_name.as_str())
                            || m.peer_dependencies.contains_key(pkg_name.as_str())
                            || m.optional_dependencies.contains_key(pkg_name.as_str())
                            || m.name == *pkg_name)
                })
            };

            let uncovered_occurrence = occurrences
                .iter()
                .find(|(file, _, _)| !covered_by_member(file));

            if let Some(&(first_file, first_line, ref first_span)) = uncovered_occurrence {
                let enclosing_member = node_info
                    .workspace_members
                    .iter()
                    .find(|m| first_file.starts_with(&m.root_dir));

                let target_manifest = enclosing_member
                    .map(|m| m.manifest_path.clone())
                    .unwrap_or_else(|| node_info.package_json_path.clone());

                let manifest_name = target_manifest
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();

                // Check if target package is an internal workspace package that was imported without declaration
                let is_internal_pkg = node_info
                    .workspace_members
                    .iter()
                    .any(|m| m.name == *pkg_name);

                let remediation = if is_internal_pkg {
                    format!(
                        "Add '{}' as a workspace dependency in {} (e.g. \"{}\": \"workspace:*\").",
                        pkg_name, manifest_name, pkg_name
                    )
                } else {
                    format!(
                        "Run '{} add {}' or add to {} dependencies.",
                        node_info.package_manager, pkg_name, manifest_name
                    )
                };

                let loc = if let Some(span) = first_span {
                    DiagnosticLocation::new(first_file.to_path_buf()).with_span(*span)
                } else {
                    DiagnosticLocation::new(first_file.to_path_buf())
                        .with_span(stackvet_core::TextSpan::single_line(first_line, 1, 1))
                };

                let mut builder =
                    Diagnostic::builder("NDEP-001", Severity::Error, LanguageTarget::Node)
                        .message(format!(
                            "Package '{}' is imported in source code but not declared in {}",
                            pkg_name, manifest_name
                        ))
                        .location(loc)
                        .explanation(format!(
                            "Importing undeclared package '{}' will fail at runtime or during production build.",
                            pkg_name
                        ))
                        .remediation(remediation)
                        .fix(FixAction::new(
                            format!("Add {} to {}", pkg_name, manifest_name),
                            target_manifest,
                            pkg_name.clone(),
                        ));

                for &(occ_file, occ_line, ref occ_span) in
                    occurrences.iter().filter(|&&(f, l, _)| {
                        (f != first_file || l != first_line) && !covered_by_member(f)
                    })
                {
                    let rel_loc = if let Some(s) = occ_span {
                        DiagnosticLocation::new(occ_file.to_path_buf()).with_span(*s)
                    } else {
                        DiagnosticLocation::new(occ_file.to_path_buf())
                            .with_span(stackvet_core::TextSpan::single_line(occ_line, 1, 1))
                    };
                    builder = builder.related_location(rel_loc.with_message("Also imported here"));
                }

                diagnostics.push(builder.build());
            }
        }

        // Collect all scripts across root package.json and workspace members
        let mut all_scripts: Vec<&String> = node_info.scripts.values().collect();
        for m in &node_info.workspace_members {
            all_scripts.extend(m.scripts.values());
        }

        // NDEP-002: Unused Node Package
        let mut dep_names: Vec<&String> = node_info.dependencies.keys().collect();
        dep_names.sort();
        for dep_name in dep_names {
            let matches_import = imported_packages.contains_key(dep_name);
            if matches_import {
                continue;
            }

            // Whitelist build tools, framework conventions, type definitions
            let whitelist = [
                "react",
                "react-dom",
                "next",
                "tailwindcss",
                "postcss",
                "autoprefixer",
                "typescript",
                "tsx",
                "ts-node",
                "nodemon",
                "webpack",
                "vite",
                "rollup",
                "esbuild",
                "babel",
                "eslint",
                "prettier",
                "jest",
                "vitest",
                "rimraf",
                "cross-env",
                "dotenv",
                "dotenv-cli",
                "concurrently",
                "npm-run-all",
            ];
            if whitelist.contains(&dep_name.as_str()) || dep_name.starts_with("@types/") {
                continue;
            }

            // Check if invoked in package.json scripts (e.g. "build": "prisma generate")
            let is_in_scripts = all_scripts.iter().any(|cmd| {
                cmd.split(|c: char| c.is_whitespace() || c == '&' || c == '|' || c == ';')
                    .any(|tok| tok == dep_name.as_str() || tok.ends_with(&format!("/{}", dep_name)))
            });
            if is_in_scripts {
                continue;
            }

            // Check for plugin / config packages loaded dynamically by external tooling
            let is_plugin_or_config = dep_name.starts_with("eslint-plugin-")
                || dep_name.starts_with("eslint-config-")
                || dep_name.starts_with("prettier-plugin-")
                || dep_name.starts_with("babel-plugin-")
                || dep_name.starts_with("babel-preset-")
                || dep_name.starts_with("postcss-")
                || dep_name.starts_with("rollup-plugin-")
                || dep_name.starts_with("vite-plugin-")
                || dep_name.starts_with("@typescript-eslint/")
                || dep_name.starts_with("@babel/");

            if is_plugin_or_config {
                diagnostics.push(
                    Diagnostic::builder("NDEP-002", Severity::Warning, LanguageTarget::Node)
                        .message(format!(
                            "Declared production dependency '{}' appears unused but matches build plugin or config pattern",
                            dep_name
                        ))
                        .file(&node_info.package_json_path)
                        .confidence(Confidence::Medium)
                        .explanation(format!(
                            "Package '{}' is not directly imported in code, but may be dynamically invoked by external configuration files. Automatic removal is withheld.",
                            dep_name
                        ))
                        .remediation(format!(
                            "Verify if '{}' is used in tool configuration or move it to devDependencies.",
                            dep_name
                        ))
                        .build(),
                );
            } else {
                diagnostics.push(
                    Diagnostic::builder("NDEP-002", Severity::Warning, LanguageTarget::Node)
                        .message(format!(
                            "Declared production dependency '{}' is not imported in any source files",
                            dep_name
                        ))
                        .file(&node_info.package_json_path)
                        .confidence(Confidence::High)
                        .explanation(format!(
                            "Package '{}' increases bundle size and node_modules installation overhead without active usage.",
                            dep_name
                        ))
                        .remediation(format!(
                            "Remove '{}' from package.json or move to devDependencies.",
                            dep_name
                        ))
                        .fix(FixAction::new(
                            format!("Remove {} from dependencies", dep_name),
                            node_info.package_json_path.clone(),
                            dep_name.clone(),
                        ))
                        .build(),
                );
            }
        }

        // NDEP-005: DevDependency in Production Source
        let mut all_dev_names: HashSet<&String> = node_info.dev_dependencies.keys().collect();
        for member in &node_info.workspace_members {
            all_dev_names.extend(member.dev_dependencies.keys());
        }
        let mut dev_pkgs: Vec<&String> = all_dev_names.into_iter().collect();
        dev_pkgs.sort();

        for dev_pkg in dev_pkgs {
            if let Some(occurrences) = imported_packages.get(dev_pkg) {
                for (file, line, span) in occurrences {
                    let file_str = file.to_string_lossy().replace('\\', "/");
                    let is_test_file = file_str.contains(".test.")
                        || file_str.contains(".spec.")
                        || file_str.contains("/tests/")
                        || file_str.contains("/__tests__/")
                        || file_str.ends_with(".config.ts")
                        || file_str.ends_with(".config.js");

                    if is_test_file {
                        continue;
                    }

                    let member = node_info
                        .workspace_members
                        .iter()
                        .find(|m| file.starts_with(&m.root_dir));

                    let is_dev_violation = if let Some(m) = member {
                        // If member lists it as production, peer, or optional dependency, it's valid in production
                        if m.dependencies.contains_key(dev_pkg)
                            || m.peer_dependencies.contains_key(dev_pkg)
                            || m.optional_dependencies.contains_key(dev_pkg)
                        {
                            false
                        } else if m.dev_dependencies.contains_key(dev_pkg) {
                            true
                        } else {
                            // Fall back to root definitions
                            !node_info.dependencies.contains_key(dev_pkg)
                                && node_info.dev_dependencies.contains_key(dev_pkg)
                        }
                    } else {
                        // Root file: check root dependencies
                        !node_info.dependencies.contains_key(dev_pkg)
                            && node_info.dev_dependencies.contains_key(dev_pkg)
                    };

                    if is_dev_violation {
                        let manifest_name = member
                            .map(|m| {
                                m.manifest_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                            })
                            .unwrap_or_else(|| {
                                node_info
                                    .package_json_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                            });

                        let loc = if let Some(s) = span {
                            DiagnosticLocation::new((*file).clone()).with_span(*s)
                        } else {
                            DiagnosticLocation::new((*file).clone())
                                .with_span(stackvet_core::TextSpan::single_line(*line, 1, 1))
                        };

                        diagnostics.push(
                            Diagnostic::builder("NDEP-005", Severity::Error, LanguageTarget::Node)
                                .message(format!(
                                    "DevDependency '{}' is imported in production file '{}'",
                                    dev_pkg,
                                    file.file_name().unwrap_or_default().to_string_lossy()
                                ))
                                .location(loc)
                                .explanation(format!(
                                    "Packages in devDependencies are not installed in production environments (NODE_ENV=production), causing runtime crash when '{}' is imported.",
                                    dev_pkg
                                ))
                                .remediation(format!(
                                    "Move '{}' to dependencies in {}.",
                                    dev_pkg, manifest_name
                                ))
                                .build(),
                        );
                        break; // report once per package
                    }
                }
            }
        }

        // NDEP-004: Lockfile Out of Sync / Missing & Drift
        if let Some(lock_file_path) = &node_info.lock_file {
            let lock_name = lock_file_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();

            let mut all_manifest_deps: Vec<(&String, &String)> = Vec::new();
            for (k, v) in &node_info.dependencies {
                all_manifest_deps.push((k, v));
            }
            for (k, v) in &node_info.dev_dependencies {
                all_manifest_deps.push((k, v));
            }
            all_manifest_deps.sort_by(|a, b| a.0.cmp(b.0));

            for (dep_name, constraint) in all_manifest_deps {
                let is_workspace_dep = constraint.starts_with("workspace:")
                    || constraint.starts_with("file:")
                    || constraint.starts_with("link:");

                if is_workspace_dep {
                    // Check workspace dependency resolution
                    if constraint.starts_with("workspace:") {
                        if let Some(ws_ver) = node_info.workspace_packages.get(dep_name) {
                            if ws_ver != "*" && !npm_version_satisfies(constraint, ws_ver) {
                                diagnostics.push(
                                    Diagnostic::builder("NDEP-004", Severity::Warning, LanguageTarget::Node)
                                        .message(format!(
                                            "Workspace dependency '{}' constraint '{}' in package.json does not match local package version '{}'",
                                            dep_name, constraint, ws_ver
                                        ))
                                        .file(&node_info.package_json_path)
                                        .explanation(
                                            "The declared workspace dependency constraint conflicts with the actual version of the local package in the workspace.",
                                        )
                                        .remediation(format!(
                                            "Update package.json constraint to match workspace package version '{}'.",
                                            ws_ver
                                        ))
                                        .build(),
                                );
                            }
                        } else if !node_info.locked_packages.contains_key(dep_name) {
                            diagnostics.push(
                                Diagnostic::builder("NDEP-004", Severity::Warning, LanguageTarget::Node)
                                    .message(format!(
                                        "Workspace dependency '{}' declared in package.json cannot be resolved in workspace or lockfile",
                                        dep_name
                                    ))
                                    .file(&node_info.package_json_path)
                                    .explanation(
                                        "Workspace dependency references a local package that does not exist or is not linked in the lockfile.",
                                    )
                                    .remediation(format!(
                                        "Verify package '{}' exists in workspace packages or run '{} install'.",
                                        dep_name, node_info.package_manager
                                    ))
                                    .build(),
                            );
                        }
                    } else if constraint.starts_with("file:") || constraint.starts_with("link:") {
                        let raw_path = constraint
                            .strip_prefix("file:")
                            .or_else(|| constraint.strip_prefix("link:"))
                            .unwrap_or(constraint)
                            .trim();
                        let target_path = ctx.root.join(raw_path);
                        if !target_path.exists() {
                            diagnostics.push(
                                Diagnostic::builder("NDEP-004", Severity::Warning, LanguageTarget::Node)
                                    .message(format!(
                                        "Local dependency path '{}' declared in package.json cannot be resolved on disk",
                                        raw_path
                                    ))
                                    .file(&node_info.package_json_path)
                                    .explanation(
                                        "Local file or link dependency references a path that does not exist on disk.",
                                    )
                                    .remediation(format!(
                                        "Verify the relative path '{}' points to an existing directory or package.",
                                        raw_path
                                    ))
                                    .build(),
                            );
                        }
                    }
                } else {
                    // Regular package: check stale entries and constraint drift
                    let all_versions = node_info.locked_versions.get(dep_name);
                    let fallback_ver = node_info.locked_packages.get(dep_name);

                    let has_match = if let Some(versions) = all_versions {
                        if !versions.is_empty() {
                            versions
                                .iter()
                                .any(|v| npm_version_satisfies(constraint, v))
                        } else if let Some(ver) = fallback_ver {
                            npm_version_satisfies(constraint, ver)
                        } else {
                            false
                        }
                    } else if let Some(ver) = fallback_ver {
                        npm_version_satisfies(constraint, ver)
                    } else {
                        false
                    };

                    let reported_ver =
                        fallback_ver.or_else(|| all_versions.and_then(|v| v.first()));

                    if let Some(locked_ver) = reported_ver {
                        if !has_match {
                            diagnostics.push(
                                Diagnostic::builder("NDEP-004", Severity::Warning, LanguageTarget::Node)
                                    .message(format!(
                                        "Lockfile dependency drift: package '{}' is locked at version '{}', which violates manifest constraint '{}'",
                                        dep_name, locked_ver, constraint
                                    ))
                                    .file(&node_info.package_json_path)
                                    .explanation(format!(
                                        "The version locked in {} does not satisfy the constraint in package.json. Run '{} install' to align the lockfile with manifest declarations.",
                                        lock_name, node_info.package_manager
                                    ))
                                    .remediation(format!(
                                        "Run '{} install' to update {} to satisfy '{}'.",
                                        node_info.package_manager, dep_name, constraint
                                    ))
                                    .build(),
                            );
                        }
                    } else if !node_info.workspace_packages.contains_key(dep_name) {
                        diagnostics.push(
                            Diagnostic::builder("NDEP-004", Severity::Warning, LanguageTarget::Node)
                                .message(format!(
                                    "Lockfile is out of sync: package '{}' declared in package.json is missing from {}",
                                    dep_name, lock_name
                                ))
                                .file(&node_info.package_json_path)
                                .explanation(
                                    "A dependency is declared in package.json but does not exist in the lockfile. This indicates an uncommitted package or out-of-sync lockfile.",
                                )
                                .remediation(format!(
                                    "Run '{} install' to update and sync your lockfile.",
                                    node_info.package_manager
                                ))
                                .build(),
                        );
                    }
                }
            }

            // NDEP-006: Inconsistent Dependency Versions (Multiple conflicting versions of singleton packages)
            let singletons = ["react", "react-dom", "@types/react", "vue", "graphql"];
            for singleton in singletons {
                if let Some(versions) = node_info.locked_versions.get(singleton) {
                    let unique_versions: HashSet<&str> =
                        versions.iter().map(|s| s.as_str()).collect();
                    if unique_versions.len() > 1 {
                        let major_versions: HashSet<&str> = unique_versions
                            .iter()
                            .filter_map(|v| v.split('.').next())
                            .collect();
                        if major_versions.len() > 1 {
                            let mut sorted_vers: Vec<&str> = unique_versions.into_iter().collect();
                            sorted_vers.sort();
                            diagnostics.push(
                                Diagnostic::builder("NDEP-006", Severity::Warning, LanguageTarget::Node)
                                    .message(format!(
                                        "Multiple conflicting versions of '{}' are locked in {}: {}",
                                        singleton,
                                        lock_name,
                                        sorted_vers.join(", ")
                                    ))
                                    .file(lock_file_path)
                                    .explanation(format!(
                                        "Package '{}' relies on singleton runtime state. Having multiple distinct versions locked in the dependency tree leads to dual-instance bugs and invalid hook calls.",
                                        singleton
                                    ))
                                    .remediation(format!(
                                        "Deduplicate dependencies or align peer dependency requirements using '{} dedupe'.",
                                        node_info.package_manager
                                    ))
                                    .build(),
                            );
                        }
                    }
                }
            }
        } else if !node_info.dependencies.is_empty() || !node_info.dev_dependencies.is_empty() {
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

        // Validate workspace member dependencies and broken references
        for member in &node_info.workspace_members {
            let mut member_deps = Vec::new();
            for (k, v) in &member.dependencies {
                member_deps.push((k, v));
            }
            for (k, v) in &member.dev_dependencies {
                member_deps.push((k, v));
            }
            for (k, v) in &member.peer_dependencies {
                member_deps.push((k, v));
            }
            member_deps.sort_by(|a, b| a.0.cmp(b.0));

            for (dep_name, constraint) in member_deps {
                if constraint.starts_with("workspace:") {
                    if let Some(ws_ver) = node_info.workspace_packages.get(dep_name) {
                        if ws_ver != "*" && !npm_version_satisfies(constraint, ws_ver) {
                            diagnostics.push(
                                Diagnostic::builder("NDEP-004", Severity::Warning, LanguageTarget::Node)
                                    .message(format!(
                                        "Workspace dependency '{}' constraint '{}' in {} does not match local package version '{}'",
                                        dep_name,
                                        constraint,
                                        member.manifest_path.file_name().unwrap_or_default().to_string_lossy(),
                                        ws_ver
                                    ))
                                    .file(&member.manifest_path)
                                    .explanation(
                                        "The declared workspace dependency constraint conflicts with the actual version of the local package in the workspace.",
                                    )
                                    .remediation(format!(
                                        "Update package.json constraint to match workspace package version '{}'.",
                                        ws_ver
                                    ))
                                    .build(),
                            );
                        }
                    } else if !node_info.locked_packages.contains_key(dep_name) {
                        diagnostics.push(
                            Diagnostic::builder("NDEP-004", Severity::Warning, LanguageTarget::Node)
                                .message(format!(
                                    "Workspace dependency '{}' declared in {} cannot be resolved in workspace or lockfile",
                                    dep_name,
                                    member.manifest_path.file_name().unwrap_or_default().to_string_lossy()
                                ))
                                .file(&member.manifest_path)
                                .explanation(
                                    "Workspace dependency references a local package that does not exist in the workspace.",
                                )
                                .remediation(format!(
                                    "Verify package '{}' exists in workspace packages or run '{} install'.",
                                    dep_name, node_info.package_manager
                                ))
                                .build(),
                        );
                    }
                } else if constraint.starts_with("file:") || constraint.starts_with("link:") {
                    let raw_path = constraint
                        .strip_prefix("file:")
                        .or_else(|| constraint.strip_prefix("link:"))
                        .unwrap_or(constraint)
                        .trim();
                    let target_path = member.root_dir.join(raw_path);
                    if !target_path.exists() {
                        diagnostics.push(
                            Diagnostic::builder("NDEP-004", Severity::Warning, LanguageTarget::Node)
                                .message(format!(
                                    "Local dependency '{}' path '{}' declared in {} cannot be resolved on disk",
                                    dep_name,
                                    raw_path,
                                    member.manifest_path.file_name().unwrap_or_default().to_string_lossy()
                                ))
                                .file(&member.manifest_path)
                                .explanation(
                                    "Local file or link dependency references a path that does not exist on disk.",
                                )
                                .remediation(format!(
                                    "Verify the relative path '{}' points to an existing directory or package.",
                                    raw_path
                                ))
                                .build(),
                        );
                    }
                }
            }
        }

        // Issue #15: Cross-Workspace Dependency Version Conflicts (NDEP-006)
        if !node_info.workspace_members.is_empty() {
            let mut cross_deps: BTreeMap<String, Vec<(&str, &Path, &str, bool)>> = BTreeMap::new();

            if !node_info.dependencies.is_empty() || !node_info.dev_dependencies.is_empty() {
                let root_name = "root";
                for (dep, constraint) in &node_info.dependencies {
                    if !node_info.workspace_packages.contains_key(dep) {
                        cross_deps.entry(dep.clone()).or_default().push((
                            root_name,
                            &node_info.package_json_path,
                            constraint,
                            false,
                        ));
                    }
                }
                for (dep, constraint) in &node_info.dev_dependencies {
                    if !node_info.workspace_packages.contains_key(dep) {
                        cross_deps.entry(dep.clone()).or_default().push((
                            root_name,
                            &node_info.package_json_path,
                            constraint,
                            false,
                        ));
                    }
                }
            }

            for member in &node_info.workspace_members {
                for (dep, constraint) in &member.dependencies {
                    if !node_info.workspace_packages.contains_key(dep) {
                        cross_deps.entry(dep.clone()).or_default().push((
                            &member.name,
                            &member.manifest_path,
                            constraint,
                            false,
                        ));
                    }
                }
                for (dep, constraint) in &member.dev_dependencies {
                    if !node_info.workspace_packages.contains_key(dep) {
                        cross_deps.entry(dep.clone()).or_default().push((
                            &member.name,
                            &member.manifest_path,
                            constraint,
                            false,
                        ));
                    }
                }
                for (dep, constraint) in &member.optional_dependencies {
                    if !node_info.workspace_packages.contains_key(dep) {
                        cross_deps.entry(dep.clone()).or_default().push((
                            &member.name,
                            &member.manifest_path,
                            constraint,
                            true,
                        ));
                    }
                }
            }

            for (pkg_name, decls) in cross_deps {
                let distinct_members: HashSet<&str> = decls.iter().map(|(m, _, _, _)| *m).collect();
                if distinct_members.len() < 2 {
                    continue;
                }

                let member_pairs: Vec<(&str, &str)> =
                    decls.iter().map(|(m, _, c, _)| (*m, *c)).collect();
                let compatibility = check_npm_constraints_compatibility(&member_pairs);

                if let ConstraintCompatibility::Incompatible { reason, .. } = compatibility {
                    let first_manifest = decls[0].1;
                    let member_summary = decls
                        .iter()
                        .map(|(m, _, c, opt)| {
                            if *opt {
                                format!("'{}' declares '{}' (optional)", m, c)
                            } else {
                                format!("'{}' declares '{}'", m, c)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ");

                    let breakdown_lines = decls
                        .iter()
                        .map(|(m, path, c, opt)| {
                            let rel = path
                                .strip_prefix(&ctx.root)
                                .unwrap_or(path)
                                .to_string_lossy()
                                .replace('\\', "/");
                            let opt_suffix = if *opt { " (optional)" } else { "" };
                            format!("- {} ({}): {}{}", m, rel, c, opt_suffix)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    diagnostics.push(
                        Diagnostic::builder("NDEP-006", Severity::Warning, LanguageTarget::Node)
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
                                "Align '{}' version constraints across all workspace members or configure overrides.",
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
