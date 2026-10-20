use anyhow::Result;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use stackvet_core::ProjectContext;
use std::path::{Path, PathBuf};

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
}

#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub source_files: Vec<PathBuf>,
    pub manifest_files: Vec<PathBuf>,
    pub config_files: Vec<PathBuf>,
    pub dockerfiles: Vec<PathBuf>,
    pub ci_workflows: Vec<PathBuf>,
    pub ignored_paths: Vec<PathBuf>,
}

impl ProjectScanner {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            custom_ignores: Vec::new(),
            use_default_exclusions: true,
            verbose: false,
        }
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

    pub fn scan(&self) -> ScanResult {
        let mut result = ScanResult::default();

        let mut builder = WalkBuilder::new(&self.root);
        builder.hidden(false);
        builder.parents(false);
        builder.git_ignore(true);

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
                let rule = if trimmed.starts_with('!') {
                    trimmed[1..].to_string()
                } else {
                    format!("!{trimmed}")
                };
                let _ = ov_builder.add(&rule);
            }
            if let Ok(overrides) = ov_builder.build() {
                builder.overrides(overrides);
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

        for entry_res in walker.flatten() {
            if entry_res.file_type().map_or(false, |ft| ft.is_file()) {
                let path = entry_res.into_path();
                let file_name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();

                // Check Dockerfiles
                if file_name == "Dockerfile"
                    || file_name.starts_with("Dockerfile.")
                    || file_name.ends_with(".dockerfile")
                    || file_name.starts_with("docker-compose")
                {
                    result.dockerfiles.push(path.clone());
                }

                // Check GitHub Actions / CI workflows
                if path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .contains(".github/workflows")
                    && (file_name.ends_with(".yml") || file_name.ends_with(".yaml"))
                {
                    result.ci_workflows.push(path.clone());
                }

                // Manifests & Lockfiles
                if file_name == "pyproject.toml"
                    || file_name == "package.json"
                    || file_name == "requirements.txt"
                    || file_name == "uv.lock"
                    || file_name == "poetry.lock"
                    || file_name == "package-lock.json"
                    || file_name == "pnpm-lock.yaml"
                    || file_name == "yarn.lock"
                {
                    result.manifest_files.push(path.clone());
                }

                // Source files
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    match ext {
                        "py" | "ts" | "tsx" | "js" | "jsx" => {
                            result.source_files.push(path);
                        }
                        "yml" | "yaml" | "toml" | "json" => {
                            result.config_files.push(path);
                        }
                        _ => {}
                    }
                }
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
        Ok(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("stackvet_test_scanner_{}", unique));
        fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_scanner_discovers_files() {
        let temp_dir = make_temp_dir();

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
        let temp_dir = make_temp_dir();

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
        let temp_dir = make_temp_dir();

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
}
