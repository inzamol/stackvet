pub mod lockfile;
pub mod stdlib;

pub use lockfile::*;

use anyhow::Result;
use ruff_python_ast::visitor::{walk_expr, walk_stmt, Visitor};
use ruff_python_ast::{
    Expr, ExprAttribute, ExprCall, ExprName, ExprStringLiteral, Stmt, StmtImport, StmtImportFrom,
};
use ruff_python_parser::parse_module;
use stackvet_core::{
    byte_range_to_span, Diagnostic, DiagnosticLocation, LanguageTarget, PythonProjectInfo,
    Severity, TextSpan,
};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonImport {
    pub module: String,
    pub symbol: Option<String>,
    pub line: usize,
    pub is_relative: bool,
    pub level: usize,
    pub is_dynamic: bool,
    pub span: Option<TextSpan>,
    pub statement_line: usize,
}

struct ImportVisitor<'b> {
    content: &'b str,
    imports: Vec<PythonImport>,
}

impl<'a, 'b> Visitor<'a> for ImportVisitor<'b> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::Import(StmtImport { names, range, .. }) => {
                let stmt_span = byte_range_to_span(
                    self.content,
                    usize::from(range.start()),
                    usize::from(range.end()),
                );
                for alias in names {
                    let start_offset = usize::from(alias.range.start());
                    let end_offset = usize::from(alias.range.end());
                    let span = byte_range_to_span(self.content, start_offset, end_offset);
                    self.imports.push(PythonImport {
                        module: alias.name.to_string(),
                        symbol: None,
                        line: span.start_line,
                        is_relative: false,
                        level: 0,
                        is_dynamic: false,
                        span: Some(span),
                        statement_line: stmt_span.start_line,
                    });
                }
            }
            Stmt::ImportFrom(StmtImportFrom {
                module,
                names,
                level,
                range,
                ..
            }) => {
                let mod_name = module.as_ref().map(|m| m.to_string()).unwrap_or_default();
                let lvl = *level as usize;
                let stmt_span = byte_range_to_span(
                    self.content,
                    usize::from(range.start()),
                    usize::from(range.end()),
                );
                for alias in names {
                    let start_offset = usize::from(alias.range.start());
                    let end_offset = usize::from(alias.range.end());
                    let span = byte_range_to_span(self.content, start_offset, end_offset);
                    self.imports.push(PythonImport {
                        module: mod_name.clone(),
                        symbol: Some(alias.name.to_string()),
                        line: span.start_line,
                        is_relative: lvl > 0,
                        level: lvl,
                        is_dynamic: false,
                        span: Some(span),
                        statement_line: stmt_span.start_line,
                    });
                }
            }
            _ => {
                walk_stmt(self, stmt);
            }
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(ExprCall {
            func,
            arguments,
            range,
            ..
        }) = expr
        {
            let is_dynamic_import = match func.as_ref() {
                Expr::Name(ExprName { id, .. }) if id.as_str() == "__import__" => true,
                Expr::Attribute(ExprAttribute { value, attr, .. })
                    if attr.as_str() == "import_module" =>
                {
                    matches!(value.as_ref(), Expr::Name(ExprName { id, .. }) if id.as_str() == "importlib")
                }
                _ => false,
            };

            if is_dynamic_import {
                if let Some(Expr::StringLiteral(ExprStringLiteral { value, .. })) =
                    arguments.args.first()
                {
                    let raw_mod = value.to_str();
                    let start_offset = usize::from(range.start());
                    let end_offset = usize::from(range.end());
                    let span = byte_range_to_span(self.content, start_offset, end_offset);
                    let leading_dots = raw_mod.chars().take_while(|&c| c == '.').count();
                    let is_relative = leading_dots > 0;
                    let clean_mod = if is_relative {
                        raw_mod.trim_start_matches('.').to_string()
                    } else {
                        raw_mod.to_string()
                    };
                    self.imports.push(PythonImport {
                        module: clean_mod,
                        symbol: None,
                        line: span.start_line,
                        is_relative,
                        level: leading_dots,
                        is_dynamic: true,
                        span: Some(span),
                        statement_line: span.start_line,
                    });
                }
            }
        }
        walk_expr(self, expr);
    }
}

pub fn parse_python_imports_with_diagnostics(
    content: &str,
    file: &Path,
) -> (Vec<PythonImport>, Vec<Diagnostic>) {
    let mut visitor = ImportVisitor {
        content,
        imports: Vec::new(),
    };
    let mut diags = Vec::new();
    match parse_module(content) {
        Ok(parsed) => {
            for stmt in parsed.suite() {
                visitor.visit_stmt(stmt);
            }
        }
        Err(err) => {
            let loc = DiagnosticLocation::new(file.to_path_buf());
            let diag = Diagnostic::builder("DISC-002", Severity::Warning, LanguageTarget::Python)
                .message(format!("Python syntax error in '{}': {}", file.display(), err))
                .location(loc)
                .explanation("A Python source file contains invalid syntax and could not be parsed into an AST. Static analysis for this file is skipped or incomplete.")
                .remediation("Fix the syntax error in the source file.")
                .build();
            diags.push(diag);
        }
    }
    (visitor.imports, diags)
}

pub fn parse_python_imports(content: &str, file: &Path) -> Vec<PythonImport> {
    parse_python_imports_with_diagnostics(content, file).0
}

struct StringLiteralVisitor {
    literals: std::collections::HashSet<String>,
}

impl<'a> Visitor<'a> for StringLiteralVisitor {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::StringLiteral(ExprStringLiteral { value, .. }) = expr {
            self.literals.insert(value.to_str().to_string());
        }
        walk_expr(self, expr);
    }
}

pub fn parse_python_string_literals(content: &str) -> std::collections::HashSet<String> {
    let mut visitor = StringLiteralVisitor {
        literals: std::collections::HashSet::new(),
    };
    if let Ok(parsed) = parse_module(content) {
        for stmt in parsed.suite() {
            visitor.visit_stmt(stmt);
        }
    }
    visitor.literals
}

/// Normalizes Python package names (PEP 503: lowercase, hyphens/underscores normalized)
pub fn normalize_package_name(name: &str) -> String {
    name.to_lowercase().replace('_', "-")
}

/// Parses requirement spec like "requests>=2.28.0", "fastapi[all]~=0.100.0", "pytest"
pub fn parse_requirement_spec(spec: &str) -> Option<(String, String)> {
    let trimmed = spec.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
        return None;
    }

    // Strip environment markers: "requests; python_version >= '3.8'"
    let clean_spec = trimmed.split(';').next().unwrap_or(trimmed).trim();

    // Strip extras: "fastapi[all]" -> "fastapi"
    let (name_part, rest) = if let Some(bracket_idx) = clean_spec.find('[') {
        let name = &clean_spec[..bracket_idx];
        let after_bracket = &clean_spec[bracket_idx..];
        let after_extra = if let Some(close_bracket) = after_bracket.find(']') {
            &after_bracket[close_bracket + 1..]
        } else {
            ""
        };
        (name.trim(), after_extra.trim())
    } else {
        // Find comparison operators: ==, >=, <=, ~=, !=, >, <
        let op_indices: Vec<usize> = clean_spec
            .match_indices(['=', '>', '<', '~', '!'])
            .map(|(idx, _)| idx)
            .collect();
        if let Some(&first_op) = op_indices.first() {
            (clean_spec[..first_op].trim(), clean_spec[first_op..].trim())
        } else {
            (clean_spec.trim(), "")
        }
    };

    if name_part.is_empty() {
        return None;
    }

    Some((normalize_package_name(name_part), rest.to_string()))
}

pub fn parse_pyproject_toml(content: &str, path: &Path) -> Result<PythonProjectInfo> {
    let parsed: toml::Value = toml::from_str(content)?;
    let mut info = PythonProjectInfo {
        manifest_file: path.to_path_buf(),
        package_manager: "pip".to_string(),
        ..Default::default()
    };

    // Check [project] (PEP 621)
    if let Some(project) = parsed.get("project") {
        if let Some(req_py) = project.get("requires-python").and_then(|v| v.as_str()) {
            info.requires_python = Some(req_py.to_string());
        }

        if let Some(deps) = project.get("dependencies").and_then(|v| v.as_array()) {
            for dep in deps {
                if let Some(dep_str) = dep.as_str() {
                    if let Some((name, ver)) = parse_requirement_spec(dep_str) {
                        info.direct_dependencies.insert(name, ver);
                    }
                }
            }
        }

        if let Some(opt_deps) = project
            .get("optional-dependencies")
            .and_then(|v| v.as_table())
        {
            for (_group, deps_val) in opt_deps {
                if let Some(deps_arr) = deps_val.as_array() {
                    for dep in deps_arr {
                        if let Some(dep_str) = dep.as_str() {
                            if let Some((name, ver)) = parse_requirement_spec(dep_str) {
                                info.dev_dependencies.insert(name, ver);
                            }
                        }
                    }
                }
            }
        }
    }

    // Check [tool.poetry]
    if let Some(poetry) = parsed
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.as_table())
    {
        info.package_manager = "poetry".to_string();
        if let Some(deps) = poetry.get("dependencies").and_then(|d| d.as_table()) {
            for (pkg, val) in deps {
                if pkg == "python" {
                    if let Some(ver_str) = val.as_str() {
                        info.requires_python = Some(ver_str.to_string());
                    }
                } else {
                    let ver = match val {
                        toml::Value::String(s) => s.clone(),
                        toml::Value::Table(t) => t
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        _ => String::new(),
                    };
                    info.direct_dependencies
                        .insert(normalize_package_name(pkg), ver);
                }
            }
        }

        if let Some(group) = poetry
            .get("group")
            .and_then(|g| g.get("dev"))
            .and_then(|d| d.get("dependencies"))
            .and_then(|deps| deps.as_table())
        {
            for (pkg, val) in group {
                let ver = val.as_str().unwrap_or("").to_string();
                info.dev_dependencies
                    .insert(normalize_package_name(pkg), ver);
            }
        }
    }

    // Check [tool.uv]
    if parsed.get("tool").and_then(|t| t.get("uv")).is_some() {
        info.package_manager = "uv".to_string();
    }

    // Extract declared pythonpath / src layout search roots from pyproject.toml
    collect_pyproject_pythonpath(&parsed, path, &mut info.pythonpath);
    info.pythonpath.sort();
    info.pythonpath.dedup();

    // Extract declared entry points from pyproject.toml
    collect_pyproject_entry_points(&parsed, &mut info.entry_points);
    info.entry_points.sort();
    info.entry_points.dedup();

    Ok(info)
}

fn collect_pyproject_pythonpath(val: &toml::Value, manifest_path: &Path, out: &mut Vec<String>) {
    // 1. If manifest_path directory has a src/ folder, default to src
    if let Some(parent) = manifest_path.parent() {
        if parent.join("src").is_dir() {
            out.push("src".to_string());
        }
    }

    if let Some(tool) = val.get("tool").and_then(|v| v.as_table()) {
        // [tool.pytest.ini_options].pythonpath
        if let Some(pytest) = tool.get("pytest").and_then(|v| v.as_table()) {
            if let Some(ini) = pytest.get("ini_options").and_then(|v| v.as_table()) {
                if let Some(pp) = ini.get("pythonpath") {
                    if let Some(arr) = pp.as_array() {
                        for item in arr.iter().filter_map(|v| v.as_str()) {
                            out.push(item.to_string());
                        }
                    } else if let Some(s) = pp.as_str() {
                        out.push(s.to_string());
                    }
                }
            }
        }

        // [tool.setuptools.package-dir] e.g. "" = "src"
        if let Some(st) = tool.get("setuptools").and_then(|v| v.as_table()) {
            if let Some(pkg_dir) = st.get("package-dir").and_then(|v| v.as_table()) {
                for dir in pkg_dir.values().filter_map(|v| v.as_str()) {
                    if !dir.is_empty() {
                        out.push(dir.to_string());
                    }
                }
            }
        }

        // [tool.ruff].src
        if let Some(ruff) = tool.get("ruff").and_then(|v| v.as_table()) {
            if let Some(src_val) = ruff.get("src").and_then(|v| v.as_array()) {
                for item in src_val.iter().filter_map(|v| v.as_str()) {
                    out.push(item.to_string());
                }
            }
        }

        // [tool.pyright].extraPaths
        if let Some(pyright) = tool.get("pyright").and_then(|v| v.as_table()) {
            if let Some(ep) = pyright.get("extraPaths").and_then(|v| v.as_array()) {
                for item in ep.iter().filter_map(|v| v.as_str()) {
                    out.push(item.to_string());
                }
            }
        }
    }
}

/// Returns all candidate python module search roots (e.g. root, root/src, and configured pythonpath entries)
pub fn get_python_search_roots(
    root: &Path,
    info: Option<&stackvet_core::PythonProjectInfo>,
) -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    roots.push(root.to_path_buf());
    let src_dir = root.join("src");
    if src_dir.is_dir() && !roots.contains(&src_dir) {
        roots.push(src_dir);
    }
    if let Some(py_info) = info {
        for p in &py_info.pythonpath {
            let path = root.join(p);
            if path.is_dir() && !roots.contains(&path) {
                roots.push(path);
            }
        }
    }
    roots
}

/// Determines whether a top-level package or module name exists locally in any python search root
pub fn is_local_python_package(search_roots: &[std::path::PathBuf], root_pkg: &str) -> bool {
    for root in search_roots {
        if root.join(format!("{}.py", root_pkg)).is_file() {
            return true;
        }
        let dir = root.join(root_pkg);
        if dir.is_dir() {
            if dir.join("__init__.py").is_file() {
                return true;
            }
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("py") {
                        return true;
                    }
                    if p.is_dir() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Resolves an absolute Python import against candidate search roots
pub fn resolve_python_absolute_import(
    search_roots: &[std::path::PathBuf],
    imp: &PythonImport,
) -> Option<std::path::PathBuf> {
    let mod_subpath = imp.module.replace('.', "/");
    for root in search_roots {
        // 1. root / mod_subpath.py
        let candidate_file = root.join(format!("{}.py", mod_subpath));
        if candidate_file.is_file() {
            return Some(candidate_file);
        }
        // 2. Candidate directory
        let candidate_dir = root.join(&mod_subpath);

        // 3. Symbol as submodule within candidate_dir (e.g. from my_pkg import worker -> my_pkg/worker.py)
        if let Some(sym) = &imp.symbol {
            let symbol_file = candidate_dir.join(format!("{}.py", sym));
            if symbol_file.is_file() {
                return Some(symbol_file);
            }
            let symbol_init = candidate_dir.join(sym).join("__init__.py");
            if symbol_init.is_file() {
                return Some(symbol_init);
            }
        }

        // 4. root / mod_subpath / __init__.py
        let candidate_init = candidate_dir.join("__init__.py");
        if candidate_init.is_file() {
            return Some(candidate_init);
        }
        // 4. PEP 420 namespace package
        if candidate_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&candidate_dir) {
                let mut py_files: Vec<std::path::PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("py"))
                    .collect();
                py_files.sort();
                if let Some(first) = py_files.into_iter().next() {
                    return Some(first);
                }
            }
        }
    }
    None
}

fn collect_pyproject_entry_points(val: &toml::Value, out: &mut Vec<String>) {
    // 1. [project.scripts] and [project.gui-scripts]
    if let Some(project) = val.get("project") {
        if let Some(scripts) = project.get("scripts").and_then(|v| v.as_table()) {
            for spec in scripts.values().filter_map(|v| v.as_str()) {
                add_python_entry_spec(spec, out);
            }
        }
        if let Some(gui_scripts) = project.get("gui-scripts").and_then(|v| v.as_table()) {
            for spec in gui_scripts.values().filter_map(|v| v.as_str()) {
                add_python_entry_spec(spec, out);
            }
        }
        if let Some(entry_points) = project.get("entry-points").and_then(|v| v.as_table()) {
            for group in entry_points.values().filter_map(|v| v.as_table()) {
                for spec in group.values().filter_map(|v| v.as_str()) {
                    add_python_entry_spec(spec, out);
                }
            }
        }
    }

    // 2. [tool.poetry.scripts]
    if let Some(poetry_scripts) = val
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.get("scripts"))
        .and_then(|s| s.as_table())
    {
        for spec in poetry_scripts.values().filter_map(|v| v.as_str()) {
            add_python_entry_spec(spec, out);
        }
    }

    // 3. [tool.flit.scripts]
    if let Some(flit_scripts) = val
        .get("tool")
        .and_then(|t| t.get("flit"))
        .and_then(|f| f.get("scripts"))
        .and_then(|s| s.as_table())
    {
        for spec in flit_scripts.values().filter_map(|v| v.as_str()) {
            add_python_entry_spec(spec, out);
        }
    }
}

fn add_python_entry_spec(spec: &str, out: &mut Vec<String>) {
    let trimmed = spec.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
        // Also add the module part before ':'
        if let Some((module, _)) = trimmed.split_once(':') {
            let m_trim = module.trim();
            if !m_trim.is_empty() {
                out.push(m_trim.to_string());
            }
        }
    }
}

pub fn parse_requirements_txt(content: &str, path: &Path) -> PythonProjectInfo {
    let mut info = PythonProjectInfo {
        manifest_file: path.to_path_buf(),
        package_manager: "pip".to_string(),
        ..Default::default()
    };

    for line in content.lines() {
        if let Some((name, ver)) = parse_requirement_spec(line) {
            info.direct_dependencies.insert(name, ver);
        }
    }

    info
}

/// Detects a Python project and loads its manifest and available lockfile metadata.
pub fn detect_python_project(root: &Path) -> Option<PythonProjectInfo> {
    detect_python_project_with_diagnostics(root).0
}

/// Detects a Python project and returns collected discovery/manifest diagnostics.
const MAX_MANIFEST_SIZE: u64 = 10 * 1024 * 1024; // 10 MB

pub fn detect_python_project_with_diagnostics(
    root: &Path,
) -> (Option<PythonProjectInfo>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let pyproject = root.join("pyproject.toml");
    let mut pyproject_val: Option<toml::Value> = None;

    if pyproject.is_file() {
        let is_oversized = fs::metadata(&pyproject)
            .map(|m| m.len() > MAX_MANIFEST_SIZE)
            .unwrap_or(false);
        if is_oversized {
            diags.push(
                Diagnostic::builder("DISC-005", Severity::Warning, LanguageTarget::Python)
                    .message("Scan resource limit exceeded: 'pyproject.toml' exceeds 10 MB limit, skipping parsing")
                    .location(DiagnosticLocation::new(pyproject.clone()))
                    .explanation("The manifest exceeds the maximum allowable file size (10 MB).")
                    .remediation("Reduce the manifest size or configure a higher max_file_size limit.")
                    .build(),
            );
        } else {
            match fs::read_to_string(&pyproject) {
                Ok(c) => match toml::from_str::<toml::Value>(&c) {
                    Ok(val) => {
                        pyproject_val = Some(val);
                    }
                    Err(e) => {
                        let mut loc = DiagnosticLocation::new(pyproject.clone());
                        let mut coords = String::new();
                        if let Some(r) = e.span() {
                            let span = stackvet_core::byte_range_to_span(&c, r.start, r.end);
                            coords =
                                format!(" at line {}, column {}", span.start_line, span.start_col);
                            loc = loc.with_span(span);
                        }
                        diags.push(
                        Diagnostic::builder("DISC-004", Severity::Error, LanguageTarget::Python)
                            .message(format!(
                                "Malformed manifest 'pyproject.toml'{}: {}",
                                coords,
                                e.message()
                            ))
                            .location(loc)
                            .explanation("The pyproject.toml manifest contains invalid TOML syntax or corrupted structure.")
                            .remediation("Validate and correct the TOML syntax in pyproject.toml.")
                            .build(),
                    );
                    }
                },
                Err(e) => {
                    diags.push(
                    Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Python)
                        .message(format!("Unreadable manifest 'pyproject.toml': {}", e))
                        .location(DiagnosticLocation::new(pyproject.clone()))
                        .explanation("The pyproject.toml file could not be read due to I/O permissions or encoding issues.")
                        .remediation("Check file read permissions and ensure file is valid UTF-8.")
                        .build(),
                );
                }
            }
        }
    }

    let members = discover_python_workspace_members(root, pyproject_val.as_ref());

    let mut info = if pyproject.is_file() && pyproject_val.is_some() {
        if let Ok(content) = fs::read_to_string(&pyproject) {
            match parse_pyproject_toml(&content, &pyproject) {
                Ok(inf) => Some(inf),
                Err(e) => {
                    diags.push(
                        Diagnostic::builder("DISC-004", Severity::Error, LanguageTarget::Python)
                            .message(format!("Invalid configuration in 'pyproject.toml': {}", e))
                            .location(DiagnosticLocation::new(pyproject.clone()))
                            .explanation("The pyproject.toml manifest could not be parsed into project metadata.")
                            .remediation("Verify the [project] or [tool.poetry] sections in pyproject.toml.")
                            .build(),
                    );
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let reqs = root.join("requirements.txt");
    if info.is_none() && !members.is_empty() {
        if reqs.is_file() {
            match fs::read_to_string(&reqs) {
                Ok(content) => {
                    info = Some(parse_requirements_txt(&content, &reqs));
                }
                Err(e) => {
                    diags.push(
                        Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Python)
                            .message(format!("Unreadable 'requirements.txt': {}", e))
                            .location(DiagnosticLocation::new(reqs.clone()))
                            .explanation("The requirements.txt file could not be read due to I/O permissions or encoding issues.")
                            .remediation("Check file read permissions and ensure file is valid UTF-8.")
                            .build(),
                    );
                }
            }
        }
        if info.is_none() {
            info = Some(PythonProjectInfo {
                package_manager: "uv".to_string(),
                manifest_file: pyproject.clone(),
                ..Default::default()
            });
        }
    }

    if let Some(mut info) = info {
        info.workspace_members = members;
        for m in &info.workspace_members {
            let v = m.version.clone().unwrap_or_else(|| "*".to_string());
            info.workspace_packages.entry(m.name.clone()).or_insert(v);
        }

        let uv_lock = root.join("uv.lock");
        let poetry_lock = root.join("poetry.lock");
        let pipfile_lock = root.join("Pipfile.lock");

        if uv_lock.is_file() {
            info.lock_file = Some(uv_lock.clone());
            info.package_manager = "uv".to_string();
            let is_oversized = fs::metadata(&uv_lock)
                .map(|m| m.len() > MAX_MANIFEST_SIZE)
                .unwrap_or(false);
            if is_oversized {
                diags.push(
                    Diagnostic::builder("DISC-005", Severity::Warning, LanguageTarget::Python)
                        .message("Scan resource limit exceeded: 'uv.lock' exceeds 10 MB limit, skipping parsing")
                        .location(DiagnosticLocation::new(uv_lock))
                        .explanation("The uv.lock file exceeds the maximum allowable file size (10 MB).")
                        .remediation("Increase max_file_size or optimize the lockfile.")
                        .build(),
                );
            } else {
                match fs::read_to_string(&uv_lock) {
                    Ok(lock_str) => {
                        let data = parse_uv_lock(&lock_str);
                        info.installed_packages = data.locked_packages.clone();
                        info.locked_packages = data.locked_packages;
                        info.locked_versions = data.locked_versions;
                        for (k, v) in data.workspace_packages {
                            info.workspace_packages.entry(k).or_insert(v);
                        }
                    }
                    Err(e) => {
                        diags.push(
                            Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Python)
                                .message(format!("Unreadable lockfile 'uv.lock': {}", e))
                                .location(DiagnosticLocation::new(uv_lock))
                                .explanation("The uv.lock file could not be read due to I/O permissions or encoding issues.")
                                .remediation("Check file read permissions and ensure file is valid UTF-8.")
                                .build(),
                        );
                    }
                }
            }
        } else if poetry_lock.is_file() {
            info.lock_file = Some(poetry_lock.clone());
            info.package_manager = "poetry".to_string();
            let is_oversized = fs::metadata(&poetry_lock)
                .map(|m| m.len() > MAX_MANIFEST_SIZE)
                .unwrap_or(false);
            if is_oversized {
                diags.push(
                    Diagnostic::builder("DISC-005", Severity::Warning, LanguageTarget::Python)
                        .message("Scan resource limit exceeded: 'poetry.lock' exceeds 10 MB limit, skipping parsing")
                        .location(DiagnosticLocation::new(poetry_lock))
                        .explanation("The poetry.lock file exceeds the maximum allowable file size (10 MB).")
                        .remediation("Increase max_file_size or optimize the lockfile.")
                        .build(),
                );
            } else {
                match fs::read_to_string(&poetry_lock) {
                    Ok(lock_str) => {
                        let data = parse_poetry_lock(&lock_str);
                        info.installed_packages = data.locked_packages.clone();
                        info.locked_packages = data.locked_packages;
                        info.locked_versions = data.locked_versions;
                        for (k, v) in data.workspace_packages {
                            info.workspace_packages.entry(k).or_insert(v);
                        }
                    }
                    Err(e) => {
                        diags.push(
                            Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Python)
                                .message(format!("Unreadable lockfile 'poetry.lock': {}", e))
                                .location(DiagnosticLocation::new(poetry_lock))
                                .explanation("The poetry.lock file could not be read due to I/O permissions or encoding issues.")
                                .remediation("Check file read permissions and ensure file is valid UTF-8.")
                                .build(),
                        );
                    }
                }
            }
        } else if pipfile_lock.is_file() {
            info.lock_file = Some(pipfile_lock.clone());
            info.package_manager = "pipenv".to_string();
            match fs::read_to_string(&pipfile_lock) {
                Ok(lock_str) => {
                    let data = parse_pipfile_lock(&lock_str);
                    info.installed_packages = data.locked_packages.clone();
                    info.locked_packages = data.locked_packages;
                    info.locked_versions = data.locked_versions;
                }
                Err(e) => {
                    diags.push(
                        Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Python)
                            .message(format!("Unreadable lockfile 'Pipfile.lock': {}", e))
                            .location(DiagnosticLocation::new(pipfile_lock))
                            .explanation("The Pipfile.lock file could not be read due to I/O permissions or encoding issues.")
                            .remediation("Check file read permissions and ensure file is valid UTF-8.")
                            .build(),
                    );
                }
            }
        }
        return (Some(info), diags);
    }

    if reqs.is_file() {
        match fs::read_to_string(&reqs) {
            Ok(content) => {
                return (Some(parse_requirements_txt(&content, &reqs)), diags);
            }
            Err(e) => {
                diags.push(
                    Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Python)
                        .message(format!("Unreadable 'requirements.txt': {}", e))
                        .location(DiagnosticLocation::new(reqs))
                        .explanation("The requirements.txt file could not be read due to I/O permissions or encoding issues.")
                        .remediation("Check file read permissions and ensure file is valid UTF-8.")
                        .build(),
                );
            }
        }
    }

    (None, diags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_python_imports() {
        let code = r#"
import os
import sys, math
from pathlib import Path
from .local_mod import helper
import requests
"#;
        let imports = parse_python_imports(code, Path::new("test.py"));
        let modules: Vec<&str> = imports.iter().map(|i| i.module.as_str()).collect();
        assert!(modules.contains(&"os"));
        assert!(modules.contains(&"sys"));
        assert!(modules.contains(&"math"));
        assert!(modules.contains(&"pathlib"));
        assert!(modules.contains(&"local_mod"));
        assert!(modules.contains(&"requests"));

        let rel = imports.iter().find(|i| i.module == "local_mod").unwrap();
        assert!(rel.is_relative);
        assert_eq!(rel.level, 1);
        assert_eq!(rel.symbol.as_deref(), Some("helper"));
    }

    #[test]
    fn test_parse_relative_and_nested_imports() {
        let code = r#"
from . import sibling
from ..parent_pkg import util
from ...ancestor.core import engine

def load_plugins():
    import plugin_a
    try:
        from optional_pkg import feature
    except ImportError:
        import fallback_pkg

class Service:
    def execute(self):
        import worker
"#;
        let imports = parse_python_imports(code, Path::new("service.py"));
        let modules: Vec<&str> = imports.iter().map(|i| i.module.as_str()).collect();

        // Check relative imports with varying levels
        let rel_sibling = imports
            .iter()
            .find(|i| i.symbol.as_deref() == Some("sibling"))
            .unwrap();
        assert!(rel_sibling.is_relative);
        assert_eq!(rel_sibling.level, 1);
        assert_eq!(rel_sibling.module, "");

        let rel_parent = imports.iter().find(|i| i.module == "parent_pkg").unwrap();
        assert!(rel_parent.is_relative);
        assert_eq!(rel_parent.level, 2);
        assert_eq!(rel_parent.symbol.as_deref(), Some("util"));

        let rel_ancestor = imports
            .iter()
            .find(|i| i.module == "ancestor.core")
            .unwrap();
        assert!(rel_ancestor.is_relative);
        assert_eq!(rel_ancestor.level, 3);
        assert_eq!(rel_ancestor.symbol.as_deref(), Some("engine"));

        // Check nested imports inside function, try/except, and class method
        assert!(modules.contains(&"plugin_a"));
        assert!(modules.contains(&"optional_pkg"));
        assert!(modules.contains(&"fallback_pkg"));
        assert!(modules.contains(&"worker"));
    }

    #[test]
    fn test_parse_dynamic_imports() {
        let code = r#"
import os
backend = __import__("custom_backend")
import importlib
driver = importlib.import_module("database_driver")
"#;
        let imports = parse_python_imports(code, Path::new("app.py"));
        let dynamic_imports: Vec<&PythonImport> = imports.iter().filter(|i| i.is_dynamic).collect();
        assert_eq!(dynamic_imports.len(), 2);

        let dyn_backend = dynamic_imports
            .iter()
            .find(|i| i.module == "custom_backend")
            .unwrap();
        assert!(dyn_backend.is_dynamic);
        assert!(!dyn_backend.is_relative);

        let dyn_driver = dynamic_imports
            .iter()
            .find(|i| i.module == "database_driver")
            .unwrap();
        assert!(dyn_driver.is_dynamic);
        assert!(!dyn_driver.is_relative);
    }

    #[test]
    fn test_parse_requirement_spec() {
        assert_eq!(
            parse_requirement_spec("requests>=2.28.0"),
            Some(("requests".to_string(), ">=2.28.0".to_string()))
        );
        assert_eq!(
            parse_requirement_spec("FastAPI[all]~=0.100.0"),
            Some(("fastapi".to_string(), "~=0.100.0".to_string()))
        );
        assert_eq!(
            parse_requirement_spec("uvicorn"),
            Some(("uvicorn".to_string(), "".to_string()))
        );
        assert_eq!(
            parse_requirement_spec("pydantic_core >= 2.0"),
            Some(("pydantic-core".to_string(), ">= 2.0".to_string()))
        );
    }

    #[test]
    fn test_parse_pyproject() {
        let toml_str = r#"
[project]
name = "my-app"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = [
    "fastapi>=0.100.0",
    "uvicorn[standard]>=0.20.0"
]

[project.optional-dependencies]
dev = [
    "pytest>=7.0.0"
]
"#;
        let info = parse_pyproject_toml(toml_str, Path::new("pyproject.toml")).unwrap();
        assert_eq!(info.requires_python.as_deref(), Some(">=3.11"));
        assert!(info.direct_dependencies.contains_key("fastapi"));
        assert!(info.direct_dependencies.contains_key("uvicorn"));
        assert!(info.dev_dependencies.contains_key("pytest"));
    }

    #[test]
    fn test_pythonpath_extraction_and_resolution() {
        let toml_str = r#"
[project]
name = "my-app"
version = "0.1.0"

[tool.pytest.ini_options]
pythonpath = ["src", "lib"]

[tool.ruff]
src = ["src", "custom_dir"]
"#;
        let info = parse_pyproject_toml(toml_str, Path::new("pyproject.toml")).unwrap();
        assert!(info.pythonpath.contains(&"src".to_string()));
        assert!(info.pythonpath.contains(&"lib".to_string()));
        assert!(info.pythonpath.contains(&"custom_dir".to_string()));

        let temp_dir = std::env::temp_dir().join(format!(
            "stackvet_python_search_roots_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let src_dir = temp_dir.join("src");
        let pkg_dir = src_dir.join("my_service");
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(pkg_dir.join("__init__.py"), "").unwrap();
        fs::write(pkg_dir.join("worker.py"), "def work(): pass\n").unwrap();

        let search_roots = get_python_search_roots(&temp_dir, Some(&info));
        assert!(search_roots.contains(&temp_dir));
        assert!(search_roots.contains(&src_dir));

        assert!(is_local_python_package(&search_roots, "my_service"));
        assert!(!is_local_python_package(&search_roots, "nonexistent_pkg"));

        let imp = PythonImport {
            module: "my_service.worker".to_string(),
            symbol: None,
            line: 1,
            is_relative: false,
            level: 0,
            is_dynamic: false,
            span: None,
            statement_line: 1,
        };
        let resolved = resolve_python_absolute_import(&search_roots, &imp);
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), pkg_dir.join("worker.py"));

        let imp_from = PythonImport {
            module: "my_service".to_string(),
            symbol: Some("worker".to_string()),
            line: 1,
            is_relative: false,
            level: 0,
            is_dynamic: false,
            span: None,
            statement_line: 1,
        };
        let resolved_from = resolve_python_absolute_import(&search_roots, &imp_from);
        assert!(resolved_from.is_some());
        assert_eq!(resolved_from.unwrap(), pkg_dir.join("worker.py"));

        let imp_missing = PythonImport {
            module: "my_service.missing_mod".to_string(),
            symbol: None,
            line: 1,
            is_relative: false,
            level: 0,
            is_dynamic: false,
            span: None,
            statement_line: 1,
        };
        let resolved_missing = resolve_python_absolute_import(&search_roots, &imp_missing);
        assert!(resolved_missing.is_none());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
