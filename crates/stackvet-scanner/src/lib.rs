use anyhow::Result;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use stackvet_core::ProjectContext;
use std::path::{Path, PathBuf};

pub mod cache;
pub mod git;

pub use cache::{
    hash_bytes, hash_file_content, normalize_rel_path, CacheInvalidationReason,
    CacheValidationResult, FileCacheEntry, IncrementalCacheManager, ScanCacheData, ScanChanges,
    CACHE_SCHEMA_VERSION, DEFAULT_CACHE_DIR, DEFAULT_CACHE_FILE,
};
pub use git::{
    collect_git_diff, get_merge_base, is_git_repository, is_shallow_repository,
    parse_explicit_files, parse_name_status, parse_patch_file, parse_unified_diff_hunks,
    ChangedFile, FileDiffKind, GitDiffSummary, PrAnalysisResult,
};

/// Sensible default directories and artifacts excluded from project scanning.
pub const DEFAULT_EXCLUSIONS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".tox",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".next",
    ".nuxt",
    ".turbo",
    ".output",
];

pub struct ProjectScanner {
    root: PathBuf,
    custom_ignores: Vec<String>,
    use_default_exclusions: bool,
    verbose: bool,
    max_file_size: u64,
    max_depth: usize,
    max_files: usize,
    follow_symlinks: bool,
    parents: bool,
    cancellation_token: Option<stackvet_core::CancellationToken>,
}

#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub source_files: Vec<PathBuf>,
    pub manifest_files: Vec<PathBuf>,
    pub config_files: Vec<PathBuf>,
    pub dockerfiles: Vec<PathBuf>,
    pub ci_workflows: Vec<PathBuf>,
    pub ignored_paths: Vec<PathBuf>,
    pub unreadable_files: Vec<(PathBuf, String)>,
    pub oversized_files: Vec<(PathBuf, u64)>,
    pub skipped_symlinks: Vec<(PathBuf, String)>,
    pub is_truncated: bool,
    pub was_cancelled: bool,
}

impl ProjectScanner {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            custom_ignores: Vec::new(),
            use_default_exclusions: true,
            verbose: false,
            max_file_size: 10 * 1024 * 1024, // 10 MB
            max_depth: 40,
            max_files: 50_000,
            follow_symlinks: false,
            parents: true,
            cancellation_token: None,
        }
    }

    /// Configures an optional cooperative cancellation token.
    pub fn with_cancellation_token(mut self, token: stackvet_core::CancellationToken) -> Self {
        self.cancellation_token = Some(token);
        self
    }

    /// Configures whether parent directories are traversed for ignore files.
    pub fn with_parents(mut self, parents: bool) -> Self {
        self.parents = parents;
        self
    }

    /// Adds custom ignore patterns (e.g. from `stackvet.toml [ignore]` or CLI `--ignore`).
    pub fn with_ignores(mut self, ignores: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.custom_ignores
            .extend(ignores.into_iter().map(Into::into));
        self
    }

    /// Configures whether built-in default exclusions (e.g. node_modules, target, .venv) are enabled.
    pub fn with_default_exclusions(mut self, use_defaults: bool) -> Self {
        self.use_default_exclusions = use_defaults;
        self
    }

    /// Enables verbose scanning mode.
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Configures maximum individual file size in bytes before skipping (prevents memory exhaustion).
    pub fn with_max_file_size(mut self, max_file_size: u64) -> Self {
        self.max_file_size = max_file_size;
        self
    }

    /// Configures maximum directory traversal depth.
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Configures maximum total files scanned.
    pub fn with_max_files(mut self, max_files: usize) -> Self {
        self.max_files = max_files;
        self
    }

    /// Configures whether symlinks should be followed (default: false for security).
    pub fn with_follow_symlinks(mut self, follow_symlinks: bool) -> Self {
        self.follow_symlinks = follow_symlinks;
        self
    }

    /// Checks whether a given path is matched by the scanner's explicit custom ignore rules.
    pub fn is_explicitly_ignored(&self, path: &Path) -> bool {
        if self.custom_ignores.is_empty() {
            return false;
        }
        let mut ov_builder = OverrideBuilder::new(&self.root);
        for pattern in &self.custom_ignores {
            let trimmed = pattern.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                continue;
            }
            let rule = format!("!{trimmed}");
            let _ = ov_builder.add(&rule);
        }
        match ov_builder.build() {
            Ok(overrides) => {
                let is_dir = path.is_dir();
                matches!(overrides.matched(path, is_dir), ignore::Match::Ignore(_))
            }
            Err(_) => false,
        }
    }

    pub fn scan(&self) -> ScanResult {
        let mut result = ScanResult::default();
        let canonical_root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());

        let mut builder = WalkBuilder::new(&self.root);
        builder.hidden(false);
        builder.parents(self.parents);
        builder.git_ignore(true);
        builder.max_depth(Some(self.max_depth));
        builder.follow_links(self.follow_symlinks);

        // Native .stackvetignore discovery across directory tree
        builder.add_custom_ignore_filename(".stackvetignore");

        // Custom glob overrides from configuration and CLI
        if !self.custom_ignores.is_empty() {
            let mut ov_builder = OverrideBuilder::new(&self.root);
            for pattern in &self.custom_ignores {
                let trimmed = pattern.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if trimmed.starts_with('!') {
                    tracing::warn!("Ignoring exclusion pattern beginning with '!': {}", trimmed);
                    continue;
                }
                let rule = format!("!{trimmed}");
                if let Err(e) = ov_builder.add(&rule) {
                    tracing::warn!("Failed to add ignore override '{}': {}", rule, e);
                }
            }
            match ov_builder.build() {
                Ok(overrides) => {
                    builder.overrides(overrides);
                }
                Err(e) => {
                    tracing::warn!("Failed to build ignore overrides: {}", e);
                }
            }
        }

        let use_defaults = self.use_default_exclusions;
        builder.filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            if use_defaults && DEFAULT_EXCLUSIONS.contains(&name.as_ref()) {
                return false;
            }
            true
        });

        let walker = builder.build();

        for entry_res in walker {
            if let Some(ref token) = self.cancellation_token {
                if token.is_cancelled() {
                    result.was_cancelled = true;
                    result.is_truncated = true;
                    break;
                }
            }

            let entry = match entry_res {
                Ok(e) => e,
                Err(err) => {
                    let (path, sanitized) = match &err {
                        ignore::Error::WithPath { path, err } => (path.clone(), err.to_string()),
                        _ => (self.root.clone(), err.to_string()),
                    };
                    result.unreadable_files.push((path, sanitized));
                    continue;
                }
            };

            let path = entry.path();

            // Security check: Verify path stays strictly within canonical repository root boundary
            if let Ok(canonical_entry) = path.canonicalize() {
                if !canonical_entry.starts_with(&canonical_root) {
                    result.skipped_symlinks.push((
                        path.to_path_buf(),
                        "Target path escapes repository boundary".to_string(),
                    ));
                    continue;
                }
            } else if entry.file_type().is_some_and(|ft| ft.is_symlink()) {
                result.skipped_symlinks.push((
                    path.to_path_buf(),
                    "Broken or unresolvable symlink".to_string(),
                ));
                continue;
            }

            // Skip non-regular files (pipes, sockets, character/block devices)
            let is_regular_file = entry.file_type().is_some_and(|ft| ft.is_file());
            if !is_regular_file {
                continue;
            }

            let path_buf = path.to_path_buf();
            let file_name = path_buf
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            // Manifests & Lockfiles (classified before size check so cache validation tracks and hashes them)
            let is_manifest = file_name == "pyproject.toml"
                || file_name == "package.json"
                || file_name == "requirements.txt"
                || file_name == "uv.lock"
                || file_name == "poetry.lock"
                || file_name == "package-lock.json"
                || file_name == "pnpm-lock.yaml"
                || file_name == "yarn.lock"
                || file_name == "bun.lock"
                || file_name == "bun.lockb"
                || file_name == "pnpm-workspace.yaml"
                || file_name == "lerna.json"
                || file_name == "bunfig.toml"
                || file_name == "Cargo.toml";

            if is_manifest {
                result.manifest_files.push(path_buf.clone());
            }

            // Check file size limits to prevent out-of-memory on huge files
            if let Ok(metadata) = entry.metadata() {
                if metadata.len() > self.max_file_size {
                    result.oversized_files.push((path_buf, metadata.len()));
                    continue;
                }
            }

            // Check Dockerfiles
            if file_name == "Dockerfile"
                || file_name.starts_with("Dockerfile.")
                || file_name.ends_with(".dockerfile")
                || file_name.starts_with("docker-compose")
            {
                result.dockerfiles.push(path_buf.clone());
            }

            // Check GitHub Actions / CI workflows
            if path_buf
                .to_string_lossy()
                .replace('\\', "/")
                .contains(".github/workflows")
                && (file_name.ends_with(".yml") || file_name.ends_with(".yaml"))
            {
                result.ci_workflows.push(path_buf.clone());
            }

            // Source files
            if let Some(ext) = path_buf.extension().and_then(|s| s.to_str()) {
                match ext {
                    "py" | "ts" | "tsx" | "js" | "jsx" => {
                        result.source_files.push(path_buf);
                    }
                    "yml" | "yaml" | "toml" | "json" => {
                        result.config_files.push(path_buf);
                    }
                    _ => {}
                }
            }

            // Check max files limit
            let total_count = result.source_files.len()
                + result.manifest_files.len()
                + result.config_files.len()
                + result.dockerfiles.len()
                + result.ci_workflows.len();
            if total_count >= self.max_files {
                result.is_truncated = true;
                break;
            }
        }

        result.source_files.sort();
        result.manifest_files.sort();
        result.config_files.sort();
        result.dockerfiles.sort();
        result.ci_workflows.sort();

        result
    }

    pub fn build_context(&self) -> Result<ProjectContext> {
        let scan = self.scan();
        let mut ctx = ProjectContext::new(self.root.clone());
        ctx.source_files = scan.source_files;
        ctx.dockerfiles = scan.dockerfiles;
        ctx.ci_workflows = scan.ci_workflows;
        ctx.manifest_files = scan.manifest_files;

        // Convert scan errors and limits into discovery diagnostics
        for (unreadable_path, err_msg) in &scan.unreadable_files {
            ctx.discovery_diagnostics.push(
                stackvet_core::Diagnostic::builder(
                    "DISC-001",
                    stackvet_core::Severity::Warning,
                    stackvet_core::LanguageTarget::General,
                )
                .message(format!("Unreadable file '{}': {}", unreadable_path.display(), err_msg))
                .location(stackvet_core::DiagnosticLocation::new(unreadable_path.clone()))
                .explanation("The file could not be read during project scanning due to I/O permissions or encoding issues.")
                .remediation("Check file read permissions and ensure file is valid UTF-8.")
                .build(),
            );
        }

        for (oversized_path, size_bytes) in &scan.oversized_files {
            let size_mb = size_bytes / (1024 * 1024);
            let limit_mb = self.max_file_size / (1024 * 1024);
            let clean_path = oversized_path.to_string_lossy().replace(r"\\?\", "");
            ctx.discovery_diagnostics.push(
                stackvet_core::Diagnostic::builder(
                    "DISC-005",
                    stackvet_core::Severity::Warning,
                    stackvet_core::LanguageTarget::General,
                )
                .message(format!(
                    "File '{}' ({} MB) exceeds maximum scanner size limit ({} MB)",
                    clean_path,
                    size_mb,
                    limit_mb
                ))
                .location(stackvet_core::DiagnosticLocation::new(oversized_path.clone()))
                .explanation("Oversized files are skipped from static AST analysis to prevent resource exhaustion.")
                .remediation("Increase max_file_size in stackvet.toml or add the file to .stackvetignore.")
                .build(),
            );
        }

        for (symlink_path, reason) in &scan.skipped_symlinks {
            let clean_symlink = symlink_path.to_string_lossy().replace(r"\\?\", "");
            ctx.discovery_diagnostics.push(
                stackvet_core::Diagnostic::builder(
                    "DISC-005",
                    stackvet_core::Severity::Warning,
                    stackvet_core::LanguageTarget::General,
                )
                .message(format!(
                    "Symlink '{}' was skipped: {}",
                    clean_symlink,
                    reason
                ))
                .location(stackvet_core::DiagnosticLocation::new(symlink_path.clone()))
                .explanation("Symlinks pointing outside the repository boundary or cyclical links are ignored to harden scanning against untrusted code.")
                .remediation("Ensure symlinks point within the repository root or replace with relative copies.")
                .build(),
            );
        }

        if scan.was_cancelled {
            ctx.discovery_diagnostics.push(
                stackvet_core::Diagnostic::builder(
                    "DISC-005",
                    stackvet_core::Severity::Warning,
                    stackvet_core::LanguageTarget::General,
                )
                .message("Scan operation was cancelled before completion (interrupted by user or timeout exceeded)")
                .location(stackvet_core::DiagnosticLocation::new(self.root.clone()))
                .explanation("The scan did not complete full traversal due to a cancellation signal (SIGINT) or configured timeout.")
                .remediation("Increase the timeout with '--timeout <seconds>' or re-run without interruption.")
                .build(),
            );
        } else if scan.is_truncated {
            ctx.discovery_diagnostics.push(
                stackvet_core::Diagnostic::builder(
                    "DISC-005",
                    stackvet_core::Severity::Warning,
                    stackvet_core::LanguageTarget::General,
                )
                .message(format!(
                    "Scan truncated after reaching maximum file limit ({} files)",
                    self.max_files
                ))
                .explanation("The project exceeds the maximum number of files configured for a single scan.")
                .remediation("Increase max_files in stackvet.toml [scanner] or use .stackvetignore to exclude large subtrees.")
                .build(),
            );
        }

        Ok(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn make_temp_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = std::env::temp_dir().join(format!(
            "stackvet_test_scanner_{}_{}_{}_{}",
            test_name,
            std::process::id(),
            unique,
            count
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_scanner_discovers_files() {
        let temp_dir = make_temp_dir("discovers");

        fs::write(temp_dir.join("main.py"), "print('hello')").unwrap();
        fs::write(temp_dir.join("index.ts"), "console.log('hi');").unwrap();
        fs::write(temp_dir.join("package.json"), "{}").unwrap();
        fs::write(temp_dir.join("pyproject.toml"), "").unwrap();
        fs::write(temp_dir.join("Dockerfile"), "FROM python:3.11").unwrap();

        let scanner = ProjectScanner::new(&temp_dir);
        let res = scanner.scan();

        assert_eq!(res.source_files.len(), 2);
        assert_eq!(res.manifest_files.len(), 2);
        assert_eq!(res.dockerfiles.len(), 1);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_scanner_default_exclusions() {
        let temp_dir = make_temp_dir("default_exclusions");

        fs::write(temp_dir.join("app.py"), "import os").unwrap();

        let node_modules = temp_dir.join("node_modules");
        fs::create_dir_all(&node_modules).unwrap();
        fs::write(node_modules.join("bad.js"), "console.log('ignored')").unwrap();

        let venv = temp_dir.join(".venv");
        fs::create_dir_all(&venv).unwrap();
        fs::write(venv.join("site.py"), "pass").unwrap();

        let target = temp_dir.join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("output.js"), "pass").unwrap();

        let scanner = ProjectScanner::new(&temp_dir);
        let res = scanner.scan();

        assert_eq!(res.source_files.len(), 1);
        assert_eq!(res.source_files[0], temp_dir.join("app.py"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_scanner_custom_ignores_and_stackvetignore() {
        let temp_dir = make_temp_dir("custom_ignores");

        fs::write(temp_dir.join("app.py"), "import os").unwrap();
        fs::write(temp_dir.join("ignored_script.py"), "pass").unwrap();

        let fixtures = temp_dir.join("fixtures");
        fs::create_dir_all(&fixtures).unwrap();
        fs::write(fixtures.join("fixture.py"), "pass").unwrap();

        // Write .stackvetignore
        fs::write(temp_dir.join(".stackvetignore"), "fixtures/**\n").unwrap();

        // With CLI custom ignore
        let scanner = ProjectScanner::new(&temp_dir).with_ignores(["ignored_script.py"]);
        let res = scanner.scan();

        assert_eq!(res.source_files.len(), 1);
        assert_eq!(res.source_files[0], temp_dir.join("app.py"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_scanner_max_file_size_limit() {
        let temp_dir = make_temp_dir("max_file_size");

        let small_file = temp_dir.join("small.py");
        fs::write(&small_file, "x = 1\n").unwrap();

        let large_file = temp_dir.join("large.py");
        fs::write(&large_file, "a".repeat(500)).unwrap();

        let scanner = ProjectScanner::new(&temp_dir).with_max_file_size(200);
        let res = scanner.scan();

        assert_eq!(res.source_files.len(), 1);
        assert_eq!(res.source_files[0], small_file);
        assert_eq!(res.oversized_files.len(), 1);
        assert_eq!(res.oversized_files[0].0, large_file);

        let ctx = scanner.build_context().unwrap();
        assert!(ctx.discovery_diagnostics.iter().any(
            |d| d.rule_id.0 == "DISC-005" && d.message.contains("exceeds maximum scanner size")
        ));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_scanner_max_files_limit() {
        let temp_dir = make_temp_dir("max_files");

        for i in 0..10 {
            fs::write(temp_dir.join(format!("file_{}.py", i)), "pass\n").unwrap();
        }

        let scanner = ProjectScanner::new(&temp_dir).with_max_files(4);
        let res = scanner.scan();

        assert!(res.is_truncated);
        assert!(res.source_files.len() <= 4);

        let ctx = scanner.build_context().unwrap();
        assert!(ctx
            .discovery_diagnostics
            .iter()
            .any(|d| d.rule_id.0 == "DISC-005" && d.message.contains("truncated")));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_scanner_max_depth_limit() {
        let temp_dir = make_temp_dir("max_depth");

        let deep_dir = temp_dir.join("a").join("b").join("c").join("d");
        fs::create_dir_all(&deep_dir).unwrap();
        fs::write(deep_dir.join("deep.py"), "pass\n").unwrap();
        fs::write(temp_dir.join("shallow.py"), "pass\n").unwrap();

        let scanner = ProjectScanner::new(&temp_dir).with_max_depth(2);
        let res = scanner.scan();

        assert!(res.source_files.contains(&temp_dir.join("shallow.py")));
        assert!(!res.source_files.contains(&deep_dir.join("deep.py")));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_scanner_cancellation_token() {
        let temp_dir = make_temp_dir("cancellation");
        for i in 0..10 {
            fs::write(temp_dir.join(format!("file_{}.py", i)), "pass\n").unwrap();
        }

        let token = stackvet_core::CancellationToken::new();
        token.cancel(); // immediately pre-cancelled

        let scanner = ProjectScanner::new(&temp_dir).with_cancellation_token(token);
        let res = scanner.scan();

        assert!(res.was_cancelled);
        assert!(res.is_truncated);

        let ctx = scanner.build_context().unwrap();
        assert!(ctx
            .discovery_diagnostics
            .iter()
            .any(|d| d.rule_id.0 == "DISC-005" && d.message.contains("cancelled")));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
