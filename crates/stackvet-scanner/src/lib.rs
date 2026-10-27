use anyhow::Result;
use ignore::gitignore::GitignoreBuilder;
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
    ".stackvet",
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
    pub distinct_files_count: usize,
}

impl ScanResult {
    /// Returns the count of distinct classified files across all categories.
    pub fn total_distinct_files(&self) -> usize {
        let mut set = std::collections::HashSet::new();
        set.extend(self.source_files.iter());
        set.extend(self.manifest_files.iter());
        set.extend(self.config_files.iter());
        set.extend(self.dockerfiles.iter());
        set.extend(self.ci_workflows.iter());
        set.len()
    }
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

    /// Configures maximum distinct files scanned.
    pub fn with_max_files(mut self, max_files: usize) -> Self {
        self.max_files = max_files;
        self
    }

    /// Configures whether symlinks should be followed (default: false for security).
    pub fn with_follow_symlinks(mut self, follow_symlinks: bool) -> Self {
        self.follow_symlinks = follow_symlinks;
        self
    }

    /// Builds a Gitignore matcher from configured custom ignore rules (from configuration and CLI).
    pub fn build_custom_gitignore(&self) -> Option<ignore::gitignore::Gitignore> {
        if self.custom_ignores.is_empty() {
            return None;
        }
        let mut builder = GitignoreBuilder::new(&self.root);
        for pattern in &self.custom_ignores {
            let trimmed = pattern.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Err(e) = builder.add_line(None, trimmed) {
                tracing::warn!("Failed to add ignore pattern '{}': {}", trimmed, e);
            }
        }
        match builder.build() {
            Ok(gi) => Some(gi),
            Err(e) => {
                tracing::warn!("Failed to build custom gitignore matcher: {}", e);
                None
            }
        }
    }

    fn is_under_root(&self, path: &Path) -> bool {
        if !path.has_root() {
            return true;
        }
        if path.starts_with(&self.root) {
            return true;
        }
        let Ok(canonical_path) = path.canonicalize() else {
            return false;
        };
        let Ok(canonical_root) = self.root.canonicalize() else {
            return false;
        };
        canonical_path.starts_with(canonical_root)
    }

    /// Checks whether a given path is matched by the scanner's explicit custom ignore rules.
    pub fn is_explicitly_ignored(&self, path: &Path) -> bool {
        if path.has_root() && !self.is_under_root(path) {
            return false;
        }
        let Some(gi) = self.build_custom_gitignore() else {
            return false;
        };
        let is_dir = path.is_dir();
        gi.matched_path_or_any_parents(path, is_dir).is_ignore()
    }

    /// Checks whether a given path is accepted as a valid source file by the scanner,
    /// adhering to default exclusions, file size limits, custom ignores, and ignore files.
    pub fn is_accepted_source_file(&self, path: &Path) -> bool {
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        if !meta.is_file() || meta.len() > self.max_file_size {
            return false;
        }

        let is_source = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|ext| matches!(ext, "py" | "ts" | "tsx" | "js" | "jsx"))
            .unwrap_or(false);
        if !is_source {
            return false;
        }

        if self.use_default_exclusions {
            for comp in path.components() {
                let s = comp.as_os_str().to_string_lossy();
                if DEFAULT_EXCLUSIONS.contains(&s.as_ref()) {
                    return false;
                }
            }
        }

        let under_root = !path.has_root() || self.is_under_root(path);

        if under_root {
            // Check custom ignores first (highest priority)
            if let Some(gi) = self.build_custom_gitignore() {
                if gi.matched_path_or_any_parents(path, false).is_ignore() {
                    return false;
                }
            }

            // Check .stackvetignore files from nearest directory outward
            let mut matchers = Vec::new();
            if self.parents {
                let mut cur = path.parent();
                while let Some(parent) = cur {
                    let sv_ignore = parent.join(".stackvetignore");
                    if sv_ignore.is_file() {
                        let (gi, _err) = ignore::gitignore::Gitignore::new(&sv_ignore);
                        matchers.push(gi);
                    }
                    if parent == self.root {
                        break;
                    }
                    cur = parent.parent();
                }
            }
            for gi in &matchers {
                let matched = gi.matched_path_or_any_parents(path, false);
                if matched.is_ignore() {
                    return false;
                }
                if matched.is_whitelist() {
                    break;
                }
            }
        }

        let mut builder = WalkBuilder::new(path);
        builder.hidden(false);
        builder.parents(self.parents);
        builder.git_ignore(true);
        builder.add_custom_ignore_filename(".stackvetignore");
        let custom_gi = if under_root {
            self.build_custom_gitignore()
        } else {
            None
        };
        builder.filter_entry(move |entry| {
            if let Some(ref gi) = custom_gi {
                let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
                if gi
                    .matched_path_or_any_parents(entry.path(), is_dir)
                    .is_ignore()
                {
                    return false;
                }
            }
            true
        });
        let walker = builder.build();
        let mut count = 0;
        for entry in walker.flatten() {
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                count += 1;
            }
        }
        count > 0
    }

    pub fn scan(&self) -> ScanResult {
        let mut result = ScanResult::default();
        if self.max_files == 0 {
            result.is_truncated = true;
            return result;
        }

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

        let custom_gi = self.build_custom_gitignore();
        let use_defaults = self.use_default_exclusions;
        builder.filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            if use_defaults && DEFAULT_EXCLUSIONS.contains(&name.as_ref()) {
                return false;
            }
            if let Some(ref gi) = custom_gi {
                let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
                if gi
                    .matched_path_or_any_parents(entry.path(), is_dir)
                    .is_ignore()
                {
                    return false;
                }
            }
            true
        });

        let walker = builder.build();
        let mut distinct_scanned_files = std::collections::HashSet::new();

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

            let mut is_classified = false;

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
                is_classified = true;
            }

            // Check file size limits to prevent out-of-memory on huge files
            if let Ok(metadata) = entry.metadata() {
                if metadata.len() > self.max_file_size {
                    result
                        .oversized_files
                        .push((path_buf.clone(), metadata.len()));
                    if is_classified {
                        distinct_scanned_files.insert(path_buf);
                        if distinct_scanned_files.len() >= self.max_files {
                            result.is_truncated = true;
                            break;
                        }
                    }
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
                is_classified = true;
            }

            // Check GitHub Actions / CI workflows
            if path_buf
                .to_string_lossy()
                .replace('\\', "/")
                .contains(".github/workflows")
                && (file_name.ends_with(".yml") || file_name.ends_with(".yaml"))
            {
                result.ci_workflows.push(path_buf.clone());
                is_classified = true;
            }

            // Source files
            if let Some(ext) = path_buf.extension().and_then(|s| s.to_str()) {
                match ext {
                    "py" | "ts" | "tsx" | "js" | "jsx" => {
                        result.source_files.push(path_buf.clone());
                        is_classified = true;
                    }
                    "yml" | "yaml" | "toml" | "json" => {
                        result.config_files.push(path_buf.clone());
                        is_classified = true;
                    }
                    _ => {}
                }
            }

            // Check max files limit based on distinct classified files
            if is_classified {
                distinct_scanned_files.insert(path_buf);
                if distinct_scanned_files.len() >= self.max_files {
                    result.is_truncated = true;
                    break;
                }
            }
        }

        result.distinct_files_count = distinct_scanned_files.len();
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
                    "Scan truncated after reaching maximum file limit ({} distinct files)",
                    self.max_files
                ))
                .explanation("The project exceeds the maximum number of distinct files configured for a single scan.")
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
    fn test_scanner_custom_ignores_polarity_and_negations() {
        let temp_dir = make_temp_dir("polarity_and_negations");

        // Ordinary source files that should be scanned
        let app_file = temp_dir.join("app.py");
        let src_dir = temp_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let ts_file = src_dir.join("index.ts");
        fs::write(&app_file, "import sys\n").unwrap();
        fs::write(&ts_file, "console.log('hello');\n").unwrap();

        // Generated files targeted by custom ignore pattern
        let gen_dir = temp_dir.join("generated");
        let gen_nested = gen_dir.join("nested");
        fs::create_dir_all(&gen_nested).unwrap();
        let gen1_file = gen_dir.join("gen1.py");
        let gen2_file = gen_nested.join("gen2.py");
        let gen_keep_file = gen_dir.join("keep.py");
        fs::write(&gen1_file, "x = 1\n").unwrap();
        fs::write(&gen2_file, "y = 2\n").unwrap();
        fs::write(&gen_keep_file, "z = 3\n").unwrap();

        // Vendor files handled via .stackvetignore with negation
        let vendor_dir = temp_dir.join("vendor");
        fs::create_dir_all(&vendor_dir).unwrap();
        let vendor_ignored = vendor_dir.join("vendored.py");
        let vendor_keep = vendor_dir.join("keep_vendor.py");
        fs::write(&vendor_ignored, "v = 1\n").unwrap();
        fs::write(&vendor_keep, "vk = 2\n").unwrap();
        fs::write(
            temp_dir.join(".stackvetignore"),
            "vendor/**\n!vendor/keep_vendor.py\n",
        )
        .unwrap();

        // Nested subpackage with its own .stackvetignore containing ignore rule and negation
        let nested_pkg = temp_dir.join("subpkg");
        fs::create_dir_all(&nested_pkg).unwrap();
        let nested_ignored = nested_pkg.join("temp.py");
        let nested_keep = nested_pkg.join("keep.py");
        fs::write(&nested_ignored, "t = 1\n").unwrap();
        fs::write(&nested_keep, "k = 2\n").unwrap();
        fs::write(nested_pkg.join(".stackvetignore"), "*.py\n!keep.py\n").unwrap();

        let scanner =
            ProjectScanner::new(&temp_dir).with_ignores(["generated/**", "!generated/keep.py"]);

        // Acceptance criterion: Correct override polarity in is_explicitly_ignored
        assert!(scanner.is_explicitly_ignored(&gen1_file));
        assert!(scanner.is_explicitly_ignored(&gen2_file));
        assert!(!scanner.is_explicitly_ignored(&gen_keep_file));
        assert!(!scanner.is_explicitly_ignored(&app_file));
        assert!(!scanner.is_explicitly_ignored(&ts_file));

        // Acceptance criterion: Correct override polarity in is_accepted_source_file
        assert!(scanner.is_accepted_source_file(&app_file));
        assert!(scanner.is_accepted_source_file(&ts_file));
        assert!(scanner.is_accepted_source_file(&gen_keep_file));

        assert!(scanner.is_accepted_source_file(&vendor_keep));
        assert!(!scanner.is_accepted_source_file(&gen1_file));
        assert!(!scanner.is_accepted_source_file(&gen2_file));
        assert!(!scanner.is_accepted_source_file(&vendor_ignored));

        assert!(scanner.is_accepted_source_file(&nested_keep));
        assert!(!scanner.is_accepted_source_file(&nested_ignored));

        // Acceptance criterion: scan() excludes matching source files and scans ordinary files
        let res = scanner.scan();

        assert!(
            res.source_files.contains(&app_file),
            "Ordinary source file app.py must be scanned"
        );
        assert!(
            res.source_files.contains(&ts_file),
            "Ordinary source file index.ts must be scanned"
        );
        assert!(
            res.source_files.contains(&gen_keep_file),
            "Whitelisted file keep.py must be scanned via negation pattern"
        );
        assert!(
            res.source_files.contains(&vendor_keep),
            "Whitelisted file keep_vendor.py must be scanned via .stackvetignore negation"
        );
        assert!(
            res.source_files.contains(&nested_keep),
            "Nested whitelisted file keep.py must be scanned"
        );
        assert!(
            !res.source_files.contains(&nested_ignored),
            "Nested ignored file temp.py must not be scanned"
        );
        assert!(
            !res.source_files.contains(&gen1_file),
            "Excluded file gen1.py must not be scanned"
        );
        assert!(
            !res.source_files.contains(&gen2_file),
            "Excluded file gen2.py must not be scanned"
        );
        assert!(
            !res.source_files.contains(&vendor_ignored),
            "Excluded file vendored.py must not be scanned"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_scanner_outside_root_paths_not_falsely_ignored() {
        let temp_dir = make_temp_dir("outside_root_main");
        let outside_dir = make_temp_dir("outside_root_other");

        let outside_file = outside_dir.join("generated_outside.py");
        fs::write(&outside_file, "print('outside')\n").unwrap();

        let scanner = ProjectScanner::new(&temp_dir).with_ignores(["generated*/**", "generated*"]);

        // Absolute path outside the scanner root should not be falsely ignored by custom ignores
        assert!(!scanner.is_explicitly_ignored(&outside_file));
        assert!(scanner.is_accepted_source_file(&outside_file));

        let _ = fs::remove_dir_all(&temp_dir);
        let _ = fs::remove_dir_all(&outside_dir);
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

        let stackvet_cache = temp_dir.join(".stackvet").join("cache");
        fs::create_dir_all(&stackvet_cache).unwrap();
        fs::write(
            stackvet_cache.join("scan_cache.json"),
            "{\"data\":\"test\"}",
        )
        .unwrap();
        fs::write(stackvet_cache.join("vulnerabilities.json"), "[]").unwrap();

        let scanner = ProjectScanner::new(&temp_dir);
        let res = scanner.scan();

        assert_eq!(res.source_files.len(), 1);
        assert_eq!(res.source_files[0], temp_dir.join("app.py"));
        assert!(
            res.config_files.is_empty(),
            "Generated cache in .stackvet must not be classified as config files: {:?}",
            res.config_files
        );
        assert_eq!(res.total_distinct_files(), 1);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_populated_stackvet_cache_excluded_from_limits_and_oversized_diagnostics() {
        let temp_dir = make_temp_dir("stackvet_cache_exclusion");

        fs::write(temp_dir.join("main.py"), "print('hello')\n").unwrap();
        fs::write(temp_dir.join("pyproject.toml"), "[project]\nname='test'\n").unwrap();

        // Create a large 50KB cache file in .stackvet/cache/scan_cache.json
        let cache_dir = temp_dir.join(".stackvet").join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        let large_cache_data = "x".repeat(50_000);
        let cache_file = cache_dir.join("scan_cache.json");
        fs::write(
            &cache_file,
            format!("{{\"cache\": \"{}\"}}", large_cache_data),
        )
        .unwrap();

        // 1. Scanner configured with tiny 500-byte max_file_size limit
        // Cache file (50KB) must NOT be reported as oversized file
        let scanner = ProjectScanner::new(&temp_dir).with_max_file_size(500);
        let res = scanner.scan();

        assert!(
            res.oversized_files.is_empty(),
            "Generated cache files in .stackvet must not trigger oversized_files diagnostics: {:?}",
            res.oversized_files
        );

        // 2. Scanner configured with max_files = 3
        // Only main.py and pyproject.toml should be counted (2 files); if cache files were counted (4 files),
        // it would hit max_files = 3 and trigger truncation.
        let scanner_max_files = ProjectScanner::new(&temp_dir).with_max_files(3);
        let res_max = scanner_max_files.scan();
        assert_eq!(res_max.distinct_files_count, 2);
        assert!(
            !res_max.is_truncated,
            "Scan should not be truncated because cache files are excluded"
        );

        // 3. build_context should not include cache files anywhere
        let ctx = scanner.build_context().expect("build context");
        assert_eq!(ctx.source_files.len(), 1);
        assert_eq!(ctx.manifest_files.len(), 1);
        assert!(!ctx.source_files.contains(&cache_file));
        assert!(!ctx.manifest_files.contains(&cache_file));

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
        assert_eq!(res.distinct_files_count, 4);
        assert_eq!(res.source_files.len(), 4);
        assert_eq!(res.total_distinct_files(), 4);

        let ctx = scanner.build_context().unwrap();
        let diag = ctx
            .discovery_diagnostics
            .iter()
            .find(|d| d.rule_id.0 == "DISC-005" && d.message.contains("truncated"))
            .expect("Expected DISC-005 truncation diagnostic");
        assert!(diag.message.contains("4 distinct files"));
        assert!(diag.explanation.contains("distinct files"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_scanner_max_files_with_overlapping_classifications() {
        let temp_dir = make_temp_dir("max_files_overlapping");

        // Overlapping classifications:
        // 1. pyproject.toml -> manifest + config
        // 2. package.json   -> manifest + config
        // 3. docker-compose.yml -> dockerfile + config
        // 4. app.py         -> source
        // 5. index.ts       -> source
        fs::write(temp_dir.join("pyproject.toml"), "[project]\nname='test'\n").unwrap();
        fs::write(temp_dir.join("package.json"), "{\"name\":\"test\"}").unwrap();
        fs::write(temp_dir.join("docker-compose.yml"), "version: '3'\n").unwrap();
        fs::write(temp_dir.join("app.py"), "print('hello')\n").unwrap();
        fs::write(temp_dir.join("index.ts"), "console.log('hello');\n").unwrap();

        // If max_files = 3:
        // Under old buggy behavior:
        // pyproject.toml (+2) + package.json (+2) = 4 >= 3 -> prematurely truncated at only 2 distinct files!
        // Under fixed behavior:
        // Truncation occurs only after the configured number of distinct files (3) is reached.
        let scanner_limit_3 = ProjectScanner::new(&temp_dir).with_max_files(3);
        let res_3 = scanner_limit_3.scan();

        assert!(
            res_3.is_truncated,
            "Scan should be marked as truncated when distinct files reach limit"
        );
        assert_eq!(
            res_3.distinct_files_count, 3,
            "Exactly 3 distinct files should be scanned"
        );
        assert_eq!(
            res_3.total_distinct_files(),
            3,
            "total_distinct_files() must match distinct file count"
        );

        // Verify the incomplete-scan diagnostic reports the actual limit semantics
        let ctx_3 = scanner_limit_3.build_context().unwrap();
        let trunc_diag = ctx_3
            .discovery_diagnostics
            .iter()
            .find(|d| d.rule_id.0 == "DISC-005" && d.message.contains("truncated"))
            .expect("Expected DISC-005 truncation diagnostic");
        assert!(trunc_diag.message.contains("3 distinct files"));
        assert!(trunc_diag.explanation.contains("distinct files"));

        // If max_files = 5:
        // All 5 distinct files should be scanned.
        let scanner_limit_5 = ProjectScanner::new(&temp_dir).with_max_files(5);
        let res_5 = scanner_limit_5.scan();
        assert!(
            res_5.is_truncated,
            "Scan should be marked truncated when reaching exactly 5 distinct files"
        );
        assert_eq!(res_5.distinct_files_count, 5);
        assert_eq!(res_5.total_distinct_files(), 5);

        // If max_files = 6:
        // No truncation occurs because there are only 5 distinct files in the project.
        let scanner_limit_6 = ProjectScanner::new(&temp_dir).with_max_files(6);
        let res_6 = scanner_limit_6.scan();
        assert!(
            !res_6.is_truncated,
            "Scan should NOT be truncated when limit > distinct files count"
        );
        assert_eq!(res_6.distinct_files_count, 5);
        assert_eq!(res_6.total_distinct_files(), 5);
        assert_eq!(res_6.manifest_files.len(), 2);
        assert_eq!(res_6.config_files.len(), 3);
        assert_eq!(res_6.dockerfiles.len(), 1);
        assert_eq!(res_6.source_files.len(), 2);

        // If max_files = 0:
        // Immediately truncated with 0 files scanned.
        let scanner_limit_0 = ProjectScanner::new(&temp_dir).with_max_files(0);
        let res_0 = scanner_limit_0.scan();
        assert!(res_0.is_truncated);
        assert_eq!(res_0.distinct_files_count, 0);
        assert_eq!(res_0.total_distinct_files(), 0);

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
