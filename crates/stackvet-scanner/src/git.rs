use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use stackvet_core::Diagnostic;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileDiffKind {
    Added,
    Modified,
    Deleted,
    Renamed { from: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub kind: FileDiffKind,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub changed_lines: Vec<RangeInclusive<usize>>,
}

impl ChangedFile {
    pub fn new(path: PathBuf, kind: FileDiffKind) -> Self {
        Self {
            path,
            kind,
            changed_lines: Vec::new(),
        }
    }

    pub fn touches_line(&self, line: usize) -> bool {
        if self.changed_lines.is_empty() {
            // If line information is empty (e.g. newly added file or whole file changed), consider it touching
            return true;
        }
        self.changed_lines.iter().any(|r| r.contains(&line))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitDiffSummary {
    pub base_ref: String,
    pub head_ref: String,
    pub merge_base: Option<String>,
    pub is_shallow: bool,
    pub changed_files: Vec<ChangedFile>,
}

/// Checks whether a directory is inside a Git repository.
pub fn is_git_repository(root: &Path) -> bool {
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(root)
        .output();

    match output {
        Ok(out) => out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "true",
        Err(_) => false,
    }
}

/// Checks if repository has shallow history.
pub fn is_shallow_repository(root: &Path) -> bool {
    let output = Command::new("git")
        .args(["rev-parse", "--is-shallow-repository"])
        .current_dir(root)
        .output();

    match output {
        Ok(out) => out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "true",
        Err(_) => false,
    }
}

/// Discovers the common ancestor merge-base between base and head refs.
pub fn get_merge_base(root: &Path, base: &str, head: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["merge-base", base, head])
        .current_dir(root)
        .output()
        .with_context(|| format!("Failed to execute 'git merge-base {base} {head}'"))?;

    if output.status.success() {
        let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !hash.is_empty() {
            return Ok(hash);
        }
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    anyhow::bail!(
        "Unable to determine common ancestor merge-base between '{}' and '{}'{}. Ensure git history is available (e.g. fetch-depth: 0 in CI).",
        base,
        head,
        if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        }
    )
}

/// Parses git diff --name-status output into ChangedFile items.
pub fn parse_name_status(output: &str, root: &Path) -> Vec<ChangedFile> {
    let mut files = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.is_empty() {
            continue;
        }

        let status = parts[0];
        if status.starts_with('R') && parts.len() >= 3 {
            // Renamed: R100\told\tnew
            let old_path = root.join(parts[1]);
            let new_path = root.join(parts[2]);
            files.push(ChangedFile::new(
                new_path,
                FileDiffKind::Renamed { from: old_path },
            ));
        } else if parts.len() >= 2 {
            let path = root.join(parts[1]);
            let kind = match status.chars().next().unwrap_or('M') {
                'A' => FileDiffKind::Added,
                'D' => FileDiffKind::Deleted,
                _ => FileDiffKind::Modified,
            };
            files.push(ChangedFile::new(path, kind));
        }
    }

    files
}

/// Parses unified diff text into line ranges per file.
pub fn parse_unified_diff_hunks(
    diff_content: &str,
    root: &Path,
) -> HashMap<PathBuf, Vec<RangeInclusive<usize>>> {
    let mut map: HashMap<PathBuf, Vec<RangeInclusive<usize>>> = HashMap::new();
    let mut current_file: Option<PathBuf> = None;

    for line in diff_content.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            let raw = rest.trim().trim_matches('"');
            if raw == "/dev/null" {
                current_file = None;
            } else {
                let rel = if let Some(stripped) = raw.strip_prefix("b/") {
                    stripped
                } else {
                    raw
                };
                let target = root.join(rel);
                if is_within_repo_boundary(&target, root) {
                    current_file = Some(target);
                } else {
                    tracing::warn!("Diff targets path escaping repository boundary: {}", raw);
                    current_file = None;
                }
            }
        } else if line.starts_with("@@ ") {
            if let Some(target) = &current_file {
                // Parse @@ -a,b +start,count @@ or @@ -a,b +start @@
                if let Some(plus_idx) = line.find('+') {
                    let rest = &line[plus_idx + 1..];
                    if let Some(space_idx) = rest.find(' ') {
                        let hunk_spec = &rest[..space_idx];
                        let parts: Vec<&str> = hunk_spec.split(',').collect();
                        if let Ok(start) = parts[0].parse::<usize>() {
                            let count: usize = parts
                                .get(1)
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(1);
                            let end = if count == 0 { start } else { start + count - 1 };
                            map.entry(target.clone()).or_default().push(start..=end);
                        }
                    }
                }
            }
        }
    }

    map
}

/// Collects Git diff information between base and head.
pub fn collect_git_diff(
    root: &Path,
    base_ref: Option<&str>,
    head_ref: Option<&str>,
) -> Result<GitDiffSummary> {
    if !is_git_repository(root) {
        anyhow::bail!(
            "Directory '{}' is not a Git repository. Pass --diff or --files to analyze diffs outside of Git.",
            root.display()
        );
    }

    let is_shallow = is_shallow_repository(root);

    // Auto-detect base ref: CLI arg -> GITHUB_BASE_REF -> main -> master -> HEAD~1
    let base = if let Some(b) = base_ref {
        b.to_string()
    } else if let Ok(gh_base) = std::env::var("GITHUB_BASE_REF") {
        if !gh_base.is_empty() {
            format!("origin/{gh_base}")
        } else {
            "origin/main".to_string()
        }
    } else {
        let ref_exists = |r: &str| -> bool {
            Command::new("git")
                .args(["rev-parse", "--verify", "--quiet", r])
                .current_dir(root)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
        if ref_exists("main") || ref_exists("origin/main") {
            if ref_exists("origin/main") {
                "origin/main".to_string()
            } else {
                "main".to_string()
            }
        } else if ref_exists("master") || ref_exists("origin/master") {
            if ref_exists("origin/master") {
                "origin/master".to_string()
            } else {
                "master".to_string()
            }
        } else {
            "HEAD~1".to_string()
        }
    };

    let head = head_ref.unwrap_or("HEAD");

    let merge_base = get_merge_base(root, &base, head)?;
    let compare_target = &merge_base;

    // 1. Get changed files using name-status
    let status_output = Command::new("git")
        .args(["diff", "--name-status", "-M", compare_target, head])
        .current_dir(root)
        .output()
        .with_context(|| {
            format!("Failed to run 'git diff --name-status {compare_target} {head}'")
        })?;

    if !status_output.status.success() {
        let stderr = String::from_utf8_lossy(&status_output.stderr)
            .trim()
            .to_string();
        anyhow::bail!(
            "Git diff command failed: {}",
            if stderr.is_empty() {
                "unknown error"
            } else {
                &stderr
            }
        );
    }

    let stdout_str = String::from_utf8_lossy(&status_output.stdout);
    let mut changed_files = parse_name_status(&stdout_str, root);

    // 2. Get line hunks
    let hunk_output = Command::new("git")
        .args(["diff", "-U0", compare_target, head])
        .current_dir(root)
        .output();

    if let Ok(hunk_out) = hunk_output {
        if hunk_out.status.success() {
            let hunk_str = String::from_utf8_lossy(&hunk_out.stdout);
            let hunk_map = parse_unified_diff_hunks(&hunk_str, root);
            for file in &mut changed_files {
                if let Some(ranges) = hunk_map.get(&file.path) {
                    file.changed_lines = ranges.clone();
                }
            }
        }
    }

    Ok(GitDiffSummary {
        base_ref: base,
        head_ref: head.to_string(),
        merge_base: Some(merge_base),
        is_shallow,
        changed_files,
    })
}

/// Validates that a file path is safely contained within the repository root boundary,
/// protecting against path traversal attempts (e.g. `../../etc/passwd` or outside absolute paths).
pub fn is_within_repo_boundary(candidate: &Path, root: &Path) -> bool {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let full = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    if let Ok(canon_full) = full.canonicalize() {
        canon_full.starts_with(&canonical_root)
    } else {
        // For non-existent / deleted paths, resolve lexical components
        let mut norm = PathBuf::new();
        for comp in full.components() {
            match comp {
                std::path::Component::ParentDir => {
                    norm.pop();
                }
                std::path::Component::CurDir => {}
                c => norm.push(c),
            }
        }
        norm.starts_with(&canonical_root) || norm.starts_with(root)
    }
}

/// Parses an explicit comma-separated file list argument (e.g. `--files users.py,requirements.txt`).
pub fn parse_explicit_files(files_arg: &str, root: &Path) -> Vec<ChangedFile> {
    files_arg
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let path = root.join(s);
            if is_within_repo_boundary(&path, root) {
                Some(ChangedFile::new(path, FileDiffKind::Modified))
            } else {
                tracing::warn!("Rejecting path outside repository boundary: {}", s);
                None
            }
        })
        .collect()
}

/// Parses a unified diff file from disk.
pub fn parse_patch_file(patch_path: &Path, root: &Path) -> Result<Vec<ChangedFile>> {
    let content = fs::read_to_string(patch_path)
        .with_context(|| format!("Failed to read patch file '{}'", patch_path.display()))?;

    let hunks = parse_unified_diff_hunks(&content, root);
    let mut files = Vec::new();

    for (path, lines) in hunks {
        files.push(ChangedFile {
            path,
            kind: FileDiffKind::Modified,
            changed_lines: lines,
        });
    }

    Ok(files)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrAnalysisResult {
    pub changed_files: Vec<ChangedFile>,
    pub new_diagnostics: Vec<Diagnostic>,
    pub existing_diagnostics: Vec<Diagnostic>,
    pub resolved_diagnostics: Vec<Diagnostic>,
    pub affected_checks_count: usize,
}

impl PrAnalysisResult {
    /// Compares pre-PR base diagnostics against post-PR diagnostics using git changed files.
    pub fn analyze(
        changed_files: Vec<ChangedFile>,
        head_diagnostics: &[Diagnostic],
        base_diagnostics: &[Diagnostic],
    ) -> Self {
        // Fingerprint base diagnostics for comparison
        let base_fingerprints: HashSet<String> = base_diagnostics
            .iter()
            .map(stackvet_core::BaselineData::compute_fingerprint)
            .collect();

        let head_fingerprints: HashSet<String> = head_diagnostics
            .iter()
            .map(stackvet_core::BaselineData::compute_fingerprint)
            .collect();

        let mut new_diagnostics = Vec::new();
        let mut existing_diagnostics = Vec::new();

        for d in head_diagnostics {
            let fp = stackvet_core::BaselineData::compute_fingerprint(d);

            // A diagnostic is "new" only if the base revision did not report it.
            if !base_fingerprints.contains(&fp) {
                new_diagnostics.push(d.clone());
            } else {
                existing_diagnostics.push(d.clone());
            }
        }

        // Deduplicate new_diagnostics against existing_diagnostics
        let existing_fps: HashSet<String> = existing_diagnostics
            .iter()
            .map(stackvet_core::BaselineData::compute_fingerprint)
            .collect();
        new_diagnostics.retain(|d| {
            !existing_fps.contains(&stackvet_core::BaselineData::compute_fingerprint(d))
        });

        // Resolved diagnostics are findings present in base but no longer in head
        let mut resolved_diagnostics = Vec::new();
        for bd in base_diagnostics {
            let fp = stackvet_core::BaselineData::compute_fingerprint(bd);
            if !head_fingerprints.contains(&fp) {
                resolved_diagnostics.push(bd.clone());
            }
        }

        let affected_checks_count = new_diagnostics.len() + existing_diagnostics.len();

        Self {
            changed_files,
            new_diagnostics,
            existing_diagnostics,
            resolved_diagnostics,
            affected_checks_count,
        }
    }

    pub fn is_clean(&self, strict: bool) -> bool {
        if strict {
            self.new_diagnostics.is_empty()
        } else {
            !self
                .new_diagnostics
                .iter()
                .any(|d| d.severity >= stackvet_core::Severity::Error)
        }
    }

    /// Renders text output matching docs/cli_reference.md format.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str("StackVet Pull Request Analysis\n\n");
        out.push_str(&format!("Changed files: {}\n", self.changed_files.len()));
        out.push_str(&format!(
            "Affected checks: {}\n\n",
            self.affected_checks_count
        ));

        if !self.new_diagnostics.is_empty() {
            out.push_str("New issues\n");
            out.push_str("──────────\n");
            for d in &self.new_diagnostics {
                let loc = if let Some(span) = &d.location.span {
                    format!("{}:{}", d.location.file_path.display(), span.start_line)
                } else {
                    d.location.file_path.display().to_string()
                };
                out.push_str(&format!("{}  {}\n", d.rule_id.0, loc));
                out.push_str(&format!("{}\n\n", d.message));
            }
        } else {
            out.push_str("✓ No new issues introduced.\n\n");
        }

        if !self.resolved_diagnostics.is_empty() {
            out.push_str("Resolved\n");
            out.push_str("────────\n");
            for d in &self.resolved_diagnostics {
                out.push_str(&format!("{}\n", d.rule_id.0));
            }
            out.push('\n');
        }

        out
    }

    /// Renders Markdown suitable for GitHub PR comments.
    pub fn render_markdown_comment(&self) -> String {
        let mut md = String::new();
        md.push_str("### 🔍 StackVet Pull Request Analysis\n\n");
        md.push_str(&format!(
            "- **Changed files**: {}\n",
            self.changed_files.len()
        ));
        md.push_str(&format!(
            "- **New issues**: {}\n",
            self.new_diagnostics.len()
        ));
        md.push_str(&format!(
            "- **Resolved legacy issues**: {}\n\n",
            self.resolved_diagnostics.len()
        ));

        if !self.new_diagnostics.is_empty() {
            md.push_str("#### ❌ Newly Introduced Issues\n\n");
            md.push_str("| Rule | Severity | Location | Message |\n");
            md.push_str("|---|---|---|---|\n");
            for d in &self.new_diagnostics {
                let loc = if let Some(span) = &d.location.span {
                    format!("{}:{}", d.location.file_path.display(), span.start_line)
                } else {
                    d.location.file_path.display().to_string()
                };
                md.push_str(&format!(
                    "| `{}` | **{}** | `{}` | {} |\n",
                    d.rule_id.0,
                    d.severity.as_str(),
                    loc,
                    d.message
                ));
            }
            md.push('\n');
        } else {
            md.push_str("✅ **Clean!** No new issues introduced by this pull request.\n\n");
        }

        if !self.resolved_diagnostics.is_empty() {
            md.push_str("#### 🎉 Resolved Issues\n\n");
            for d in &self.resolved_diagnostics {
                md.push_str(&format!("- `{}`: {}\n", d.rule_id.0, d.message));
            }
            md.push('\n');
        }

        md
    }

    /// Emits GitHub Actions workflow inline annotations (`::error` and `::warning`).
    pub fn render_github_annotations(&self) -> Vec<String> {
        let mut annotations = Vec::new();
        for d in &self.new_diagnostics {
            let level = match d.severity {
                stackvet_core::Severity::Critical | stackvet_core::Severity::Error => "error",
                stackvet_core::Severity::Warning => "warning",
                _ => "notice",
            };

            let file_str = d.location.file_path.to_string_lossy();
            let escaped_file = escape_workflow_property(&file_str);
            let escaped_title = escape_workflow_property(&d.rule_id.0);
            let escaped_message = escape_workflow_data(&d.message);
            let line = d.location.span.as_ref().map(|s| s.start_line).unwrap_or(1);
            let col = d.location.span.as_ref().map(|s| s.start_col).unwrap_or(1);

            annotations.push(format!(
                "::{} file={},line={},col={},title={}::{}",
                level, escaped_file, line, col, escaped_title, escaped_message
            ));
        }
        annotations
    }
}

fn escape_workflow_property(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn escape_workflow_data(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}
#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;
    use stackvet_core::Severity;

    #[test]
    fn test_parse_name_status() {
        let root = Path::new("/workspace");
        let sample = "A\tapp/new.py\nM\tapp/main.py\nD\tapp/old.py\nR100\tapp/from.py\tapp/to.py\n";
        let res = parse_name_status(sample, root);

        assert_eq!(res.len(), 4);
        assert_eq!(res[0].kind, FileDiffKind::Added);
        assert_eq!(res[1].kind, FileDiffKind::Modified);
        assert_eq!(res[2].kind, FileDiffKind::Deleted);
        match &res[3].kind {
            FileDiffKind::Renamed { from } => {
                assert_eq!(from, &root.join("app/from.py"));
            }
            _ => panic!("Expected renamed"),
        }
    }

    #[test]
    fn test_parse_unified_diff_hunks() {
        let root = Path::new("/workspace");
        let diff = r#"
--- a/app/main.py
+++ b/app/main.py
@@ -10,0 +11,5 @@
+import httpx
+print("test")
--- a/app/other.py
+++ b/app/other.py
@@ -40,3 +40,1 @@
-old_code
+new_code
"#;
        let hunks = parse_unified_diff_hunks(diff, root);
        let main_ranges = hunks.get(&root.join("app/main.py")).unwrap();
        assert_eq!(main_ranges, &[11..=15]);

        let other_ranges = hunks.get(&root.join("app/other.py")).unwrap();
        assert_eq!(other_ranges, &[40..=40]);
    }

    #[test]
    fn test_parse_unified_diff_hunks_with_deletion() {
        let root = Path::new("/workspace");
        let diff = r#"
--- a/app/main.py
+++ b/app/main.py
@@ -10,0 +11,5 @@
+import httpx
--- a/app/deleted.py
+++ /dev/null
@@ -1,5 +0,0 @@
-old_deleted_content
"#;
        let hunks = parse_unified_diff_hunks(diff, root);
        let main_ranges = hunks.get(&root.join("app/main.py")).unwrap();
        // app/main.py should only have the 11..=15 range, not the deletion range
        assert_eq!(main_ranges, &[11..=15]);
        assert!(!hunks.contains_key(&root.join("app/deleted.py")));
        assert!(!hunks.contains_key(&PathBuf::from("/dev/null")));
    }

    #[test]
    fn test_pr_analysis_distinguishes_new_and_resolved() {
        let root = Path::new("/workspace");
        let changed = vec![ChangedFile {
            path: root.join("app/users.py"),
            kind: FileDiffKind::Modified,
            changed_lines: vec![40..=45],
        }];

        let new_diag = stackvet_core::Diagnostic::builder(
            "FAP-021",
            Severity::Error,
            stackvet_core::LanguageTarget::Python,
        )
        .message("Async endpoint performs blocking I/O")
        .location(
            stackvet_core::DiagnosticLocation::new(root.join("app/users.py"))
                .with_span(stackvet_core::TextSpan::single_line(42, 1, 10)),
        )
        .build();

        let resolved_diag = stackvet_core::Diagnostic::builder(
            "SQL-008",
            Severity::Warning,
            stackvet_core::LanguageTarget::Python,
        )
        .message("Unindexed query")
        .location(
            stackvet_core::DiagnosticLocation::new(root.join("app/db.py"))
                .with_span(stackvet_core::TextSpan::single_line(20, 1, 10)),
        )
        .build();

        let head = vec![new_diag.clone()];
        let base = vec![resolved_diag.clone()];

        let result = PrAnalysisResult::analyze(changed, &head, &base);

        assert_eq!(result.new_diagnostics.len(), 1);
        assert_eq!(result.new_diagnostics[0].rule_id.0, "FAP-021");
        assert_eq!(result.resolved_diagnostics.len(), 1);
        assert_eq!(result.resolved_diagnostics[0].rule_id.0, "SQL-008");

        let text = result.render_text();
        assert!(text.contains("StackVet Pull Request Analysis"));
        assert!(text.contains("FAP-021"));
        assert!(text.contains("SQL-008"));

        let md = result.render_markdown_comment();
        assert!(md.contains("Newly Introduced Issues"));
        assert!(md.contains("Resolved Issues"));

        let annotations = result.render_github_annotations();
        assert_eq!(annotations.len(), 1);
        assert!(annotations[0].starts_with("::error file="));
    }

    #[test]
    fn test_path_traversal_rejected_in_explicit_files() {
        let root = Path::new("test_repo");
        let untrusted = "../../etc/passwd,/etc/shadow,normal/path.py";
        let filtered = parse_explicit_files(untrusted, root);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].path, root.join("normal/path.py"));
    }

    #[test]
    fn test_path_traversal_rejected_in_diff() {
        let root = Path::new("test_repo");
        let malicious_diff = r#"
--- a/../../etc/passwd
+++ b/../../etc/passwd
@@ -1,1 +1,2 @@
 root:x:0:0:root:/root:/bin/bash
+pwned:x:0:0:root:/root:/bin/bash
--- a/src/app.py
+++ b/src/app.py
@@ -10,3 +10,4 @@
 def hello():
+    print("safe")
     return True
"#;
        let files = parse_unified_diff_hunks(malicious_diff, root);
        assert_eq!(files.len(), 1);
        assert!(files.contains_key(&root.join("src/app.py")));
    }
}
