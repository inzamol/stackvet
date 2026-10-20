use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use stackvet_core::{Diagnostic, FixAction, TextSpan};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{Array, DocumentMut, Item, Value};

pub const ROLLBACK_FILENAME: &str = ".stackvet-rollback.json";

/// Extracts the raw package name from a Python PEP 508 requirement specifier
pub fn extract_python_package_name(req: &str) -> &str {
    let trimmed = req.trim();
    let end = trimmed
        .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != '.')
        .unwrap_or(trimmed.len());
    &trimmed[..end]
}

/// Normalizes Python package names according to PEP 503 (lowercase, underscores/dots to hyphens)
pub fn normalize_python_package_name(name: &str) -> String {
    name.to_lowercase().replace(['_', '.'], "-")
}

/// Injects dependency into pyproject.toml without altering existing formatting or comments
pub fn add_dependency_to_pyproject(content: &str, package: &str, version: &str) -> Result<String> {
    let mut doc: DocumentMut = content
        .parse()
        .context("Failed to parse pyproject.toml for editing: invalid TOML syntax")?;

    let norm_pkg = normalize_python_package_name(package);

    let spec = if version.is_empty() {
        package.to_string()
    } else if version.starts_with('>')
        || version.starts_with('=')
        || version.starts_with('~')
        || version.starts_with('<')
        || version.starts_with('^')
        || version.starts_with('!')
    {
        format!("{}{}", package, version)
    } else {
        format!("{}>={}", package, version)
    };

    if let Some(project) = doc.get_mut("project").and_then(Item::as_table_mut) {
        if let Some(deps_item) = project.get_mut("dependencies") {
            if let Some(deps) = deps_item.as_array_mut() {
                // Check if already present using normalized package name
                let already_has = deps.iter().any(|v| {
                    v.as_str().is_some_and(|s| {
                        normalize_python_package_name(extract_python_package_name(s)) == norm_pkg
                    })
                });
                if already_has {
                    return Ok(content.to_string());
                }

                let mut val = Value::from(spec.clone());
                // Inherit indentation and newline formatting from existing array items
                if let Some(first) = deps.iter().next() {
                    if let Some(s) = first.decor().prefix().and_then(|p| p.as_str()) {
                        if s.contains('\n') {
                            val.decor_mut().set_prefix(s);
                        }
                    }
                }
                deps.push_formatted(val);
            } else {
                anyhow::bail!(
                    "Failed to edit pyproject.toml: 'dependencies' in [project] is not an array"
                );
            }
        } else {
            let mut arr = Array::new();
            arr.push(spec);
            project.insert("dependencies", Item::Value(Value::Array(arr)));
        }
    } else if let Some(tool) = doc.get_mut("tool").and_then(Item::as_table_mut) {
        if let Some(poetry) = tool.get_mut("poetry").and_then(Item::as_table_mut) {
            if let Some(deps_item) = poetry.get_mut("dependencies") {
                if let Some(deps) = deps_item.as_table_mut() {
                    let already_has = deps
                        .iter()
                        .any(|(k, _)| normalize_python_package_name(k) == norm_pkg);
                    if already_has {
                        return Ok(content.to_string());
                    }
                    let ver_spec = if version.is_empty() { "*" } else { version };
                    deps.insert(package, Item::Value(Value::from(ver_spec)));
                } else {
                    anyhow::bail!("Failed to edit pyproject.toml: 'dependencies' in [tool.poetry] is not a table");
                }
            } else {
                let mut deps_table = toml_edit::Table::new();
                let ver_spec = if version.is_empty() { "*" } else { version };
                deps_table.insert(package, Item::Value(Value::from(ver_spec)));
                poetry.insert("dependencies", Item::Table(deps_table));
            }
        } else {
            let mut project = toml_edit::Table::new();
            let mut arr = Array::new();
            arr.push(spec);
            project.insert("dependencies", Item::Value(Value::Array(arr)));
            doc.insert("project", Item::Table(project));
        }
    } else {
        let mut project = toml_edit::Table::new();
        let mut arr = Array::new();
        arr.push(spec);
        project.insert("dependencies", Item::Value(Value::Array(arr)));
        doc.insert("project", Item::Table(project));
    }

    let modified = doc.to_string();
    let _verify: DocumentMut = modified
        .parse()
        .context("Auto-fix resulted in invalid TOML syntax; changes reverted")?;
    Ok(modified)
}

/// Adds dependency to package.json dependencies object safely and idempotently
pub fn add_dependency_to_package_json(
    content: &str,
    package: &str,
    version: &str,
) -> Result<String> {
    let mut val: serde_json::Value = serde_json::from_str(content)
        .context("Failed to parse package.json: invalid JSON syntax")?;

    let obj = val
        .as_object_mut()
        .context("package.json root must be a JSON object")?;

    let ver_spec = if version.is_empty() {
        "latest"
    } else {
        version
    };

    // Check if already in dependencies or devDependencies
    let in_deps = obj
        .get("dependencies")
        .and_then(|d| d.as_object())
        .is_some_and(|deps| deps.contains_key(package));
    let in_dev = obj
        .get("devDependencies")
        .and_then(|d| d.as_object())
        .is_some_and(|deps| deps.contains_key(package));

    if in_deps || in_dev {
        return Ok(content.to_string());
    }

    if let Some(deps_val) = obj.get("dependencies") {
        if !deps_val.is_object() {
            anyhow::bail!("'dependencies' in package.json is not an object");
        }
    }

    let deps = obj
        .entry("dependencies")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

    if let Some(deps_map) = deps.as_object_mut() {
        deps_map.insert(
            package.to_string(),
            serde_json::Value::String(ver_spec.to_string()),
        );
    }

    let mut out = serde_json::to_string_pretty(&val)?;
    if content.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }

    let _verify: serde_json::Value = serde_json::from_str(&out)
        .context("Auto-fix resulted in invalid JSON; changes reverted")?;
    Ok(out)
}

/// Appends dependency to requirements.txt idempotently while preserving comments and structure
pub fn add_dependency_to_requirements_txt(
    content: &str,
    package: &str,
    version: &str,
) -> Result<String> {
    let norm_pkg = normalize_python_package_name(package);

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let decl = extract_python_package_name(trimmed);
        if normalize_python_package_name(decl) == norm_pkg {
            return Ok(content.to_string());
        }
    }

    let spec = if version.is_empty() {
        package.to_string()
    } else if version.starts_with('>')
        || version.starts_with('=')
        || version.starts_with('~')
        || version.starts_with('<')
        || version.starts_with('^')
        || version.starts_with('!')
    {
        format!("{}{}", package, version)
    } else {
        format!("{}>={}", package, version)
    };

    let mut out = content.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&spec);
    out.push('\n');
    Ok(out)
}

/// Removes dependency from pyproject.toml without altering formatting or unrelated comments
pub fn remove_dependency_from_pyproject(content: &str, package: &str) -> Result<String> {
    let mut doc: DocumentMut = content
        .parse()
        .context("Failed to parse pyproject.toml for editing: invalid TOML syntax")?;

    let norm_pkg = normalize_python_package_name(package);

    if let Some(project) = doc.get_mut("project").and_then(Item::as_table_mut) {
        if let Some(deps_item) = project.get_mut("dependencies") {
            if let Some(deps) = deps_item.as_array_mut() {
                let mut idx_to_remove = None;
                for (idx, v) in deps.iter().enumerate() {
                    if let Some(s) = v.as_str() {
                        let pkg_name = extract_python_package_name(s);
                        if normalize_python_package_name(pkg_name) == norm_pkg {
                            idx_to_remove = Some(idx);
                            break;
                        }
                    }
                }
                if let Some(idx) = idx_to_remove {
                    deps.remove(idx);
                    return Ok(doc.to_string());
                }
            }
        }
    }

    if let Some(tool) = doc.get_mut("tool").and_then(Item::as_table_mut) {
        if let Some(poetry) = tool.get_mut("poetry").and_then(Item::as_table_mut) {
            if let Some(deps) = poetry.get_mut("dependencies").and_then(Item::as_table_mut) {
                let to_remove: Vec<String> = deps
                    .iter()
                    .filter(|(k, _)| normalize_python_package_name(k) == norm_pkg)
                    .map(|(k, _)| k.to_string())
                    .collect();
                if !to_remove.is_empty() {
                    for k in to_remove {
                        deps.remove(&k);
                    }
                    return Ok(doc.to_string());
                }
            }
        }
    }

    Ok(content.to_string())
}

/// Removes dependency from package.json while preserving indentation and ending newline
pub fn remove_dependency_from_package_json(content: &str, package: &str) -> Result<String> {
    let mut val: serde_json::Value =
        serde_json::from_str(content).context("Failed to parse package.json for editing")?;

    let mut removed = false;
    if let Some(deps) = val.get_mut("dependencies").and_then(|d| d.as_object_mut()) {
        if deps.remove(package).is_some() {
            removed = true;
        }
    }
    if let Some(dev_deps) = val
        .get_mut("devDependencies")
        .and_then(|d| d.as_object_mut())
    {
        if dev_deps.remove(package).is_some() {
            removed = true;
        }
    }

    if removed {
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        val.serialize(&mut ser)?;
        let mut res = String::from_utf8(buf)?;
        if content.ends_with('\n') && !res.ends_with('\n') {
            res.push('\n');
        }
        return Ok(res);
    }

    Ok(content.to_string())
}

/// Removes dependency from requirements.txt while preserving surrounding comments
pub fn remove_dependency_from_requirements_txt(content: &str, package: &str) -> String {
    let norm_target = normalize_python_package_name(package);
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            lines.push(line.to_string());
            continue;
        }
        let pkg_name = extract_python_package_name(trimmed);
        if normalize_python_package_name(pkg_name) == norm_target {
            continue;
        }
        lines.push(line.to_string());
    }
    let mut res = lines.join("\n");
    if content.ends_with('\n') && !res.ends_with('\n') {
        res.push('\n');
    }
    res
}

/// Verification result comparing pre-fix and post-fix diagnostics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixVerificationResult {
    pub resolved_diagnostics: Vec<Diagnostic>,
    pub unresolved_diagnostics: Vec<Diagnostic>,
    pub new_regressions: Vec<Diagnostic>,
    pub is_clean: bool,
}

impl FixVerificationResult {
    /// Compares pre-fix diagnostics and post-fix diagnostics to determine resolved issues and regressions
    pub fn verify(
        pre_diagnostics: &[Diagnostic],
        post_diagnostics: &[Diagnostic],
        targeted_fixes: &[FixAction],
    ) -> Self {
        let diag_fingerprint = |d: &Diagnostic| -> String {
            let path = d.location.file_path.to_string_lossy().replace('\\', "/");
            let line = d.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);
            format!("{}:{}:{}:{}", d.rule_id.as_str(), path, line, d.message)
        };

        // Diagnostics that had automated fixes targeted
        let targeted_pre: Vec<&Diagnostic> = pre_diagnostics
            .iter()
            .filter(|d| {
                if targeted_fixes.is_empty() {
                    d.fix.is_some()
                } else if let Some(dfix) = &d.fix {
                    targeted_fixes.iter().any(|tf| {
                        tf.file_path == dfix.file_path
                            && (tf.description == dfix.description
                                || tf.replacement == dfix.replacement)
                    }) || targeted_fixes
                        .iter()
                        .any(|tf| tf.description == dfix.description)
                } else {
                    false
                }
            })
            .collect();

        let post_fingerprints: std::collections::HashSet<String> =
            post_diagnostics.iter().map(diag_fingerprint).collect();
        let pre_fingerprints: std::collections::HashSet<String> =
            pre_diagnostics.iter().map(diag_fingerprint).collect();

        let mut resolved = Vec::new();
        let mut unresolved = Vec::new();

        for d in targeted_pre {
            let fp = diag_fingerprint(d);
            if post_fingerprints.contains(&fp) {
                unresolved.push(d.clone());
            } else {
                resolved.push(d.clone());
            }
        }

        let mut regressions = Vec::new();
        for d in post_diagnostics {
            let fp = diag_fingerprint(d);
            if !pre_fingerprints.contains(&fp) {
                regressions.push(d.clone());
            }
        }

        let is_clean = regressions.is_empty() && unresolved.is_empty();

        Self {
            resolved_diagnostics: resolved,
            unresolved_diagnostics: unresolved,
            new_regressions: regressions,
            is_clean,
        }
    }
}

/// Applies a surgical span replacement to a text document
pub fn apply_span_replacement(content: &str, span: &TextSpan, replacement: &str) -> Result<String> {
    if span.start_line == 0 || span.end_line < span.start_line {
        anyhow::bail!(
            "Invalid text span: lines {}-{}",
            span.start_line,
            span.end_line
        );
    }

    let mut char_offset = 0;
    let mut span_start_byte = None;
    let mut span_end_byte = None;

    for (idx, line) in content.split_inclusive('\n').enumerate() {
        let line_num = idx + 1;
        if line_num == span.start_line {
            let col = (span.start_col.saturating_sub(1)).min(line.len());
            span_start_byte = Some(char_offset + col);
        }
        if line_num == span.end_line {
            let col = (span.end_col.saturating_sub(1)).min(line.len());
            span_end_byte = Some(char_offset + col);
        }
        char_offset += line.len();
    }

    if span_start_byte.is_none() {
        anyhow::bail!("Span start line {} exceeds file length", span.start_line);
    }

    let start = span_start_byte.unwrap();
    let end = span_end_byte.unwrap_or(char_offset).max(start);

    if start > content.len() || end > content.len() {
        anyhow::bail!("Span byte offset exceeds content bounds");
    }

    let mut result = String::with_capacity(content.len() + replacement.len());
    result.push_str(&content[..start]);
    result.push_str(replacement);
    result.push_str(&content[end..]);
    Ok(result)
}

/// Computes the updated content for a single fix without modifying disk
pub fn compute_file_fix(content: &str, fix: &FixAction) -> Result<String> {
    let file_name = fix
        .file_path
        .file_name()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();

    let is_remove = fix.description.to_lowercase().starts_with("remove ");

    if is_remove {
        if file_name == "pyproject.toml" {
            remove_dependency_from_pyproject(content, &fix.replacement)
        } else if file_name == "package.json" {
            remove_dependency_from_package_json(content, &fix.replacement)
        } else if file_name == "requirements.txt" {
            Ok(remove_dependency_from_requirements_txt(
                content,
                &fix.replacement,
            ))
        } else if let Some(span) = &fix.span {
            apply_span_replacement(content, span, "")
        } else {
            Ok(content.to_string())
        }
    } else if file_name == "pyproject.toml" {
        add_dependency_to_pyproject(content, &fix.replacement, "")
    } else if file_name == "package.json" {
        add_dependency_to_package_json(content, &fix.replacement, "*")
    } else if file_name == "requirements.txt" {
        add_dependency_to_requirements_txt(content, &fix.replacement, "")
    } else if let Some(span) = &fix.span {
        apply_span_replacement(content, span, &fix.replacement)
    } else {
        Ok(content.to_string())
    }
}

/// Generates a unified diff comparing original content to modified content
pub fn generate_unified_diff(path: &Path, original: &str, modified: &str) -> String {
    if original == modified {
        return String::new();
    }

    let orig_lines: Vec<&str> = original.lines().collect();
    let mod_lines: Vec<&str> = modified.lines().collect();

    let path_str = path.display().to_string();
    let mut out = format!("--- a/{}\n+++ b/{}\n", path_str, path_str);

    let mut prefix = 0;
    while prefix < orig_lines.len()
        && prefix < mod_lines.len()
        && orig_lines[prefix] == mod_lines[prefix]
    {
        prefix += 1;
    }

    let mut orig_suffix = orig_lines.len();
    let mut mod_suffix = mod_lines.len();
    while orig_suffix > prefix
        && mod_suffix > prefix
        && orig_lines[orig_suffix - 1] == mod_lines[mod_suffix - 1]
    {
        orig_suffix -= 1;
        mod_suffix -= 1;
    }

    let ctx_before = prefix.saturating_sub(2);
    let ctx_after_orig = (orig_suffix + 2).min(orig_lines.len());
    let ctx_after_mod = (mod_suffix + 2).min(mod_lines.len());

    let orig_count = ctx_after_orig - ctx_before;
    let mod_count = ctx_after_mod - ctx_before;

    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        ctx_before + 1,
        orig_count,
        ctx_before + 1,
        mod_count
    ));

    for line in &orig_lines[ctx_before..prefix] {
        out.push_str(&format!(" {}\n", line));
    }
    for line in &orig_lines[prefix..orig_suffix] {
        out.push_str(&format!("-{}\n", line));
    }
    for line in &mod_lines[prefix..mod_suffix] {
        out.push_str(&format!("+{}\n", line));
    }
    for line in &orig_lines[orig_suffix..ctx_after_orig] {
        out.push_str(&format!(" {}\n", line));
    }

    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixPreview {
    pub file_path: PathBuf,
    pub description: String,
    pub original_content: String,
    pub modified_content: String,
    pub diff: String,
    pub has_changes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBackup {
    pub path: PathBuf,
    pub original_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackSnapshot {
    pub timestamp: u64,
    pub backups: Vec<FileBackup>,
    pub descriptions: Vec<String>,
    #[serde(default)]
    pub pre_diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub fixes: Vec<FixAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemediationReport {
    pub applied_count: usize,
    pub modified_files: Vec<PathBuf>,
    pub previews: Vec<FixPreview>,
}

pub struct RemediationEngine;

impl Default for RemediationEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RemediationEngine {
    pub fn new() -> Self {
        Self
    }

    /// Previews a single fix action and returns diff without writing to disk
    pub fn preview_fix(&self, fix: &FixAction) -> Result<FixPreview> {
        let path = &fix.file_path;
        if !path.is_file() {
            anyhow::bail!("Target file does not exist: {}", path.display());
        }
        let original = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let modified = compute_file_fix(&original, fix)?;
        let diff = generate_unified_diff(path, &original, &modified);
        let has_changes = original != modified;

        Ok(FixPreview {
            file_path: path.clone(),
            description: fix.description.clone(),
            original_content: original,
            modified_content: modified,
            diff,
            has_changes,
        })
    }

    /// Previews cumulative changes for multiple fix actions
    pub fn preview_fixes(&self, fixes: &[FixAction]) -> Result<Vec<FixPreview>> {
        let mut staged: std::collections::HashMap<PathBuf, (String, String)> =
            std::collections::HashMap::new();

        for fix in fixes {
            let path = &fix.file_path;
            if !path.is_file() {
                anyhow::bail!("Target file does not exist: {}", path.display());
            }
            if !staged.contains_key(path) {
                let content = fs::read_to_string(path)
                    .with_context(|| format!("Failed to read {}", path.display()))?;
                staged.insert(path.clone(), (content.clone(), content));
            }
            let (_, current) = staged.get_mut(path).unwrap();
            *current = compute_file_fix(current, fix)?;
        }

        let mut previews = Vec::new();
        for (path, (orig, modified)) in staged {
            let diff = generate_unified_diff(&path, &orig, &modified);
            let has_changes = orig != modified;
            let matching_desc = fixes
                .iter()
                .filter(|f| f.file_path == path)
                .map(|f| f.description.as_str())
                .collect::<Vec<_>>()
                .join(", ");

            previews.push(FixPreview {
                file_path: path,
                description: matching_desc,
                original_content: orig,
                modified_content: modified,
                diff,
                has_changes,
            });
        }

        Ok(previews)
    }

    /// Backwards compatible method to apply a single fix
    pub fn apply_fix(&self, fix: &FixAction) -> Result<()> {
        self.apply_fixes_transactional(std::slice::from_ref(fix), None)?;
        Ok(())
    }

    /// Transactionally applies a series of fixes. If any fix fails or produces invalid files,
    /// all already modified files are automatically rolled back.
    pub fn apply_fixes_transactional(
        &self,
        fixes: &[FixAction],
        project_root: Option<&Path>,
    ) -> Result<RemediationReport> {
        self.apply_fixes_transactional_with_diagnostics(fixes, project_root, None)
    }

    /// Transactionally applies a series of fixes, persisting pre-remediation diagnostics for verification.
    pub fn apply_fixes_transactional_with_diagnostics(
        &self,
        fixes: &[FixAction],
        project_root: Option<&Path>,
        pre_diagnostics: Option<&[Diagnostic]>,
    ) -> Result<RemediationReport> {
        if fixes.is_empty() {
            return Ok(RemediationReport::default());
        }

        // Phase 1: In-memory simulation and validation
        let mut staged: std::collections::HashMap<PathBuf, (String, String)> =
            std::collections::HashMap::new();

        for fix in fixes {
            let path = &fix.file_path;
            if !path.is_file() {
                anyhow::bail!("Target file does not exist: {}", path.display());
            }

            if !staged.contains_key(path) {
                let content = fs::read_to_string(path)
                    .with_context(|| format!("Failed to read {}", path.display()))?;
                staged.insert(path.clone(), (content.clone(), content));
            }

            let (_, current) = staged.get_mut(path).unwrap();
            *current = compute_file_fix(current, fix)
                .with_context(|| format!("Failed to compute fix for {}", path.display()))?;
        }

        // Phase 2: Disk Write with Rollback on error
        let mut modified_files = Vec::new();
        let mut written_so_far: Vec<(PathBuf, String)> = Vec::new();
        let mut backups = Vec::new();
        let mut previews = Vec::new();

        for (path, (orig_content, new_content)) in staged {
            let diff = generate_unified_diff(&path, &orig_content, &new_content);
            let has_changes = orig_content != new_content;

            previews.push(FixPreview {
                file_path: path.clone(),
                description: format!("Modified {}", path.display()),
                original_content: orig_content.clone(),
                modified_content: new_content.clone(),
                diff,
                has_changes,
            });

            if has_changes {
                backups.push(FileBackup {
                    path: path.clone(),
                    original_content: orig_content.clone(),
                });

                if let Err(write_err) = fs::write(&path, &new_content) {
                    // Rollback all already-written files in this transaction
                    for (prev_path, prev_orig) in written_so_far {
                        let _ = fs::write(prev_path, prev_orig);
                    }
                    anyhow::bail!(
                        "Failed to write to {}: {}. Successfully rolled back all modified files.",
                        path.display(),
                        write_err
                    );
                }

                written_so_far.push((path.clone(), orig_content));
                modified_files.push(path);
            }
        }

        // Phase 3: Persist rollback snapshot for manual revert
        if !backups.is_empty() {
            let root = project_root.unwrap_or_else(|| Path::new("."));
            let snapshot = RollbackSnapshot {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                backups,
                descriptions: fixes.iter().map(|f| f.description.clone()).collect(),
                pre_diagnostics: pre_diagnostics.map(|d| d.to_vec()).unwrap_or_default(),
                fixes: fixes.to_vec(),
            };
            let snapshot_path = root.join(ROLLBACK_FILENAME);
            if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
                let _ = fs::write(snapshot_path, json);
            }
        }

        Ok(RemediationReport {
            applied_count: modified_files.len(),
            modified_files,
            previews,
        })
    }

    /// Checks if a rollback session exists
    pub fn has_rollback_session(&self, project_root: &Path) -> bool {
        project_root.join(ROLLBACK_FILENAME).is_file()
    }

    /// Reads active rollback snapshot
    pub fn read_rollback_snapshot(&self, project_root: &Path) -> Result<RollbackSnapshot> {
        let snapshot_path = project_root.join(ROLLBACK_FILENAME);
        if !snapshot_path.is_file() {
            anyhow::bail!(
                "No active rollback session found in {}",
                project_root.display()
            );
        }

        let content = fs::read_to_string(&snapshot_path).with_context(|| {
            format!(
                "Failed to read rollback snapshot from {}",
                snapshot_path.display()
            )
        })?;
        let snapshot: RollbackSnapshot =
            serde_json::from_str(&content).context("Failed to parse rollback snapshot JSON")?;
        Ok(snapshot)
    }

    /// Reverts files changed in the last remediation session
    pub fn rollback_session(&self, project_root: &Path) -> Result<Vec<PathBuf>> {
        let snapshot = self.read_rollback_snapshot(project_root)?;

        let mut restored = Vec::new();
        for backup in snapshot.backups {
            fs::write(&backup.path, &backup.original_content).with_context(|| {
                format!(
                    "Failed to restore original content to {}",
                    backup.path.display()
                )
            })?;
            restored.push(backup.path);
        }

        let snapshot_path = project_root.join(ROLLBACK_FILENAME);
        let _ = fs::remove_file(&snapshot_path);
        Ok(restored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempTestDir(PathBuf);
    impl TempTestDir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "stackvet_test_{}_{}",
                name,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            let _ = fs::create_dir_all(&p);
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_add_dependency_to_pyproject() {
        let original = r#"
[project]
name = "sample"
version = "0.1.0"
dependencies = [
    "requests>=2.28.0",
]
"#;
        let modified = add_dependency_to_pyproject(original, "fastapi", "0.100.0").unwrap();
        assert!(modified.contains("\"fastapi>=0.100.0\""));
        assert!(modified.contains("\"requests>=2.28.0\""));
    }

    #[test]
    fn test_toml_comment_preservation() {
        let original = r#"# Top header comment
[project]
# Name comment
name = "sample"
version = "0.1.0" # Inline version comment

# Dependencies header
dependencies = [
    # HTTP client
    "requests>=2.28.0", # Main requests lib
]

[tool.mypy]
strict = true # Strict typing
"#;
        let modified = add_dependency_to_pyproject(original, "pydantic", "2.0").unwrap();
        assert!(modified.contains("# Top header comment"));
        assert!(modified.contains("# Name comment"));
        assert!(modified.contains("# Inline version comment"));
        assert!(modified.contains("# Dependencies header"));
        assert!(modified.contains("# HTTP client"));
        assert!(modified.contains("# Main requests lib"));
        assert!(modified.contains("# Strict typing"));
        assert!(modified.contains("\"pydantic>=2.0\""));
    }

    #[test]
    fn test_idempotency_pyproject() {
        let original = r#"[project]
name = "sample"
dependencies = [
    "requests>=2.28.0",
]
"#;
        let first_pass = add_dependency_to_pyproject(original, "fastapi", "0.100.0").unwrap();
        let second_pass = add_dependency_to_pyproject(&first_pass, "fastapi", "0.100.0").unwrap();
        assert_eq!(first_pass, second_pass);

        // Case-insensitive / PEP 503 normalization idempotency
        let third_pass = add_dependency_to_pyproject(&second_pass, "FastAPI", "").unwrap();
        assert_eq!(second_pass, third_pass);
    }

    #[test]
    fn test_package_name_prefix_matching() {
        let original = r#"[project]
name = "sample"
dependencies = [
    "pytest-cov>=4.0",
    "flask-cors>=3.0",
]
"#;
        // Adding pytest should NOT be blocked by pytest-cov
        let with_pytest = add_dependency_to_pyproject(original, "pytest", "7.0").unwrap();
        assert!(with_pytest.contains("\"pytest>=7.0\""));
        assert!(with_pytest.contains("\"pytest-cov>=4.0\""));

        // Adding flask should NOT be blocked by flask-cors
        let with_flask = add_dependency_to_pyproject(&with_pytest, "flask", "").unwrap();
        assert!(with_flask.contains("\"flask\""));
        assert!(with_flask.contains("\"flask-cors>=3.0\""));
    }

    #[test]
    fn test_malformed_pyproject_toml() {
        // Syntax error
        let bad_toml = r#"[project
name = "broken"
"#;
        let res = add_dependency_to_pyproject(bad_toml, "requests", "");
        assert!(res.is_err());

        // Dependencies is not an array
        let invalid_deps_type = r#"[project]
name = "broken"
dependencies = "not-an-array"
"#;
        let res2 = add_dependency_to_pyproject(invalid_deps_type, "requests", "");
        assert!(res2.is_err());
    }

    #[test]
    fn test_add_dependency_to_package_json() {
        let original = r#"{
  "name": "sample",
  "dependencies": {
    "react": "^18.0.0"
  }
}"#;
        let modified = add_dependency_to_package_json(original, "axios", "^1.6.0").unwrap();
        assert!(modified.contains("\"axios\": \"^1.6.0\""));
        assert!(modified.contains("\"react\": \"^18.0.0\""));
    }

    #[test]
    fn test_idempotency_package_json() {
        let original = r#"{
  "name": "sample",
  "dependencies": {
    "react": "^18.0.0"
  }
}"#;
        let first_pass = add_dependency_to_package_json(original, "axios", "^1.6.0").unwrap();
        let second_pass = add_dependency_to_package_json(&first_pass, "axios", "^1.6.0").unwrap();
        assert_eq!(first_pass, second_pass);

        // Does not overwrite existing dependency
        let already_has_react = add_dependency_to_package_json(&first_pass, "react", "*").unwrap();
        assert_eq!(first_pass, already_has_react);
    }

    #[test]
    fn test_malformed_package_json() {
        // Broken JSON
        let bad_json = r#"{ "name": "bad", dependencies: }"#;
        assert!(add_dependency_to_package_json(bad_json, "axios", "").is_err());

        // Root is array
        let arr_json = r#"["not", "an", "object"]"#;
        assert!(add_dependency_to_package_json(arr_json, "axios", "").is_err());

        // dependencies is string
        let bad_deps = r#"{ "dependencies": "react" }"#;
        assert!(add_dependency_to_package_json(bad_deps, "axios", "").is_err());
    }

    #[test]
    fn test_requirements_txt_idempotency_and_comments() {
        let original = r#"# Production requirements
requests==2.28.0
# Async server
uvicorn>=0.20.0
"#;
        let modified = add_dependency_to_requirements_txt(original, "fastapi", "0.100.0").unwrap();
        assert!(modified.contains("# Production requirements"));
        assert!(modified.contains("# Async server"));
        assert!(modified.ends_with("fastapi>=0.100.0\n"));

        // Idempotency
        let pass2 = add_dependency_to_requirements_txt(&modified, "fastapi", "0.100.0").unwrap();
        assert_eq!(modified, pass2);

        // Normalized match
        let pass3 = add_dependency_to_requirements_txt(&pass2, "FASTAPI", "").unwrap();
        assert_eq!(pass2, pass3);
    }

    #[test]
    fn test_apply_span_replacement() {
        let text = "alpha\nbravo\ncharlie\n";
        let span = TextSpan::new(2, 2, 1, 6); // replace line 2 "bravo"
        let res = apply_span_replacement(text, &span, "BETTER").unwrap();
        assert_eq!(res, "alpha\nBETTER\ncharlie\n");

        // Invalid span out of bounds
        let bad_span = TextSpan::new(10, 10, 1, 1);
        assert!(apply_span_replacement(text, &bad_span, "foo").is_err());
    }

    #[test]
    fn test_diff_preview() {
        let orig = "line 1\nline 2\nline 3\n";
        let modif = "line 1\nline 2 updated\nline 3\n";
        let diff = generate_unified_diff(Path::new("test.txt"), orig, modif);
        assert!(diff.contains("--- a/test.txt"));
        assert!(diff.contains("+++ b/test.txt"));
        assert!(diff.contains("-line 2"));
        assert!(diff.contains("+line 2 updated"));
    }

    #[test]
    fn test_transactional_rollback_on_failure() {
        let dir = TempTestDir::new("rollback_failure");
        let file1 = dir.path().join("pyproject.toml");
        let orig_content = r#"[project]
name = "sample"
dependencies = []
"#;
        fs::write(&file1, orig_content).unwrap();

        let engine = RemediationEngine::new();
        let fix1 = FixAction::new("Add fastapi", &file1, "fastapi");
        // Fix2 points to a non-existent file
        let fix2 = FixAction::new(
            "Add express",
            dir.path().join("non_existent.json"),
            "express",
        );

        let res = engine.apply_fixes_transactional(&[fix1, fix2], Some(dir.path()));
        assert!(res.is_err());

        // File 1 must remain completely unmodified
        let content_after = fs::read_to_string(&file1).unwrap();
        assert_eq!(content_after, orig_content);
    }

    #[test]
    fn test_explicit_rollback_session() {
        let dir = TempTestDir::new("rollback_session");
        let file1 = dir.path().join("pyproject.toml");
        let orig_content = r#"[project]
name = "sample"
dependencies = []
"#;
        fs::write(&file1, orig_content).unwrap();

        let engine = RemediationEngine::new();
        let fix = FixAction::new("Add fastapi", &file1, "fastapi");

        let report = engine
            .apply_fixes_transactional(&[fix], Some(dir.path()))
            .unwrap();
        assert_eq!(report.applied_count, 1);

        let content_modified = fs::read_to_string(&file1).unwrap();
        assert!(content_modified.contains("fastapi"));
        assert!(engine.has_rollback_session(dir.path()));

        // Perform rollback
        let restored = engine.rollback_session(dir.path()).unwrap();
        assert_eq!(restored.len(), 1);
        let content_restored = fs::read_to_string(&file1).unwrap();
        assert_eq!(content_restored, orig_content);
        assert!(!engine.has_rollback_session(dir.path()));
    }

    #[test]
    fn test_remove_dependency_from_pyproject() {
        let orig = r#"[project]
name = "sample"
dependencies = [
    "requests>=2.28.0",
    "unused-pkg>=1.0.0",
]
"#;
        let modified = remove_dependency_from_pyproject(orig, "unused_pkg").unwrap();
        assert!(!modified.contains("unused-pkg"));
        assert!(modified.contains("requests>=2.28.0"));

        let poetry_orig = r#"[tool.poetry.dependencies]
python = "^3.10"
requests = "^2.28.0"
unused-lib = "^1.0.0"
"#;
        let poetry_mod = remove_dependency_from_pyproject(poetry_orig, "unused_lib").unwrap();
        assert!(!poetry_mod.contains("unused-lib"));
        assert!(poetry_mod.contains("requests"));
    }

    #[test]
    fn test_remove_dependency_from_package_json() {
        let orig = r#"{
  "name": "sample",
  "dependencies": {
    "axios": "^1.0.0",
    "lodash": "^4.17.21"
  }
}
"#;
        let modified = remove_dependency_from_package_json(orig, "lodash").unwrap();
        assert!(!modified.contains("lodash"));
        assert!(modified.contains("axios"));
    }

    #[test]
    fn test_remove_dependency_from_requirements_txt() {
        let orig = "# Header\nrequests>=2.28.0\nunused-pkg==1.0.0\n# Footer\n";
        let modified = remove_dependency_from_requirements_txt(orig, "unused_pkg");
        assert!(!modified.contains("unused-pkg"));
        assert!(modified.contains("requests>=2.28.0"));
        assert!(modified.contains("# Header"));
        assert!(modified.contains("# Footer"));
    }

    #[test]
    fn test_fix_verification_result() {
        use stackvet_core::{Diagnostic, DiagnosticLocation, RuleId, Severity};

        let diag1 = Diagnostic {
            rule_id: RuleId::new("DEP-002"),
            message: "Missing requests".to_string(),
            severity: Severity::Error,
            location: DiagnosticLocation::new("main.py"),
            fix: Some(FixAction::new("Add requests", "pyproject.toml", "requests")),
            ..Default::default()
        };
        let diag2 = Diagnostic {
            rule_id: RuleId::new("NDEP-002"),
            message: "Unused lodash".to_string(),
            severity: Severity::Warning,
            location: DiagnosticLocation::new("package.json"),
            fix: Some(FixAction::new("Remove lodash", "package.json", "lodash")),
            ..Default::default()
        };

        let pre = vec![diag1.clone(), diag2.clone()];
        // Post fix: diag1 is resolved, diag2 is still unresolved, and no new regressions
        let post = vec![diag2.clone()];
        let targeted = vec![diag1.fix.clone().unwrap(), diag2.fix.clone().unwrap()];

        let res = FixVerificationResult::verify(&pre, &post, &targeted);
        assert_eq!(res.resolved_diagnostics.len(), 1);
        assert_eq!(res.resolved_diagnostics[0].rule_id.0, "DEP-002");
        assert_eq!(res.unresolved_diagnostics.len(), 1);
        assert_eq!(res.unresolved_diagnostics[0].rule_id.0, "NDEP-002");
        assert!(res.new_regressions.is_empty());
        assert!(!res.is_clean); // not clean because diag2 unresolved
    }

    #[test]
    fn test_remove_dependency_from_poetry_with_project_table() {
        let orig = r#"[project]
name = "hybrid-app"
version = "0.1.0"

[tool.poetry.dependencies]
python = "^3.11"
unused-poetry-pkg = "^1.2.0"
requests = "^2.31.0"
"#;
        let modified = remove_dependency_from_pyproject(orig, "unused-poetry-pkg").unwrap();
        assert!(!modified.contains("unused-poetry-pkg"));
        assert!(modified.contains("requests = \"^2.31.0\""));
        assert!(modified.contains("name = \"hybrid-app\""));
    }
}
