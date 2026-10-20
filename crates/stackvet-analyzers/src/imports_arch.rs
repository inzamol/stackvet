use anyhow::Result;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, LanguageTarget, ProjectContext, Severity,
};
use stackvet_graph::ImportGraph;
use stackvet_node::parse_node_imports;
use stackvet_python::parse_python_imports;
use stackvet_python::stdlib::is_pep594_removed;
use std::collections::HashSet;
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

        let mut all_source_files: HashSet<String> = HashSet::new();

        for file in &ctx.source_files {
            let rel_path = file
                .strip_prefix(&ctx.root)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            all_source_files.insert(rel_path.clone());
            graph.add_node(rel_path.clone());

            let ext = file.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext == "py" {
                if let Ok(content) = fs::read_to_string(file) {
                    let imports = parse_python_imports(&content, file);
                    for imp in imports {
                        // Check PEP 594: IMP-004
                        if is_pep594_removed(&imp.module) {
                            let loc =
                                make_import_location(file, imp.span.as_ref(), imp.line, &content);
                            diagnostics.push(
                                Diagnostic::builder("IMP-004", Severity::Error, LanguageTarget::Python)
                                    .message(format!(
                                        "Module '{}' was removed in Python 3.13 (PEP 594)",
                                        imp.module
                                    ))
                                    .location(loc)
                                    .explanation(format!(
                                        "PEP 594 completely removed deprecated standard library module '{}' from Python 3.13+.",
                                        imp.module
                                    ))
                                    .remediation("Migrate to a modern replacement library or backport.")
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
                                graph.add_import(rel_path.clone(), target_rel);
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
                                        imp.symbol.as_deref().unwrap_or("*")
                                    )
                                };
                                let loc = make_import_location(
                                    file,
                                    imp.span.as_ref(),
                                    imp.line,
                                    &content,
                                );
                                diagnostics.push(
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
                        }
                    }
                }
            } else if matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs") {
                if let Ok(content) = fs::read_to_string(file) {
                    let imports = parse_node_imports(&content, file);
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
                                graph.add_import(rel_path.clone(), target_rel);
                            } else {
                                let loc = make_import_location(
                                    file,
                                    imp.span.as_ref(),
                                    imp.line,
                                    &content,
                                );
                                diagnostics.push(
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
                                    .remediation("Check file extension, path casing, or tsconfig path aliases.")
                                    .build(),
                                );
                            }
                        } else if let Some(node_info) = &ctx.node {
                            if stackvet_node::is_tsconfig_alias(
                                &imp.specifier,
                                &node_info.tsconfig_paths,
                            ) {
                                if let Some(target_file) = stackvet_node::resolve_aliased_import(
                                    &ctx.root,
                                    &node_info.tsconfig_paths,
                                    node_info.tsconfig_base_url.as_deref(),
                                    &imp.specifier,
                                ) {
                                    let target_rel = target_file
                                        .strip_prefix(&ctx.root)
                                        .unwrap_or(&target_file)
                                        .to_string_lossy()
                                        .replace('\\', "/");
                                    graph.add_import(rel_path.clone(), target_rel);
                                } else {
                                    let loc = make_import_location(
                                        file,
                                        imp.span.as_ref(),
                                        imp.line,
                                        &content,
                                    );
                                    diagnostics.push(
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
                                        .remediation("Verify tsconfig.json paths mapping and ensure target file exists.")
                                        .build(),
                                    );
                                }
                            } else if let Some(target_file) = stackvet_node::resolve_aliased_import(
                                &ctx.root,
                                &node_info.tsconfig_paths,
                                node_info.tsconfig_base_url.as_deref(),
                                &imp.specifier,
                            ) {
                                let target_rel = target_file
                                    .strip_prefix(&ctx.root)
                                    .unwrap_or(&target_file)
                                    .to_string_lossy()
                                    .replace('\\', "/");
                                graph.add_import(rel_path.clone(), target_rel);
                            }
                        }
                    }
                }
            }
        }

        // IMP-001: Circular Imports via Tarjan's SCC
        let cycles = graph.find_cycles();
        for cycle in cycles {
            let cycle_path = cycle.join(" -> ");
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

        // IMP-003: Orphan File / Dead Code
        for file_str in &all_source_files {
            let is_entrypoint = file_str.ends_with("main.py")
                || file_str.ends_with("app.py")
                || file_str.ends_with("index.ts")
                || file_str.ends_with("index.tsx")
                || file_str.ends_with("index.js")
                || file_str.ends_with("App.tsx")
                || file_str.ends_with("page.tsx")
                || file_str.ends_with("layout.tsx")
                || file_str.ends_with("route.ts")
                || file_str.contains("/test")
                || file_str.contains("__init__.py")
                || file_str.contains(".config.")
                || file_str.contains("conftest.py");

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

        // ARC-001: Layer Violation (e.g. database importing api)
        for (from, to) in [
            ("db", "api"),
            ("models", "routes"),
            ("database", "controllers"),
        ] {
            for file_str in &all_source_files {
                if file_str.contains(&format!("/{}/", from)) {
                    // Check outgoing imports
                    for target_file in &all_source_files {
                        if target_file.contains(&format!("/{}/", to))
                            && graph.find_cycles().is_empty()
                        {
                            // If from imports target
                            // check if there's an edge
                        }
                    }
                }
            }
        }

        Ok(diagnostics)
    }
}
