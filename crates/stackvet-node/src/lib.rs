pub mod builtins;
pub mod lockfile;

pub use lockfile::*;

use anyhow::Result;
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast::visit::walk;
use oxc_ast::visit::Visit;
use oxc_parser::Parser;
use oxc_span::SourceType;
use stackvet_core::{
    byte_range_to_span, Diagnostic, DiagnosticLocation, LanguageTarget, NodeProjectInfo, Severity,
    TextSpan,
};
use std::fs;
use std::path::Path;

struct NodeImportVisitor<'a> {
    content: &'a str,
    imports: Vec<NodeImport>,
}

impl<'a> Visit<'a> for NodeImportVisitor<'a> {
    fn visit_module_declaration(&mut self, decl: &ModuleDeclaration<'a>) {
        match decl {
            ModuleDeclaration::ImportDeclaration(import_decl) => {
                let specifier = import_decl.source.value.to_string();
                let span = byte_range_to_span(
                    self.content,
                    import_decl.span.start as usize,
                    import_decl.span.end as usize,
                );
                self.imports.push(NodeImport {
                    specifier,
                    is_dynamic: false,
                    is_type_only: import_decl.import_kind.is_type(),
                    line: span.start_line,
                    span: Some(span),
                });
            }
            ModuleDeclaration::ExportAllDeclaration(export_all) => {
                let specifier = export_all.source.value.to_string();
                let span = byte_range_to_span(
                    self.content,
                    export_all.span.start as usize,
                    export_all.span.end as usize,
                );
                self.imports.push(NodeImport {
                    specifier,
                    is_dynamic: false,
                    is_type_only: export_all.export_kind.is_type(),
                    line: span.start_line,
                    span: Some(span),
                });
            }
            ModuleDeclaration::ExportNamedDeclaration(export_named) => {
                if let Some(source) = &export_named.source {
                    let specifier = source.value.to_string();
                    let span = byte_range_to_span(
                        self.content,
                        export_named.span.start as usize,
                        export_named.span.end as usize,
                    );
                    self.imports.push(NodeImport {
                        specifier,
                        is_dynamic: false,
                        is_type_only: export_named.export_kind.is_type(),
                        line: span.start_line,
                        span: Some(span),
                    });
                }
            }
            _ => {}
        }
        walk::walk_module_declaration(self, decl);
    }

    fn visit_ts_import_equals_declaration(&mut self, decl: &TSImportEqualsDeclaration<'a>) {
        if let TSModuleReference::ExternalModuleReference(module_ref) = &decl.module_reference {
            let specifier = module_ref.expression.value.to_string();
            let span = byte_range_to_span(
                self.content,
                decl.span.start as usize,
                decl.span.end as usize,
            );
            self.imports.push(NodeImport {
                specifier,
                is_dynamic: false,
                is_type_only: decl.import_kind.is_type(),
                line: span.start_line,
                span: Some(span),
            });
        }
        walk::walk_ts_import_equals_declaration(self, decl);
    }

    fn visit_import_expression(&mut self, expr: &ImportExpression<'a>) {
        if let Expression::StringLiteral(lit) = &expr.source {
            let span = byte_range_to_span(
                self.content,
                expr.span.start as usize,
                expr.span.end as usize,
            );
            self.imports.push(NodeImport {
                specifier: lit.value.to_string(),
                is_dynamic: true,
                is_type_only: false,
                line: span.start_line,
                span: Some(span),
            });
        }
        walk::walk_import_expression(self, expr);
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        let is_require = match &expr.callee {
            Expression::Identifier(id) => id.name.as_str() == "require",
            Expression::StaticMemberExpression(member)
                if member.property.name.as_str() == "resolve" =>
            {
                if let Expression::Identifier(obj) = &member.object {
                    obj.name.as_str() == "require"
                } else {
                    false
                }
            }
            _ => false,
        };

        if is_require {
            if let Some(first_arg) = expr.arguments.first() {
                if let Some(Expression::StringLiteral(lit)) = first_arg.as_expression() {
                    let span = byte_range_to_span(
                        self.content,
                        expr.span.start as usize,
                        expr.span.end as usize,
                    );
                    self.imports.push(NodeImport {
                        specifier: lit.value.to_string(),
                        is_dynamic: true,
                        is_type_only: false,
                        line: span.start_line,
                        span: Some(span),
                    });
                }
            }
        }

        walk::walk_call_expression(self, expr);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeImport {
    pub specifier: String,
    pub is_dynamic: bool,
    pub is_type_only: bool,
    pub line: usize,
    pub span: Option<TextSpan>,
}

pub fn parse_node_imports(content: &str, file: &Path) -> Vec<NodeImport> {
    parse_node_imports_with_diagnostics(content, file).0
}

pub fn parse_node_imports_with_diagnostics(
    content: &str,
    file: &Path,
) -> (Vec<NodeImport>, Vec<Diagnostic>) {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(file).unwrap_or_default();
    let ret = Parser::new(&allocator, content, source_type).parse();

    let mut diags = Vec::new();
    if let Some(err) = ret.errors.first() {
        let msg = err.to_string();
        let loc = DiagnosticLocation::new(file.to_path_buf());
        let diag = Diagnostic::builder("DISC-003", Severity::Warning, LanguageTarget::Node)
            .message(format!(
                "JavaScript/TypeScript syntax error in '{}': {}",
                file.display(),
                msg
            ))
            .location(loc)
            .explanation("A JavaScript or TypeScript source file contains invalid syntax and could not be parsed into an AST. Static analysis for this file is skipped or incomplete.")
            .remediation("Fix the syntax error in the source file.")
            .build();
        diags.push(diag);
    }

    let mut visitor = NodeImportVisitor {
        content,
        imports: Vec::new(),
    };
    visitor.visit_program(&ret.program);
    (visitor.imports, diags)
}

/// Normalizes package name from import specifier:
/// "@scope/pkg/foo" -> "@scope/pkg"
/// "axios/lib/adapters" -> "axios"
/// "./utils" -> None (local import)
/// "@/components/Button" -> None (path alias)
pub fn extract_package_name(specifier: &str) -> Option<String> {
    if specifier.starts_with('.')
        || specifier.starts_with('/')
        || specifier.starts_with("@/")
        || specifier.starts_with("~/")
    {
        return None;
    }
    let parts: Vec<&str> = specifier.split('/').collect();
    if specifier.starts_with('@') {
        if parts[0] == "@" {
            return None;
        }
        if parts.len() >= 2 {
            Some(format!("{}/{}", parts[0], parts[1]))
        } else {
            Some(specifier.to_string())
        }
    } else {
        parts.first().map(|s| s.to_string())
    }
}

/// Strips line comments, block comments, and trailing commas from JSONC (tsconfig.json / jsconfig.json)
pub fn strip_json_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut in_string = false;
    let mut escape = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for nc in chars.by_ref() {
                if nc == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(nc) = chars.next() {
                if nc == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }

    let mut result = String::with_capacity(out.len());
    let out_chars: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < out_chars.len() {
        if out_chars[i] == ',' {
            let mut j = i + 1;
            while j < out_chars.len() && out_chars[j].is_whitespace() {
                j += 1;
            }
            if j < out_chars.len() && (out_chars[j] == '}' || out_chars[j] == ']') {
                i += 1;
                continue;
            }
        }
        result.push(out_chars[i]);
        i += 1;
    }

    result
}

/// Parses compilerOptions.paths and compilerOptions.baseUrl from tsconfig.json or jsconfig.json
pub fn parse_tsconfig(
    content: &str,
) -> (
    std::collections::HashMap<String, Vec<String>>,
    Option<String>,
) {
    let clean = strip_json_comments(content);
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&clean) {
        if let Some(opts) = val.get("compilerOptions") {
            let base_url = opts
                .get("baseUrl")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let mut paths = std::collections::HashMap::new();
            if let Some(paths_obj) = opts.get("paths").and_then(|v| v.as_object()) {
                for (k, v) in paths_obj {
                    let targets: Vec<String> = if let Some(arr) = v.as_array() {
                        arr.iter()
                            .filter_map(|t| t.as_str().map(|s| s.to_string()))
                            .collect()
                    } else if let Some(s) = v.as_str() {
                        vec![s.to_string()]
                    } else {
                        Vec::new()
                    };
                    paths.insert(k.clone(), targets);
                }
            }
            return (paths, base_url);
        }
    }
    (std::collections::HashMap::new(), None)
}

/// Selects the best matching paths key: exact match first, then wildcard with longest prefix.
fn find_best_matching_path_key<'a>(
    specifier: &str,
    paths: &'a std::collections::HashMap<String, Vec<String>>,
) -> Option<(&'a str, &'a [String])> {
    if let Some((key, targets)) = paths.get_key_value(specifier) {
        return Some((key.as_str(), targets.as_slice()));
    }

    let mut best_match: Option<(&'a str, usize, &'a [String])> = None;
    for (pattern, targets) in paths {
        if let Some(star_idx) = pattern.find('*') {
            let prefix = &pattern[..star_idx];
            let suffix = &pattern[star_idx + 1..];
            if specifier.starts_with(prefix)
                && specifier.ends_with(suffix)
                && specifier.len() >= prefix.len() + suffix.len()
            {
                let prefix_len = prefix.len();
                if best_match
                    .as_ref()
                    .is_none_or(|(_, len, _)| prefix_len > *len)
                {
                    best_match = Some((pattern.as_str(), prefix_len, targets.as_slice()));
                }
            }
        }
    }

    best_match.map(|(pattern, _, targets)| (pattern, targets))
}

/// Checks whether an import specifier matches a TypeScript path alias pattern
pub fn is_tsconfig_alias(
    specifier: &str,
    paths: &std::collections::HashMap<String, Vec<String>>,
) -> bool {
    if specifier.starts_with('.') || specifier.starts_with('/') {
        return false;
    }
    if specifier.starts_with("@/") || specifier.starts_with("~/") {
        return true;
    }
    if let Some((pattern, _)) = find_best_matching_path_key(specifier, paths) {
        // Ensure a bare "*" (prefix "") does not classify every arbitrary package as an alias
        if pattern == "*" {
            return false;
        }
        return true;
    }
    false
}

/// Resolves a TypeScript path alias or baseUrl import to a concrete file on disk
pub fn resolve_aliased_import(
    root: &Path,
    paths: &std::collections::HashMap<String, Vec<String>>,
    base_url: Option<&str>,
    specifier: &str,
) -> Option<std::path::PathBuf> {
    let extensions = ["ts", "tsx", "js", "jsx", "mjs", "cjs", "json"];
    let base_dir = if let Some(b) = base_url {
        let trimmed = b.trim_start_matches("./");
        if trimmed.is_empty() || trimmed == "." {
            root.to_path_buf()
        } else {
            root.join(trimmed)
        }
    } else {
        root.to_path_buf()
    };

    // 1. Try best matching path key (exact first, then longest wildcard prefix)
    if let Some((pattern, targets)) = find_best_matching_path_key(specifier, paths) {
        if let Some(star_idx) = pattern.find('*') {
            let prefix = &pattern[..star_idx];
            let suffix = &pattern[star_idx + 1..];
            let remainder = &specifier[prefix.len()..specifier.len() - suffix.len()];
            for target in targets {
                let clean_target = target.replace('*', remainder);
                let target_trimmed = clean_target.trim_start_matches("./");
                let candidate = base_dir.join(target_trimmed);
                if let Some(resolved) = resolve_candidate_path(&candidate, &extensions) {
                    return Some(resolved);
                }
            }
        } else {
            for target in targets {
                let target_trimmed = target.trim_start_matches("./");
                let candidate = base_dir.join(target_trimmed);
                if let Some(resolved) = resolve_candidate_path(&candidate, &extensions) {
                    return Some(resolved);
                }
            }
        }
        return None;
    }

    // 2. Direct baseUrl fallback (only when no paths key matches)
    if base_url.is_some() {
        let candidate = base_dir.join(specifier);
        if let Some(resolved) = resolve_candidate_path(&candidate, &extensions) {
            return Some(resolved);
        }
    }

    None
}

fn resolve_candidate_path(candidate: &Path, extensions: &[&str]) -> Option<std::path::PathBuf> {
    if candidate.is_file() {
        return Some(candidate.to_path_buf());
    }

    // 1. If candidate has a recognized runtime extension (e.g. .js, .jsx, .mjs, .cjs),
    // substitute with TypeScript / JavaScript extensions:
    // .mjs -> .mts, .mjs
    // .cjs -> .cts, .cjs
    // .jsx -> .tsx, .jsx
    // .js  -> .ts, .tsx, .js, .jsx
    if let Some(ext) = candidate.extension().and_then(|s| s.to_str()) {
        let candidate_subs: &[&str] = match ext {
            "mjs" => &["mts", "mjs"],
            "cjs" => &["cts", "cjs"],
            "jsx" => &["tsx", "jsx"],
            "js" => &["ts", "tsx", "js", "jsx"],
            _ => &[],
        };
        for target_ext in candidate_subs {
            let with_sub = candidate.with_extension(target_ext);
            if with_sub.is_file() {
                return Some(with_sub);
            }
        }
    }

    // 2. Append lookup for dotted basenames / extensionless paths (e.g. ./user.service -> ./user.service.ts)
    let file_name = candidate.file_name().and_then(|n| n.to_str()).unwrap_or("");
    for ext in extensions {
        let with_ext = candidate.with_file_name(format!("{}.{}", file_name, ext));
        if with_ext.is_file() {
            return Some(with_ext);
        }
    }

    // 3. Directory index files
    if candidate.is_dir() {
        for ext in extensions {
            let index = candidate.join(format!("index.{}", ext));
            if index.is_file() {
                return Some(index);
            }
        }
    }
    None
}

pub fn parse_package_json(content: &str, path: &Path) -> Result<NodeProjectInfo> {
    let val: serde_json::Value = serde_json::from_str(content)?;
    let mut info = NodeProjectInfo {
        package_json_path: path.to_path_buf(),
        package_manager: "npm".to_string(),
        ..Default::default()
    };

    if let Some(pm) = val.get("packageManager").and_then(|v| v.as_str()) {
        let name = pm.split('@').next().unwrap_or(pm);
        info.package_manager = name.to_string();
    }

    if let Some(engines) = val.get("engines").and_then(|v| v.as_object()) {
        if let Some(node_ver) = engines.get("node").and_then(|v| v.as_str()) {
            info.node_version_constraint = Some(node_ver.to_string());
        }
    }

    if let Some(deps) = val.get("dependencies").and_then(|v| v.as_object()) {
        for (k, v) in deps {
            info.dependencies
                .insert(k.clone(), v.as_str().unwrap_or("").to_string());
        }
    }

    if let Some(dev_deps) = val.get("devDependencies").and_then(|v| v.as_object()) {
        for (k, v) in dev_deps {
            info.dev_dependencies
                .insert(k.clone(), v.as_str().unwrap_or("").to_string());
        }
    }

    if let Some(peer_deps) = val.get("peerDependencies").and_then(|v| v.as_object()) {
        for (k, v) in peer_deps {
            info.peer_dependencies
                .insert(k.clone(), v.as_str().unwrap_or("").to_string());
        }
    }

    if let Some(scripts) = val.get("scripts").and_then(|v| v.as_object()) {
        for (k, v) in scripts {
            if let Some(cmd) = v.as_str() {
                info.scripts.insert(k.clone(), cmd.to_string());
            }
        }
    }

    Ok(info)
}

/// Detects a Node project and loads its manifest, workspace packages, and lockfile versions.
pub fn detect_node_project(root: &Path) -> Option<NodeProjectInfo> {
    detect_node_project_with_diagnostics(root).0
}

/// Detects a Node project and returns collected discovery/manifest diagnostics.
const MAX_MANIFEST_SIZE: u64 = 10 * 1024 * 1024; // 10 MB

pub fn detect_node_project_with_diagnostics(
    root: &Path,
) -> (Option<NodeProjectInfo>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let pkg_json = root.join("package.json");
    let mut pkg_val: Option<serde_json::Value> = None;

    let mut info = if pkg_json.is_file() {
        let is_oversized = fs::metadata(&pkg_json)
            .map(|m| m.len() > MAX_MANIFEST_SIZE)
            .unwrap_or(false);
        if is_oversized {
            diags.push(
                Diagnostic::builder("DISC-005", Severity::Warning, LanguageTarget::Node)
                    .message("Scan resource limit exceeded: 'package.json' exceeds 10 MB limit, skipping parsing")
                    .location(DiagnosticLocation::new(pkg_json.clone()))
                    .explanation("The package.json manifest exceeds the maximum allowable file size (10 MB).")
                    .remediation("Reduce the manifest size or configure a higher max_file_size limit.")
                    .build(),
            );
            None
        } else {
            match fs::read_to_string(&pkg_json) {
                Ok(content) => {
                    let clean = strip_json_comments(&content);
                    match serde_json::from_str::<serde_json::Value>(&clean) {
                        Ok(val) => {
                            pkg_val = Some(val);
                            match parse_package_json(&content, &pkg_json) {
                                Ok(inf) => Some(inf),
                                Err(e) => {
                                    diags.push(
                                    Diagnostic::builder("DISC-004", Severity::Error, LanguageTarget::Node)
                                        .message(format!("Invalid configuration in 'package.json': {}", e))
                                        .location(DiagnosticLocation::new(pkg_json.clone()))
                                        .explanation("The package.json manifest could not be parsed into project metadata.")
                                        .remediation("Verify the dependencies and structure of package.json.")
                                        .build(),
                                );
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            diags.push(
                            Diagnostic::builder("DISC-004", Severity::Error, LanguageTarget::Node)
                                .message(format!("Malformed manifest 'package.json': {}", e))
                                .location(DiagnosticLocation::new(pkg_json.clone()))
                                .explanation("The package.json manifest contains invalid JSON syntax or corrupted structure.")
                                .remediation("Validate and correct JSON syntax in package.json.")
                                .build(),
                        );
                            None
                        }
                    }
                }
                Err(e) => {
                    diags.push(
                    Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Node)
                        .message(format!("Unreadable manifest 'package.json': {}", e))
                        .location(DiagnosticLocation::new(pkg_json.clone()))
                        .explanation("The package.json file could not be read due to I/O permissions or encoding issues.")
                        .remediation("Check file read permissions and ensure file is valid UTF-8.")
                        .build(),
                );
                    None
                }
            }
        }
    } else {
        None
    };

    let pnpm_ws = root.join("pnpm-workspace.yaml");
    let lerna_json = root.join("lerna.json");
    let bunfig_toml = root.join("bunfig.toml");

    let members = discover_workspace_members(root, pkg_val.as_ref());

    if info.is_none()
        && (!members.is_empty()
            || pnpm_ws.is_file()
            || lerna_json.is_file()
            || bunfig_toml.is_file())
    {
        let pm = if pnpm_ws.is_file() {
            "pnpm".to_string()
        } else if bunfig_toml.is_file() || root.join("bun.lock").is_file() {
            "bun".to_string()
        } else {
            "npm".to_string()
        };
        info = Some(NodeProjectInfo {
            package_manager: pm,
            package_json_path: pkg_json.clone(),
            ..Default::default()
        });
    }

    let mut info = match info {
        Some(i) => i,
        None => return (None, diags),
    };

    // Populate workspace members and packages
    info.workspace_members = members;
    for m in &info.workspace_members {
        let v = m.version.clone().unwrap_or_else(|| "*".to_string());
        info.workspace_packages.entry(m.name.clone()).or_insert(v);
    }

    // Detect and parse lockfiles
    let pnpm_lock = root.join("pnpm-lock.yaml");
    let yarn_lock = root.join("yarn.lock");
    let npm_lock = root.join("package-lock.json");
    let bun_text_lock = root.join("bun.lock");
    let bun_lock = root.join("bun.lockb");

    if pnpm_lock.is_file() {
        info.lock_file = Some(pnpm_lock.clone());
        info.package_manager = "pnpm".to_string();
        let is_oversized = fs::metadata(&pnpm_lock)
            .map(|m| m.len() > MAX_MANIFEST_SIZE)
            .unwrap_or(false);
        if is_oversized {
            diags.push(
                Diagnostic::builder("DISC-005", Severity::Warning, LanguageTarget::Node)
                    .message("Scan resource limit exceeded: 'pnpm-lock.yaml' exceeds 10 MB limit, skipping parsing")
                    .location(DiagnosticLocation::new(pnpm_lock))
                    .explanation("The pnpm-lock.yaml file exceeds the maximum allowable file size (10 MB).")
                    .remediation("Increase max_file_size or optimize the lockfile.")
                    .build(),
            );
        } else {
            match fs::read_to_string(&pnpm_lock) {
                Ok(lcontent) => match parse_pnpm_lock(&lcontent) {
                    Ok(lock_data) => {
                        info.locked_packages = lock_data.locked_packages;
                        info.locked_versions = lock_data.locked_versions;
                        for (k, v) in lock_data.workspace_packages {
                            info.workspace_packages.entry(k).or_insert(v);
                        }
                    }
                    Err(e) => {
                        diags.push(
                            Diagnostic::builder(
                                "DISC-004",
                                Severity::Warning,
                                LanguageTarget::Node,
                            )
                            .message(format!("Malformed lockfile 'pnpm-lock.yaml': {}", e))
                            .location(DiagnosticLocation::new(pnpm_lock))
                            .explanation("The pnpm-lock.yaml file contains invalid YAML syntax.")
                            .remediation("Regenerate pnpm-lock.yaml using 'pnpm install'.")
                            .build(),
                        );
                    }
                },
                Err(e) => {
                    diags.push(
                        Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Node)
                            .message(format!("Unreadable lockfile 'pnpm-lock.yaml': {}", e))
                            .location(DiagnosticLocation::new(pnpm_lock))
                            .explanation("The pnpm-lock.yaml file could not be read.")
                            .remediation("Check file read permissions.")
                            .build(),
                    );
                }
            }
        }
    } else if yarn_lock.is_file() {
        info.lock_file = Some(yarn_lock.clone());
        info.package_manager = "yarn".to_string();
        let is_oversized = fs::metadata(&yarn_lock)
            .map(|m| m.len() > MAX_MANIFEST_SIZE)
            .unwrap_or(false);
        if is_oversized {
            diags.push(
                Diagnostic::builder("DISC-005", Severity::Warning, LanguageTarget::Node)
                    .message("Scan resource limit exceeded: 'yarn.lock' exceeds 10 MB limit, skipping parsing")
                    .location(DiagnosticLocation::new(yarn_lock))
                    .explanation("The yarn.lock file exceeds the maximum allowable file size (10 MB).")
                    .remediation("Increase max_file_size or optimize the lockfile.")
                    .build(),
            );
        } else {
            match fs::read_to_string(&yarn_lock) {
                Ok(lcontent) => {
                    let lock_data = parse_yarn_lock(&lcontent);
                    info.locked_packages = lock_data.locked_packages;
                    info.locked_versions = lock_data.locked_versions;
                }
                Err(e) => {
                    diags.push(
                        Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Node)
                            .message(format!("Unreadable lockfile 'yarn.lock': {}", e))
                            .location(DiagnosticLocation::new(yarn_lock))
                            .explanation("The yarn.lock file could not be read.")
                            .remediation("Check file read permissions.")
                            .build(),
                    );
                }
            }
        }
    } else if npm_lock.is_file() {
        info.lock_file = Some(npm_lock.clone());
        info.package_manager = "npm".to_string();
        let is_oversized = fs::metadata(&npm_lock)
            .map(|m| m.len() > MAX_MANIFEST_SIZE)
            .unwrap_or(false);
        if is_oversized {
            diags.push(
                Diagnostic::builder("DISC-005", Severity::Warning, LanguageTarget::Node)
                    .message("Scan resource limit exceeded: 'package-lock.json' exceeds 10 MB limit, skipping parsing")
                    .location(DiagnosticLocation::new(npm_lock))
                    .explanation("The package-lock.json file exceeds the maximum allowable file size (10 MB).")
                    .remediation("Increase max_file_size or optimize the lockfile.")
                    .build(),
            );
        } else {
            match fs::read_to_string(&npm_lock) {
                Ok(lcontent) => match parse_npm_lock(&lcontent) {
                    Ok(lock_data) => {
                        info.locked_packages = lock_data.locked_packages;
                        info.locked_versions = lock_data.locked_versions;
                        for (k, v) in lock_data.workspace_packages {
                            info.workspace_packages.entry(k).or_insert(v);
                        }
                    }
                    Err(e) => {
                        diags.push(
                            Diagnostic::builder(
                                "DISC-004",
                                Severity::Warning,
                                LanguageTarget::Node,
                            )
                            .message(format!("Malformed lockfile 'package-lock.json': {}", e))
                            .location(DiagnosticLocation::new(npm_lock))
                            .explanation("The package-lock.json file contains invalid JSON syntax.")
                            .remediation("Regenerate package-lock.json using 'npm install'.")
                            .build(),
                        );
                    }
                },
                Err(e) => {
                    diags.push(
                        Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Node)
                            .message(format!("Unreadable lockfile 'package-lock.json': {}", e))
                            .location(DiagnosticLocation::new(npm_lock))
                            .explanation("The package-lock.json file could not be read.")
                            .remediation("Check file read permissions.")
                            .build(),
                    );
                }
            }
        }
    } else if bun_text_lock.is_file() {
        info.lock_file = Some(bun_text_lock.clone());
        info.package_manager = "bun".to_string();
        match fs::read_to_string(&bun_text_lock) {
            Ok(lcontent) => {
                let lock_data = parse_bun_lock(&lcontent);
                info.locked_packages = lock_data.locked_packages;
                info.locked_versions = lock_data.locked_versions;
                for (k, v) in lock_data.workspace_packages {
                    info.workspace_packages.entry(k).or_insert(v);
                }
            }
            Err(e) => {
                diags.push(
                    Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Node)
                        .message(format!("Unreadable lockfile 'bun.lock': {}", e))
                        .location(DiagnosticLocation::new(bun_text_lock))
                        .explanation("The bun.lock file could not be read.")
                        .remediation("Check file read permissions.")
                        .build(),
                );
            }
        }
    } else if bun_lock.is_file() {
        info.lock_file = Some(bun_lock);
        info.package_manager = "bun".to_string();
    }

    // Detect tsconfig.json / jsconfig.json paths
    let tsconfig_candidates = [
        "tsconfig.json",
        "jsconfig.json",
        "tsconfig.base.json",
        "tsconfig.app.json",
    ];
    for file_name in tsconfig_candidates {
        let ts_path = root.join(file_name);
        if ts_path.is_file() {
            match fs::read_to_string(&ts_path) {
                Ok(ts_content) => {
                    let clean = strip_json_comments(&ts_content);
                    if serde_json::from_str::<serde_json::Value>(&clean).is_err() {
                        diags.push(
                            Diagnostic::builder("DISC-004", Severity::Warning, LanguageTarget::Node)
                                .message(format!("Malformed config file '{}': invalid JSON", file_name))
                                .location(DiagnosticLocation::new(ts_path.clone()))
                                .explanation("The TypeScript/JavaScript config file contains invalid JSON syntax.")
                                .remediation("Validate JSON syntax in tsconfig/jsconfig.")
                                .build(),
                        );
                    } else {
                        let (paths, base_url) = parse_tsconfig(&ts_content);
                        if !paths.is_empty() || base_url.is_some() {
                            info.tsconfig_paths = paths;
                            info.tsconfig_base_url = base_url;
                            break;
                        }
                    }
                }
                Err(e) => {
                    diags.push(
                        Diagnostic::builder("DISC-001", Severity::Warning, LanguageTarget::Node)
                            .message(format!("Unreadable config file '{}': {}", file_name, e))
                            .location(DiagnosticLocation::new(ts_path.clone()))
                            .explanation("The config file could not be read due to I/O permissions or encoding issues.")
                            .remediation("Check file read permissions.")
                            .build(),
                    );
                }
            }
        }
    }

    (Some(info), diags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_name() {
        assert_eq!(extract_package_name("react"), Some("react".to_string()));
        assert_eq!(
            extract_package_name("lodash/fp/map"),
            Some("lodash".to_string())
        );
        assert_eq!(
            extract_package_name("@tanstack/react-query"),
            Some("@tanstack/react-query".to_string())
        );
        assert_eq!(
            extract_package_name("@tanstack/react-query/devtools"),
            Some("@tanstack/react-query".to_string())
        );
        assert_eq!(extract_package_name("./components/Button"), None);
        assert_eq!(extract_package_name("../utils"), None);
    }

    #[test]
    fn test_parse_package_json() {
        let content = r#"{
            "name": "web-app",
            "packageManager": "pnpm@9.1.0",
            "engines": {
                "node": ">=20.0.0"
            },
            "dependencies": {
                "react": "^18.2.0",
                "next": "^14.2.0"
            },
            "devDependencies": {
                "typescript": "^5.4.0",
                "vitest": "^1.4.0"
            }
        }"#;

        let info = parse_package_json(content, Path::new("package.json")).unwrap();
        assert_eq!(info.package_manager, "pnpm");
        assert_eq!(info.node_version_constraint.as_deref(), Some(">=20.0.0"));
        assert_eq!(
            info.dependencies.get("react").map(|s| s.as_str()),
            Some("^18.2.0")
        );
        assert_eq!(
            info.dev_dependencies.get("vitest").map(|s| s.as_str()),
            Some("^1.4.0")
        );
    }

    #[test]
    fn test_parse_node_imports() {
        let code = r#"
import React, { useState } from 'react';
import type { User } from './types';
import { Button } from '@mui/material';
const x = 1;
"#;
        let imports = parse_node_imports(code, Path::new("test.tsx"));
        let specs: Vec<&str> = imports.iter().map(|i| i.specifier.as_str()).collect();
        assert!(specs.contains(&"react"));
        assert!(specs.contains(&"./types"));
        assert!(specs.contains(&"@mui/material"));

        let type_import = imports.iter().find(|i| i.specifier == "./types").unwrap();
        assert!(type_import.is_type_only);
    }

    #[test]
    fn test_parse_dynamic_commonjs_and_reexports() {
        let code = r#"
export * from 'ramda';
export { helper } from './helpers';
import type { Config } from './config';

async function load() {
    const _ = await import('lodash');
    const util = require('./util');
    const path = require.resolve('path-browserify');
}
"#;
        let imports = parse_node_imports(code, Path::new("index.ts"));
        let specs: Vec<(&str, bool, bool)> = imports
            .iter()
            .map(|i| (i.specifier.as_str(), i.is_dynamic, i.is_type_only))
            .collect();

        assert!(specs.contains(&("ramda", false, false)));
        assert!(specs.contains(&("./helpers", false, false)));
        assert!(specs.contains(&("./config", false, true)));
        assert!(specs.contains(&("lodash", true, false)));
        assert!(specs.contains(&("./util", true, false)));
        assert!(specs.contains(&("path-browserify", true, false)));
    }

    #[test]
    fn test_extract_package_name_aliases() {
        assert_eq!(extract_package_name("@/components/Button"), None);
        assert_eq!(extract_package_name("~/utils/format"), None);
        assert_eq!(
            extract_package_name("@mui/material/Button"),
            Some("@mui/material".to_string())
        );
    }

    #[test]
    fn test_parse_tsconfig_with_comments() {
        let tsconfig = r#"{
            // Compiler options for project
            "compilerOptions": {
                "baseUrl": ".",
                /* Path mapping aliases */
                "paths": {
                    "@/*": ["src/*"],
                    "~components/*": ["./src/components/*"],
                    "@config": ["src/config.ts"],
                },
            },
        }"#;

        let (paths, base_url) = parse_tsconfig(tsconfig);
        assert_eq!(base_url, Some(".".to_string()));
        assert_eq!(paths.get("@/*"), Some(&vec!["src/*".to_string()]));
        assert_eq!(
            paths.get("~components/*"),
            Some(&vec!["./src/components/*".to_string()])
        );
        assert_eq!(
            paths.get("@config"),
            Some(&vec!["src/config.ts".to_string()])
        );

        assert!(is_tsconfig_alias("@/components/Card", &paths));
        assert!(is_tsconfig_alias("~components/Card", &paths));
        assert!(is_tsconfig_alias("@config", &paths));
        assert!(!is_tsconfig_alias("react", &paths));
        assert!(!is_tsconfig_alias("@mui/material", &paths));
    }
}
