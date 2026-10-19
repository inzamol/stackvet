use anyhow::Result;
use ignore::WalkBuilder;
use stackvet_core::ProjectContext;
use std::path::{Path, PathBuf};

pub struct ProjectScanner {
    root: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub source_files: Vec<PathBuf>,
    pub manifest_files: Vec<PathBuf>,
    pub config_files: Vec<PathBuf>,
    pub dockerfiles: Vec<PathBuf>,
    pub ci_workflows: Vec<PathBuf>,
}

impl ProjectScanner {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn scan(&self) -> ScanResult {
        let mut result = ScanResult::default();

        let walker = WalkBuilder::new(&self.root)
            .hidden(false)
            .parents(false)
            .git_ignore(true)
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                name != "node_modules"
                    && name != ".venv"
                    && name != "venv"
                    && name != "__pycache__"
                    && name != "target"
                    && name != ".git"
                    && name != "dist"
                    && name != "build"
                    && name != ".next"
            })
            .build();

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

    #[test]
    fn test_scanner_discovers_files() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("stackvet_test_scanner_{}", unique));
        fs::create_dir_all(&temp_dir).unwrap();

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
}
