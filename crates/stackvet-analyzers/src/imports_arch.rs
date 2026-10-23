use anyhow::Result;
use rayon::prelude::*;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, LanguageTarget, ProjectContext, Severity,
};
use stackvet_graph::ImportGraph;
use stackvet_node::parse_node_imports_with_diagnostics;
use stackvet_python::parse_python_imports_with_diagnostics;
use stackvet_python::stdlib::is_pep594_removed;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

fn make_import_location(
    file: &Path,
    span: Option<&stackvet_core::TextSpan>,
    line: usize,
    content: &str,
) -> DiagnosticLocation {
    if let Some(s) = span {
        let mut loc = DiagnosticLocation::new(file).with_span(*s);
        if let Some(snippet) = DiagnosticLocation::extract_snippet_from_source(content, s) {
            loc = loc.with_snippet(snippet);
        }
        loc
    } else {
        DiagnosticLocation::new(file).with_span(stackvet_core::TextSpan::single_line(line, 1, 1))
    }
}

pub struct ImportsArchAnalyzer;

impl ImportsArchAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Resolves local import to file on disk
    fn resolve_local_import(base_file: &Path, specifier: &str) -> Option<PathBuf> {
        let parent = base_file.parent()?;
        let target_base = parent.join(specifier);

        // Check exact match (e.g. ./foo.py or ./foo.ts)
        if target_base.is_file() {
            return Some(target_base);
        }

        let extensions = ["ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "json"];

        // 1. If target_base has a recognized runtime extension (e.g. .js, .jsx, .mjs, .cjs),
        // substitute with TypeScript / JavaScript extensions:
        // .mjs -> .mts, .mjs
        // .cjs -> .cts, .cjs
        // .jsx -> .tsx, .jsx
        // .js  -> .ts, .tsx, .js, .jsx
        if let Some(ext) = target_base.extension().and_then(|s| s.to_str()) {
            let candidate_subs: &[&str] = match ext {
                "mjs" => &["mts", "mjs"],
                "cjs" => &["cts", "cjs"],
                "jsx" => &["tsx", "jsx"],
                "js" => &["ts", "tsx", "js", "jsx"],
                _ => &[],
            };
            for target_ext in candidate_subs {
                let with_sub = target_base.with_extension(target_ext);
                if with_sub.is_file() {
                    return Some(with_sub);
                }
            }
        }

        // 2. Try extensions (preserving dotted basenames)
        let file_name = target_base
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        for ext in extensions {
            let with_ext = target_base.with_file_name(format!("{}.{}", file_name, ext));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }

        // Try directory index files
        if target_base.is_dir() {
            for ext in extensions {
                let index_file = target_base.join(format!("index.{}", ext));
                if index_file.is_file() {
                    return Some(index_file);
                }
            }
            let init_py = target_base.join("__init__.py");
            if init_py.is_file() {
                return Some(init_py);
            }
        }

        None
    }

    /// Resolves Python relative imports taking into account traversal level, submodule path, and package symbols
    fn resolve_python_relative_import(
        base_file: &Path,
        imp: &stackvet_python::PythonImport,
    ) -> Option<PathBuf> {
        let mut current_dir = base_file.parent()?;
        if imp.level > 1 {
            for _ in 1..imp.level {
                current_dir = current_dir.parent()?;
            }
        }

        // 1. Explicit submodule/subpackage path (e.g. from .models import User or from ..common.auth import verify)
        if !imp.module.is_empty() {
            let subpath = imp.module.replace('.', "/");
            let candidate_file = current_dir.join(format!("{}.py", subpath));
            if candidate_file.is_file() {
                return Some(candidate_file);
            }
            let candidate_dir = current_dir.join(&subpath);
            if let Some(sym) = &imp.symbol {
                let symbol_file = candidate_dir.join(format!("{}.py", sym));
                if symbol_file.is_file() {
                    return Some(symbol_file);
                }
            }
            let candidate_init = candidate_dir.join("__init__.py");
            if candidate_init.is_file() {
                return Some(candidate_init);
            }
            // PEP 420 namespace package (directory containing .py files)
            if candidate_dir.is_dir() {
                if let Ok(entries) = fs::read_dir(&candidate_dir) {
                    let mut py_files: Vec<PathBuf> = entries
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| {
                            p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("py")
                        })
                        .collect();
                    py_files.sort();
                    if let Some(init) = py_files
                        .iter()
                        .find(|p| p.file_name().and_then(|n| n.to_str()) == Some("__init__.py"))
                    {
                        return Some(init.clone());
                    }
                    if let Some(first) = py_files.into_iter().next() {
                        return Some(first);
                    }
                }
            }
        } else if let Some(sym) = &imp.symbol {
            // 2. Bare relative import (e.g. from . import helper or from .. import config)
            let candidate_file = current_dir.join(format!("{}.py", sym));
            if candidate_file.is_file() {
                return Some(candidate_file);
            }
            let candidate_dir = current_dir.join(sym);
            let candidate_init = candidate_dir.join("__init__.py");
            if candidate_init.is_file() {
                return Some(candidate_init);
            }
            if candidate_dir.is_dir() {
                if let Ok(entries) = fs::read_dir(&candidate_dir) {
                    let mut py_files: Vec<PathBuf> = entries
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| {
                            p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("py")
                        })
                        .collect();
                    py_files.sort();
                    if let Some(init) = py_files
                        .iter()
                        .find(|p| p.file_name().and_then(|n| n.to_str()) == Some("__init__.py"))
                    {
                        return Some(init.clone());
                    }
                    if let Some(first) = py_files.into_iter().next() {
                        return Some(first);
                    }
                }
            }
            // Check if current directory has __init__.py (symbol exported from current package)
            let pkg_init = current_dir.join("__init__.py");
            if pkg_init.is_file() {
                return Some(pkg_init);
            }
        }

        None
    }
}

impl Default for ImportsArchAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer for ImportsArchAnalyzer {
    fn name(&self) -> &'static str {
        "ImportsArchAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::General
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        let mut graph = ImportGraph::new();
        let mut edge_type_only: HashMap<(String, String), bool> = HashMap::new();
        let mut all_source_files: HashSet<String> = HashSet::new();

        // 1. Parallel AST parsing and local import resolution across Rayon thread pool
        let python_search_roots =
            stackvet_python::get_python_search_roots(&ctx.root, ctx.python.as_ref());

        let parsed_files: Vec<ParsedFileImports> = ctx
            .source_files
            .par_iter()
            .map(|file| {
                let rel_path = file
                    .strip_prefix(&ctx.root)
                    .unwrap_or(file)
                    .to_string_lossy()
                    .replace('\\', "/");

                let mut file_diags = Vec::new();
                let mut file_imports = Vec::new();

                let ext = file.extension().and_then(|s| s.to_str()).unwrap_or("");
                if ext == "py" {
                    match fs::read_to_string(file) {
                        Ok(content) => {
                            let (imports, syn_diags) =
                                parse_python_imports_with_diagnostics(&content, file);
                            file_diags.extend(syn_diags);
                            for imp in imports {
                                // Check PEP 594: IMP-004
                                if is_pep594_removed(&imp.module) {
                                    let loc = make_import_location(
                                        file,
                                        imp.span.as_ref(),
                                        imp.line,
                                        &content,
                                    );
                                    file_diags.push(
                                        Diagnostic::builder(
                                            "IMP-004",
                                            Severity::Error,
                                            LanguageTarget::Python,
                                        )
                                        .message(format!(
                                            "Module '{}' was removed in Python 3.13 (PEP 594)",
                                            imp.module
                                        ))
                                        .location(loc)
                                        .explanation(format!(
                                            "PEP 594 completely removed deprecated standard library module '{}' from Python 3.13+.",
                                            imp.module
                                        ))
                                        .remediation(
                                            "Migrate to a modern replacement library or backport.",
                                        )
                                        .build(),
                                    );
                                }

                                if imp.is_relative {
                                    if let Some(target_file) =
                                        Self::resolve_python_relative_import(file, &imp)
                                    {
                                        let target_rel = target_file
                                            .strip_prefix(&ctx.root)
                                            .unwrap_or(&target_file)
                                            .to_string_lossy()
                                            .replace('\\', "/");
                                        file_imports.push((rel_path.clone(), target_rel, false));
                                    } else {
                                        let dots = ".".repeat(imp.level);
                                        let import_desc = if imp.module.is_empty() {
                                            format!(
                                                "from {} import {}",
                                                dots,
                                                imp.symbol.as_deref().unwrap_or("?")
                                            )
                                        } else {
                                            format!(
                                                "from {}{} import {}",
                                                dots,
                                                imp.module,
                                                imp.symbol.as_deref().unwrap_or("?")
                                            )
                                        };
                                        let loc = make_import_location(
                                            file,
                                            imp.span.as_ref(),
                                            imp.line,
                                            &content,
                                        );
                                        file_diags.push(
                                            Diagnostic::builder(
                                                "IMP-002",
                                                Severity::Error,
                                                LanguageTarget::Python,
                                            )
                                            .message(format!(
                                                "Unresolved relative Python import '{}' in '{}'",
                                                import_desc, rel_path
                                            ))
                                            .location(loc)
                                            .explanation(
                                                "Relative import does not correspond to an existing Python file or package module.",
                                            )
                                            .remediation("Check the module path and verify file exists.")
                                            .build(),
                                        );
                                    }
                                } else {
                                    // Non-relative import: check if target is a local package/module in search roots
                                    let root_pkg = imp.module.split('.').next().unwrap_or(&imp.module);
                                    if !stackvet_python::stdlib::is_python_stdlib(root_pkg)
                                        && !is_pep594_removed(root_pkg)
                                        && stackvet_python::is_local_python_package(
                                            &python_search_roots,
                                            root_pkg,
                                        )
                                    {
                                        if let Some(target_file) =
                                            stackvet_python::resolve_python_absolute_import(
                                                &python_search_roots,
                                                &imp,
                                            )
                                        {
                                            let target_rel = target_file
                                                .strip_prefix(&ctx.root)
                                                .unwrap_or(&target_file)
                                                .to_string_lossy()
                                                .replace('\\', "/");
                                            file_imports.push((rel_path.clone(), target_rel, false));
                                        } else {
                                            let import_desc = if let Some(sym) = &imp.symbol {
                                                format!("from {} import {}", imp.module, sym)
                                            } else {
                                                format!("import {}", imp.module)
                                            };
                                            let loc = make_import_location(
                                                file,
                                                imp.span.as_ref(),
                                                imp.line,
                                                &content,
                                            );
                                            file_diags.push(
                                                Diagnostic::builder(
                                                    "IMP-002",
                                                    Severity::Error,
                                                    LanguageTarget::Python,
                                                )
                                                .message(format!(
                                                    "Unresolved local Python import '{}' in '{}'",
                                                    import_desc, rel_path
                                                ))
                                                .location(loc)
                                                .explanation(format!(
                                                    "Package '{}' is a local project module, but the imported path '{}' could not be resolved in candidate search roots ({:?}).",
                                                    root_pkg, imp.module, python_search_roots
                                                ))
                                                .remediation("Verify the module name, check PYTHONPATH / src layout, and ensure the target file exists.")
                                                .build(),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            file_diags.push(
                                Diagnostic::builder(
                                    "DISC-001",
                                    Severity::Warning,
                                    LanguageTarget::General,
                                )
                                .message(format!("Unreadable file '{}': {}", file.display(), e))
                                .location(DiagnosticLocation::new(file.to_path_buf()))
                                .explanation(
                                    "The source file could not be read during AST import traversal.",
                                )
                                .remediation("Check file read permissions.")
                                .build(),
                            );
                        }
                    }
                } else if matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs") {
                    match fs::read_to_string(file) {
                        Ok(content) => {
                            let (imports, syn_diags) =
                                parse_node_imports_with_diagnostics(&content, file);
                            file_diags.extend(syn_diags);
                            for imp in imports {
                                if imp.specifier.starts_with('.') {
                                    if let Some(target_file) =
                                        Self::resolve_local_import(file, &imp.specifier)
                                    {
                                        let target_rel = target_file
                                            .strip_prefix(&ctx.root)
                                            .unwrap_or(&target_file)
                                            .to_string_lossy()
                                            .replace('\\', "/");
                                        file_imports.push((
                                            rel_path.clone(),
                                            target_rel,
                                            imp.is_type_only,
                                        ));
                                    } else {
                                        let loc = make_import_location(
                                            file,
                                            imp.span.as_ref(),
                                            imp.line,
                                            &content,
                                        );
                                        file_diags.push(
                                            Diagnostic::builder(
                                                "IMP-002",
                                                Severity::Error,
                                                LanguageTarget::Node,
                                            )
                                            .message(format!(
                                                "Unresolved local import '{}' in '{}'",
                                                imp.specifier, rel_path
                                            ))
                                            .location(loc)
                                            .explanation(
                                                "Specifier could not be resolved to any file or directory index on disk.",
                                            )
                                            .remediation(
                                                "Check file extension, path casing, or tsconfig path aliases.",
                                            )
                                            .build(),
                                        );
                                    }
                                } else if imp.specifier.starts_with('#') {
                                    // Package subpath import
                                    let resolved = if let Some(node_info) = &ctx.node {
                                        stackvet_node::resolve_package_subpath_import(
                                            &ctx.root,
                                            &node_info.package_imports,
                                            &imp.specifier,
                                        )
                                    } else {
                                        None
                                    };

                                    if let Some(target_file) = resolved {
                                        let target_rel = target_file
                                            .strip_prefix(&ctx.root)
                                            .unwrap_or(&target_file)
                                            .to_string_lossy()
                                            .replace('\\', "/");
                                        file_imports.push((
                                            rel_path.clone(),
                                            target_rel,
                                            imp.is_type_only,
                                        ));
                                    } else {
                                        let loc = make_import_location(
                                            file,
                                            imp.span.as_ref(),
                                            imp.line,
                                            &content,
                                        );
                                        file_diags.push(
                                            Diagnostic::builder(
                                                "IMP-002",
                                                Severity::Error,
                                                LanguageTarget::Node,
                                            )
                                            .message(format!(
                                                "Unresolved package subpath import '{}' in '{}'",
                                                imp.specifier, rel_path
                                            ))
                                            .location(loc)
                                            .explanation(
                                                "Subpath import specifier matching '#*' could not be resolved to any file on disk via package.json 'imports'.",
                                            )
                                            .remediation(
                                                "Verify package.json 'imports' mapping and ensure the destination file exists.",
                                            )
                                            .build(),
                                        );
                                    }
                                } else if let Some(node_info) = &ctx.node {
                                    if stackvet_node::is_tsconfig_alias(
                                        &imp.specifier,
                                        &node_info.tsconfig_paths,
                                    ) {
                                        if let Some(target_file) =
                                            stackvet_node::resolve_aliased_import(
                                                &ctx.root,
                                                &node_info.tsconfig_paths,
                                                node_info.tsconfig_base_url.as_deref(),
                                                &imp.specifier,
                                            )
                                        {
                                            let target_rel = target_file
                                                .strip_prefix(&ctx.root)
                                                .unwrap_or(&target_file)
                                                .to_string_lossy()
                                                .replace('\\', "/");
                                            file_imports.push((
                                                rel_path.clone(),
                                                target_rel,
                                                imp.is_type_only,
                                            ));
                                        } else {
                                            let loc = make_import_location(
                                                file,
                                                imp.span.as_ref(),
                                                imp.line,
                                                &content,
                                            );
                                            file_diags.push(
                                                Diagnostic::builder(
                                                    "IMP-002",
                                                    Severity::Error,
                                                    LanguageTarget::Node,
                                                )
                                                .message(format!(
                                                    "Unresolved TypeScript path alias '{}' in '{}'",
                                                    imp.specifier, rel_path
                                                ))
                                                .location(loc)
                                                .explanation(
                                                    "Path alias matched tsconfig.json paths but target file could not be found on disk.",
                                                )
                                                .remediation(
                                                    "Verify tsconfig.json paths mapping and ensure target file exists.",
                                                )
                                                .build(),
                                            );
                                        }
                                    } else if let Some(target_file) =
                                        stackvet_node::resolve_aliased_import(
                                            &ctx.root,
                                            &node_info.tsconfig_paths,
                                            node_info.tsconfig_base_url.as_deref(),
                                            &imp.specifier,
                                        )
                                    {
                                        let target_rel = target_file
                                            .strip_prefix(&ctx.root)
                                            .unwrap_or(&target_file)
                                            .to_string_lossy()
                                            .replace('\\', "/");
                                        file_imports.push((
                                            rel_path.clone(),
                                            target_rel,
                                            imp.is_type_only,
                                        ));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            file_diags.push(
                                Diagnostic::builder(
                                    "DISC-001",
                                    Severity::Warning,
                                    LanguageTarget::General,
                                )
                                .message(format!("Unreadable file '{}': {}", file.display(), e))
                                .location(DiagnosticLocation::new(file.to_path_buf()))
                                .explanation(
                                    "The source file could not be read during AST import traversal.",
                                )
                                .remediation("Check file read permissions.")
                                .build(),
                            );
                        }
                    }
                }

                ParsedFileImports {
                    rel_path,
                    diagnostics: file_diags,
                    imports: file_imports,
                }
            })
            .collect();

        // 2. Aggregate parsed files into graph and edge metadata on main thread
        for pf in parsed_files {
            all_source_files.insert(pf.rel_path.clone());
            graph.add_node(pf.rel_path.clone());
            diagnostics.extend(pf.diagnostics);
            for (from, to, is_type) in pf.imports {
                let key = (from, to);
                match edge_type_only.entry(key) {
                    std::collections::hash_map::Entry::Occupied(mut occ) => {
                        *occ.get_mut() = *occ.get() && is_type;
                    }
                    std::collections::hash_map::Entry::Vacant(vac) => {
                        let (f, t) = vac.key().clone();
                        graph.add_import(f, t);
                        vac.insert(is_type);
                    }
                }
            }
        }

        // 3. IMP-001: Circular Imports via Tarjan's SCC with type-only cycle differentiation
        // Build runtime-only graph to determine which nodes participate in runtime cycles
        let mut runtime_graph = ImportGraph::new();
        for ((from, to), &is_type) in &edge_type_only {
            if !is_type {
                runtime_graph.add_import(from.clone(), to.clone());
            }
        }
        let runtime_cycles = runtime_graph.find_cycles();
        let mut runtime_cycle_nodes: HashSet<&str> = HashSet::new();
        for rc in &runtime_cycles {
            for node in rc {
                runtime_cycle_nodes.insert(node.as_str());
            }
        }

        let cycles = graph.find_cycles();
        for cycle in cycles {
            let cycle_path = cycle.join(" -> ");
            let has_runtime_cycle = cycle
                .iter()
                .any(|node| runtime_cycle_nodes.contains(node.as_str()));

            if !has_runtime_cycle {
                // Type-only cycles are completely erased at compile-time and cannot cause runtime failures.
                let mut builder = Diagnostic::builder("IMP-001", Severity::Info, LanguageTarget::General)
                    .message(format!("Type-only circular dependency detected: {}", cycle_path))
                    .file(Path::new(&cycle[0]))
                    .explanation(format!(
                        "Cycles involving type-only imports ({}) are erased during compilation and do not cause runtime crashes, but may indicate architectural coupling.",
                        cycle_path
                    ))
                    .remediation("Ensure all cyclical imports use 'import type' or refactor common types into a shared types module.");

                for step in cycle.iter().skip(1) {
                    builder = builder.related_location(
                        DiagnosticLocation::new(Path::new(step))
                            .with_message("Part of type-only circular dependency"),
                    );
                }
                diagnostics.push(builder.build());
            } else {
                let mut builder = Diagnostic::builder("IMP-001", Severity::Error, LanguageTarget::General)
                    .message(format!("Circular import cycle detected: {}", cycle_path))
                    .file(Path::new(&cycle[0]))
                    .explanation(format!(
                        "Cycles between modules ({}) cause runtime import failures, initialization deadlock, or undefined symbols.",
                        cycle_path
                    ))
                    .remediation("Refactor shared interfaces into a separate common module or use dependency injection.");

                for step in cycle.iter().skip(1) {
                    builder = builder.related_location(
                        DiagnosticLocation::new(Path::new(step))
                            .with_message("Part of circular import cycle"),
                    );
                }
                diagnostics.push(builder.build());
            }
        }

        // 4. Collect manifest-declared entry points across Node and Python projects
        let mut manifest_entrypoints = HashSet::new();
        if let Some(ref node_info) = ctx.node {
            for ep in &node_info.entry_points {
                manifest_entrypoints.insert(ep.clone());
            }
        }
        if let Some(ref py_info) = ctx.python {
            for ep in &py_info.entry_points {
                manifest_entrypoints.insert(ep.clone());
            }
        }

        // 5. IMP-003: Orphan File / Dead Code (only applicable when analyzing multi-file project)
        if all_source_files.len() > 1 {
            for file_str in &all_source_files {
                let norm = file_str.replace('\\', "/");
                let file_name = Path::new(file_str)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                let is_test_file = norm.starts_with("test/")
                    || norm.starts_with("tests/")
                    || norm.starts_with("__tests__/")
                    || norm.contains("/test/")
                    || norm.contains("/tests/")
                    || norm.contains("/__tests__/")
                    || norm.contains(".test.")
                    || norm.contains(".spec.")
                    || file_name.starts_with("test_")
                    || file_name.ends_with("_test.py")
                    || file_name == "conftest.py";

                let is_script_file = norm.starts_with("scripts/")
                    || norm.contains("/scripts/")
                    || norm.starts_with("bin/")
                    || norm.contains("/bin/")
                    || norm.starts_with("tasks/")
                    || norm.contains("/tasks/");

                let is_entrypoint = is_test_file
                    || is_script_file
                    || is_manifest_entrypoint(&norm, file_name, &manifest_entrypoints)
                    || file_name == "main.py"
                    || file_name == "main.ts"
                    || file_name == "main.js"
                    || file_name == "main.tsx"
                    || file_name == "main.jsx"
                    || file_name == "app.py"
                    || file_name == "app.ts"
                    || file_name == "app.js"
                    || file_name == "app.tsx"
                    || file_name == "app.jsx"
                    || file_name == "extension.ts"
                    || file_name == "extension.js"
                    || file_name == "extension.tsx"
                    || file_name == "extension.jsx"
                    || file_name == "server.ts"
                    || file_name == "server.js"
                    || file_name == "server.py"
                    || file_name == "cli.ts"
                    || file_name == "cli.js"
                    || file_name == "cli.py"
                    || file_name == "index.ts"
                    || file_name == "index.tsx"
                    || file_name == "index.js"
                    || file_name == "index.jsx"
                    || file_name == "App.tsx"
                    || file_name == "App.jsx"
                    || file_name == "page.tsx"
                    || file_name == "page.jsx"
                    || file_name == "layout.tsx"
                    || file_name == "layout.jsx"
                    || file_name == "route.ts"
                    || file_name == "route.js"
                    || file_name == "__init__.py"
                    || norm.contains(".config.");

                if !is_entrypoint && graph.incoming_count(file_str) == 0 {
                    diagnostics.push(
                        Diagnostic::builder("IMP-003", Severity::Warning, LanguageTarget::General)
                            .message(format!("Orphan source file with 0 incoming imports: '{}'", file_str))
                            .file(Path::new(file_str))
                            .explanation(
                                "File is never imported by any other source files in the project. It may be dead code or an unexported utility.",
                            )
                            .remediation("Import the module or delete it if obsolete.")
                            .build(),
                    );
                }
            }
        }

        // 6. ARC-001: Architecture Boundary & Layer Rules (Configurable or Fallback Defaults)
        let edges = graph.edges();
        let mut checked_custom = false;

        let matches_path_segment = |path: &str, pat: &str| -> bool {
            let trimmed = pat.trim_matches('/');
            if trimmed.is_empty() {
                return false;
            }
            path == trimmed
                || path.starts_with(&format!("{}/", trimmed))
                || path.ends_with(&format!("/{}", trimmed))
                || path.contains(&format!("/{}/", trimmed))
        };

        if let Some(ref arch_cfg) = ctx.architecture {
            // A. Explicit forbidden dependency rules
            for rule in &arch_cfg.forbidden {
                checked_custom = true;
                let from_pat = rule.from.replace('\\', "/");
                let to_pat = rule.cannot_import.replace('\\', "/");

                for (from_file, to_file) in edges {
                    let from_norm = from_file.replace('\\', "/");
                    let to_norm = to_file.replace('\\', "/");

                    let matches_from = matches_path_segment(&from_norm, &from_pat);
                    let matches_to = matches_path_segment(&to_norm, &to_pat);

                    if matches_from && matches_to {
                        let custom_msg = rule.message.clone().unwrap_or_else(|| {
                            format!(
                                "Architecture boundary violation: module '{}' is forbidden from importing '{}' (rule: '{}' cannot import '{}')",
                                from_file, to_file, rule.from, rule.cannot_import
                            )
                        });
                        diagnostics.push(
                            Diagnostic::builder("ARC-001", Severity::Warning, LanguageTarget::General)
                                .message(custom_msg)
                                .file(Path::new(from_file))
                                .explanation(format!(
                                    "Project architecture configuration forbids '{}' components from depending on '{}' components.",
                                    rule.from, rule.cannot_import
                                ))
                                .remediation("Invert the dependency or decouple components via interfaces.")
                                .build(),
                        );
                    }
                }
            }

            // B. Ordered layer hierarchy rules (upper layers in arch_cfg.layers must not be imported by lower layers)
            if !arch_cfg.layers.is_empty() {
                checked_custom = true;
                let layers = &arch_cfg.layers;
                for (i, upper_layer_raw) in layers.iter().enumerate() {
                    let upper_layer = upper_layer_raw.replace('\\', "/");
                    for lower_layer_raw in layers.iter().skip(i + 1) {
                        let lower_layer = lower_layer_raw.replace('\\', "/");

                        for (from_file, to_file) in edges {
                            let from_norm = from_file.replace('\\', "/");
                            let to_norm = to_file.replace('\\', "/");

                            let from_has_lower = matches_path_segment(&from_norm, &lower_layer);
                            let to_has_upper = matches_path_segment(&to_norm, &upper_layer);

                            if from_has_lower && to_has_upper {
                                diagnostics.push(
                                    Diagnostic::builder("ARC-001", Severity::Warning, LanguageTarget::General)
                                        .message(format!(
                                            "Architecture layer violation: lower layer module '{}' ({}) imports upper layer module '{}' ({})",
                                            from_file, lower_layer, to_file, upper_layer
                                        ))
                                        .file(Path::new(from_file))
                                        .explanation(format!(
                                            "Configured layer hierarchy stipulates that lower-level components (e.g. '{}') must not depend on higher-level layers (e.g. '{}').",
                                            lower_layer, upper_layer
                                        ))
                                        .remediation("Route access through intermediate service/repository layers or invert the dependency using interfaces.")
                                        .build(),
                                );
                            }
                        }
                    }
                }
            }
        }

        // C. Fallback to default 6 layer pairs if no custom rules matched
        if !checked_custom {
            for (lower_layer, upper_layer) in [
                ("db", "api"),
                ("models", "routes"),
                ("database", "controllers"),
                ("services", "controllers"),
                ("repositories", "views"),
                ("domain", "presentation"),
            ] {
                for (from_file, to_file) in edges {
                    let from_norm = from_file.replace('\\', "/");
                    let to_norm = to_file.replace('\\', "/");
                    let from_has_lower = matches_path_segment(&from_norm, lower_layer);
                    let to_has_upper = matches_path_segment(&to_norm, upper_layer);

                    if from_has_lower && to_has_upper {
                        diagnostics.push(
                            Diagnostic::builder("ARC-001", Severity::Warning, LanguageTarget::General)
                                .message(format!(
                                    "Architecture layer violation: lower layer module '{}' ({}) imports upper layer module '{}' ({})",
                                    from_file, lower_layer, to_file, upper_layer
                                ))
                                .file(Path::new(from_file))
                                .explanation(format!(
                                    "Lower layer components (e.g. '{}') must not depend on higher-level presentation/API layers (e.g. '{}'). This inverts the architectural hierarchy and creates tight coupling.",
                                    lower_layer, upper_layer
                                ))
                                .remediation("Route access through intermediate service/repository layers or invert the dependency using interfaces.")
                                .build(),
                        );
                    }
                }
            }
        }

        Ok(diagnostics)
    }
}

struct ParsedFileImports {
    rel_path: String,
    diagnostics: Vec<Diagnostic>,
    imports: Vec<(String, String, bool)>,
}

fn is_manifest_entrypoint(
    norm_file: &str,
    file_name: &str,
    manifest_entrypoints: &HashSet<String>,
) -> bool {
    let file_stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    for entry in manifest_entrypoints {
        let norm_entry = entry.trim().trim_start_matches("./").replace('\\', "/");
        if norm_entry.is_empty() {
            continue;
        }

        // 1. Direct equality (e.g. "src/index.ts" == "src/index.ts")
        if norm_file == norm_entry {
            return true;
        }

        // 2. Python module specifier (e.g. "my_app.cli" -> "my_app/cli" or "cli")
        let module_subpath = norm_entry.replace('.', "/");
        if norm_file.ends_with(&format!("{}.py", module_subpath))
            || norm_file.ends_with(&format!("{}/__init__.py", module_subpath))
        {
            return true;
        }
        if !file_stem.is_empty()
            && norm_file.ends_with(&format!("{}.py", file_stem))
            && (module_subpath == file_stem || module_subpath.ends_with(&format!("/{}", file_stem)))
        {
            return true;
        }

        // 3. Build output directory strip (e.g. "out/src/extension.js", "dist/index.js", "build/index.js", "lib/main.js")
        let mut stripped = norm_entry.as_str();
        for build_prefix in &[
            "out/", "dist/", "build/", "lib/", "target/", "bundle/", "bin/",
        ] {
            if let Some(rest) = stripped.strip_prefix(build_prefix) {
                stripped = rest;
                break;
            }
        }

        if norm_file == stripped {
            return true;
        }

        // If manifest points to build output (e.g. "dist/index.js" -> stripped is "index.js"),
        // match with "src/index.ts", "src/index.js", etc.
        if norm_file.strip_prefix("src/").unwrap_or(norm_file) == stripped {
            return true;
        }

        // Cross-extension matching (.js -> .ts/.tsx, .mjs -> .mts, etc.)
        let stripped_stem = Path::new(stripped)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let entry_dir = Path::new(stripped)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .replace('\\', "/");

        let file_dir = Path::new(norm_file)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .replace('\\', "/");

        if stripped_stem == file_stem {
            // Same directory or inside src/
            if entry_dir == file_dir
                || file_dir.strip_prefix("src/").unwrap_or(&file_dir) == entry_dir
                || entry_dir.is_empty()
            {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arc_001_layer_violation() {
        let temp_dir = std::env::temp_dir().join(format!(
            "stackvet_test_arc_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = &temp_dir;

        let db_dir = root.join("src").join("db");
        let api_dir = root.join("src").join("api");
        fs::create_dir_all(&db_dir).unwrap();
        fs::create_dir_all(&api_dir).unwrap();

        let api_file = api_dir.join("routes.py");
        let db_file = db_dir.join("models.py");

        fs::write(&api_file, "def get_users(): pass\n").unwrap();
        // Lower layer (db) imports higher layer (api) -> violation!
        fs::write(&db_file, "from ..api import routes\n").unwrap();

        let mut ctx = ProjectContext::new(root.to_path_buf());
        ctx.source_files = vec![api_file.clone(), db_file.clone()];

        let analyzer = ImportsArchAnalyzer;
        let diags = analyzer.analyze(&ctx).unwrap();
        let arc001 = diags.iter().find(|d| d.rule_id.as_str() == "ARC-001");
        assert!(
            arc001.is_some(),
            "Expected ARC-001 diagnostic for db -> api layer violation"
        );
        let diag = arc001.unwrap();
        assert_eq!(diag.severity, Severity::Warning);
        assert!(diag.message.contains("Architecture layer violation"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_manifest_driven_entrypoint_node() {
        let temp_dir = std::env::temp_dir().join(format!(
            "stackvet_test_node_ep_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(temp_dir.join("src")).unwrap();

        let custom_runner = temp_dir.join("src").join("custom_runner.ts");
        let widget = temp_dir.join("src").join("widget.tsx");
        let orphan = temp_dir.join("src").join("unreferenced.ts");

        fs::write(&custom_runner, "export function run() {}\n").unwrap();
        fs::write(&widget, "export function Widget() {}\n").unwrap();
        fs::write(&orphan, "export const x = 42;\n").unwrap();

        let pkg_content = r#"{
            "name": "my-pkg",
            "main": "./dist/custom_runner.js",
            "exports": {
                "./widget": "./dist/widget.js"
            }
        }"#;

        let node_info =
            stackvet_node::parse_package_json(pkg_content, &temp_dir.join("package.json")).unwrap();
        assert!(node_info
            .entry_points
            .contains(&"./dist/custom_runner.js".to_string()));
        assert!(node_info
            .entry_points
            .contains(&"./dist/widget.js".to_string()));

        let mut ctx = ProjectContext::new(temp_dir.clone());
        ctx.source_files = vec![custom_runner.clone(), widget.clone(), orphan.clone()];
        ctx.node = Some(node_info);

        let analyzer = ImportsArchAnalyzer;
        let diags = analyzer.analyze(&ctx).unwrap();

        let imp003_files: Vec<String> = diags
            .iter()
            .filter(|d| d.rule_id.as_str() == "IMP-003")
            .map(|d| d.location.file_path.to_string_lossy().replace('\\', "/"))
            .collect();

        // unreferenced.ts SHOULD be flagged as orphan
        assert!(
            imp003_files.iter().any(|f| f.ends_with("unreferenced.ts")),
            "Expected unreferenced.ts to be flagged as orphan, got: {:?}",
            imp003_files
        );

        // custom_runner.ts and widget.tsx are manifest entry points and MUST NOT be flagged
        assert!(
            !imp003_files.iter().any(|f| f.ends_with("custom_runner.ts")),
            "custom_runner.ts declared in package.json main should NOT be flagged as orphan"
        );
        assert!(
            !imp003_files.iter().any(|f| f.ends_with("widget.tsx")),
            "widget.tsx declared in package.json exports should NOT be flagged as orphan"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_manifest_driven_entrypoint_python() {
        let temp_dir = std::env::temp_dir().join(format!(
            "stackvet_test_py_ep_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let pkg_dir = temp_dir.join("mypkg");
        fs::create_dir_all(&pkg_dir).unwrap();

        let cli_file = pkg_dir.join("cli.py");
        let orphan_file = pkg_dir.join("forgotten.py");

        fs::write(&cli_file, "def main(): pass\n").unwrap();
        fs::write(&orphan_file, "value = 100\n").unwrap();

        let pyproject_content = r#"
        [project]
        name = "mypkg"
        version = "0.1.0"

        [project.scripts]
        my-cli = "mypkg.cli:main"
        "#;

        let py_info = stackvet_python::parse_pyproject_toml(
            pyproject_content,
            &temp_dir.join("pyproject.toml"),
        )
        .unwrap();
        assert!(py_info.entry_points.contains(&"mypkg.cli:main".to_string()));
        assert!(py_info.entry_points.contains(&"mypkg.cli".to_string()));

        let mut ctx = ProjectContext::new(temp_dir.clone());
        ctx.source_files = vec![cli_file.clone(), orphan_file.clone()];
        ctx.python = Some(py_info);

        let analyzer = ImportsArchAnalyzer;
        let diags = analyzer.analyze(&ctx).unwrap();

        let imp003_files: Vec<String> = diags
            .iter()
            .filter(|d| d.rule_id.as_str() == "IMP-003")
            .map(|d| d.location.file_path.to_string_lossy().replace('\\', "/"))
            .collect();

        assert!(
            imp003_files.iter().any(|f| f.ends_with("forgotten.py")),
            "Expected forgotten.py to be flagged as orphan, got: {:?}",
            imp003_files
        );

        assert!(
            !imp003_files.iter().any(|f| f.ends_with("cli.py")),
            "cli.py declared in [project.scripts] should NOT be flagged as orphan"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_configurable_architecture_forbidden_rules() {
        let temp_dir = std::env::temp_dir().join(format!(
            "stackvet_test_arch_forbidden_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let core_dir = temp_dir.join("src").join("core");
        let web_dir = temp_dir.join("src").join("web");
        fs::create_dir_all(&core_dir).unwrap();
        fs::create_dir_all(&web_dir).unwrap();

        let core_file = core_dir.join("engine.py");
        let web_file = web_dir.join("handler.py");

        fs::write(&web_file, "def handle(): pass\n").unwrap();
        // core imports web (which is explicitly forbidden by config!)
        fs::write(&core_file, "from ..web import handler\n").unwrap();

        let mut ctx = ProjectContext::new(temp_dir.clone());
        ctx.source_files = vec![core_file.clone(), web_file.clone()];

        use stackvet_core::{ArchitectureConfig, ForbiddenDependencyRule};
        ctx.architecture = Some(ArchitectureConfig {
            layers: vec![],
            forbidden: vec![ForbiddenDependencyRule {
                from: "core".to_string(),
                cannot_import: "web".to_string(),
                message: Some("Core must never depend on web layer".to_string()),
            }],
        });

        let analyzer = ImportsArchAnalyzer;
        let diags = analyzer.analyze(&ctx).unwrap();
        let arc001 = diags.iter().find(|d| d.rule_id.as_str() == "ARC-001");
        assert!(
            arc001.is_some(),
            "Expected custom ARC-001 forbidden rule violation"
        );
        assert!(arc001
            .unwrap()
            .message
            .contains("Core must never depend on web layer"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_configurable_architecture_ordered_layers() {
        let temp_dir = std::env::temp_dir().join(format!(
            "stackvet_test_arch_layers_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let domain_dir = temp_dir.join("src").join("domain");
        let app_dir = temp_dir.join("src").join("application");
        fs::create_dir_all(&domain_dir).unwrap();
        fs::create_dir_all(&app_dir).unwrap();

        let domain_file = domain_dir.join("entity.py");
        let app_file = app_dir.join("service.py");

        fs::write(&app_file, "def run(): pass\n").unwrap();
        // domain (lower layer in Clean Arch) imports application (upper layer)
        fs::write(&domain_file, "from ..application import service\n").unwrap();

        let mut ctx = ProjectContext::new(temp_dir.clone());
        ctx.source_files = vec![domain_file.clone(), app_file.clone()];

        use stackvet_core::ArchitectureConfig;
        ctx.architecture = Some(ArchitectureConfig {
            layers: vec!["application".to_string(), "domain".to_string()],
            forbidden: vec![],
        });

        let analyzer = ImportsArchAnalyzer;
        let diags = analyzer.analyze(&ctx).unwrap();
        let arc001 = diags.iter().find(|d| d.rule_id.as_str() == "ARC-001");
        assert!(
            arc001.is_some(),
            "Expected ARC-001 for domain -> application layer violation"
        );
        assert!(arc001
            .unwrap()
            .message
            .contains("imports upper layer module"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_type_only_circular_dependency_is_info() {
        let temp_dir = std::env::temp_dir().join(format!(
            "stackvet_test_type_cycle_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let src_dir = temp_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let user_file = src_dir.join("user.ts");
        let post_file = src_dir.join("post.ts");

        // user.ts imports post.ts via value import
        fs::write(
            &user_file,
            "import { Post } from './post'; export class User {}\n",
        )
        .unwrap();
        // post.ts imports user.ts via type-only import
        fs::write(
            &post_file,
            "import type { User } from './user'; export class Post {}\n",
        )
        .unwrap();

        let mut ctx = ProjectContext::new(temp_dir.clone());
        ctx.source_files = vec![user_file.clone(), post_file.clone()];

        let analyzer = ImportsArchAnalyzer;
        let diags = analyzer.analyze(&ctx).unwrap();

        let cycle_diag = diags.iter().find(|d| d.rule_id.as_str() == "IMP-001");
        assert!(
            cycle_diag.is_some(),
            "Expected IMP-001 for cycle between user.ts and post.ts"
        );
        let diag = cycle_diag.unwrap();
        assert_eq!(
            diag.severity,
            Severity::Info,
            "Type-only cycles must have Severity::Info"
        );
        assert!(diag
            .message
            .contains("Type-only circular dependency detected"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_package_subpath_import_resolution_and_imp002() {
        let temp_dir = std::env::temp_dir().join(format!(
            "stackvet_test_subpath_imp_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let src_utils = temp_dir.join("src").join("utils");
        fs::create_dir_all(&src_utils).unwrap();

        let math_file = src_utils.join("math.ts");
        fs::write(
            &math_file,
            "export const add = (a: number, b: number) => a + b;\n",
        )
        .unwrap();

        let index_file = temp_dir.join("src").join("index.ts");
        fs::write(
            &index_file,
            "import { add } from '#utils/math';\nimport { missing } from '#utils/missing';\n",
        )
        .unwrap();

        let mut package_imports = std::collections::HashMap::new();
        package_imports.insert("#utils/*".to_string(), vec!["./src/utils/*.ts".to_string()]);
        let node_info = stackvet_core::NodeProjectInfo {
            package_imports,
            ..Default::default()
        };

        let mut ctx = ProjectContext::new(temp_dir.clone());
        ctx.node = Some(node_info);
        ctx.source_files = vec![index_file.clone(), math_file.clone()];

        let analyzer = ImportsArchAnalyzer;
        let diags = analyzer.analyze(&ctx).unwrap();

        // 1. Valid subpath import #utils/math should resolve and not trigger IMP-002
        let math_imp002 = diags
            .iter()
            .find(|d| d.rule_id.as_str() == "IMP-002" && d.message.contains("#utils/math"));
        assert!(
            math_imp002.is_none(),
            "Valid package subpath import '#utils/math' must resolve cleanly"
        );

        // 2. Invalid subpath import #utils/missing should trigger IMP-002
        let missing_imp002 = diags
            .iter()
            .find(|d| d.rule_id.as_str() == "IMP-002" && d.message.contains("#utils/missing"));
        assert!(
            missing_imp002.is_some(),
            "Expected IMP-002 for unresolved subpath import '#utils/missing'"
        );
        assert!(missing_imp002
            .unwrap()
            .message
            .contains("Unresolved package subpath import '#utils/missing'"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_python_src_layout_resolution_and_imp002() {
        let temp_dir = std::env::temp_dir().join(format!(
            "stackvet_test_py_src_layout_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let pkg_dir = temp_dir.join("src").join("my_app");
        fs::create_dir_all(&pkg_dir).unwrap();

        let init_file = pkg_dir.join("__init__.py");
        fs::write(&init_file, "").unwrap();

        let models_file = pkg_dir.join("models.py");
        fs::write(&models_file, "class User:\n    pass\n").unwrap();

        let main_file = pkg_dir.join("main.py");
        fs::write(
            &main_file,
            "from my_app.models import User\nfrom my_app.ghost import Spooky\n",
        )
        .unwrap();

        let py_info = stackvet_core::PythonProjectInfo {
            pythonpath: vec!["src".to_string()],
            ..Default::default()
        };

        let mut ctx = ProjectContext::new(temp_dir.clone());
        ctx.python = Some(py_info);
        ctx.source_files = vec![init_file.clone(), models_file.clone(), main_file.clone()];

        let analyzer = ImportsArchAnalyzer;
        let diags = analyzer.analyze(&ctx).unwrap();

        // 1. Valid absolute import from my_app.models must resolve without IMP-002
        let models_imp002 = diags
            .iter()
            .find(|d| d.rule_id.as_str() == "IMP-002" && d.message.contains("my_app.models"));
        assert!(
            models_imp002.is_none(),
            "Valid absolute import from my_app.models must resolve without IMP-002"
        );

        // 2. Unresolved submodule my_app.ghost must trigger IMP-002
        let ghost_imp002 = diags
            .iter()
            .find(|d| d.rule_id.as_str() == "IMP-002" && d.message.contains("my_app.ghost"));
        assert!(
            ghost_imp002.is_some(),
            "Expected IMP-002 for non-existent local submodule 'my_app.ghost'"
        );
        assert!(ghost_imp002
            .unwrap()
            .message
            .contains("Unresolved local Python import"));

        // 3. models.py has incoming import from main.py, so it must NOT be flagged as orphan (IMP-003)
        let orphan_models = diags.iter().find(|d| {
            d.rule_id.as_str() == "IMP-003"
                && d.location.file_path.to_string_lossy().contains("models.py")
        });
        assert!(
            orphan_models.is_none(),
            "models.py must have incoming import from main.py and not be flagged as orphan"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
