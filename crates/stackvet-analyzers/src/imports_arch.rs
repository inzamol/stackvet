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

        // Try extensions
        let extensions = ["ts", "tsx", "js", "jsx", "py", "json"];
        for ext in extensions {
            let with_ext = target_base.with_extension(ext);
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
                            diagnostics.push(
                                Diagnostic::builder("IMP-004", Severity::Error, LanguageTarget::Python)
                                    .message(format!(
                                        "Module '{}' was removed in Python 3.13 (PEP 594)",
                                        imp.module
                                    ))
                                    .location(
                                        DiagnosticLocation::new(file.clone())
                                            .with_span(stackvet_core::TextSpan::single_line(imp.line, 1, 1)),
                                    )
                                    .explanation(format!(
                                        "PEP 594 completely removed deprecated standard library module '{}' from Python 3.13+.",
                                        imp.module
                                    ))
                                    .remediation("Migrate to a modern replacement library or backport.")
                                    .build(),
                            );
                        }

                        if imp.is_relative {
                            // Check local relative import resolution: IMP-002
                            let spec = format!("./{}", imp.module.replace('.', "/"));
                            if let Some(target_file) = Self::resolve_local_import(file, &spec) {
                                let target_rel = target_file
                                    .strip_prefix(&ctx.root)
                                    .unwrap_or(&target_file)
                                    .to_string_lossy()
                                    .replace('\\', "/");
                                graph.add_import(rel_path.clone(), target_rel);
                            } else {
                                diagnostics.push(
                                    Diagnostic::builder(
                                        "IMP-002",
                                        Severity::Error,
                                        LanguageTarget::Python,
                                    )
                                    .message(format!(
                                        "Unresolved relative Python import 'from .{}' in '{}'",
                                        imp.module, rel_path
                                    ))
                                    .location(
                                        DiagnosticLocation::new(file.clone())
                                            .with_span(stackvet_core::TextSpan::single_line(imp.line, 1, 1)),
                                    )
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
            } else if matches!(ext, "ts" | "tsx" | "js" | "jsx") {
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
                                    .location(
                                        DiagnosticLocation::new(file.clone())
                                            .with_span(stackvet_core::TextSpan::single_line(imp.line, 1, 1)),
                                    )
                                    .explanation(
                                        "Specifier could not be resolved to any file or directory index on disk.",
                                    )
                                    .remediation("Check file extension, path casing, or tsconfig path aliases.")
                                    .build(),
                                );
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
            diagnostics.push(
                Diagnostic::builder("IMP-001", Severity::Error, LanguageTarget::General)
                    .message(format!("Circular import cycle detected: {}", cycle_path))
                    .file(Path::new(&cycle[0]))
                    .explanation(format!(
                        "Cycles between modules ({}) cause runtime import failures, initialization deadlock, or undefined symbols.",
                        cycle_path
                    ))
                    .remediation("Refactor shared interfaces into a separate common module or use dependency injection.")
                    .build(),
            );
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
