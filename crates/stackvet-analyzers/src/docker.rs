use anyhow::Result;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, LanguageTarget, ProjectContext, Severity, TextSpan,
};
use std::fs;
use std::path::Path;

pub struct DockerAnalyzer;

impl DockerAnalyzer {
    pub fn new() -> Self {
        Self
    }

    fn analyze_dockerfile(
        &self,
        path: &Path,
        content: &str,
        diags: &mut Vec<Diagnostic>,
        root: &Path,
    ) {
        let lines: Vec<&str> = content.lines().collect();
        let mut has_user_instruction = false;
        let mut has_healthcheck = false;
        let mut has_expose = false;
        let mut has_copy_before_deps = false;
        let mut found_copy = false;
        let mut found_web_server = false;
        let mut has_python_unbuffered = false;
        let mut has_python_dontwritebytecode = false;

        let web_keywords = [
            "uvicorn", "gunicorn", "fastapi", "flask", "django", "next", "vite",
        ];

        // DOC-003: Missing .dockerignore
        let dockerignore = path.parent().unwrap_or(root).join(".dockerignore");
        let root_dockerignore = root.join(".dockerignore");
        if !dockerignore.is_file() && !root_dockerignore.is_file() {
            diags.push(
                Diagnostic::builder("DOC-003", Severity::Warning, LanguageTarget::Docker)
                    .message("Missing .dockerignore file in project directory")
                    .file(path)
                    .explanation(
                        "Without a .dockerignore file, sensitive files (.env, .git, venv, node_modules) may be copied into the container build context, bloating image size and leaking secrets.",
                    )
                    .remediation("Create a .dockerignore excluding .git, .env, __pycache__, node_modules, and virtual environments.")
                    .build(),
            );
        }

        for (idx, line) in lines.iter().enumerate() {
            let line_num = idx + 1;
            let trimmed = line.trim();

            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }

            let upper = trimmed.to_uppercase();

            // Track flags
            if upper.starts_with("USER ") {
                has_user_instruction = true;
            }
            if upper.starts_with("HEALTHCHECK ") {
                has_healthcheck = true;
            }
            if upper.starts_with("EXPOSE ") {
                has_expose = true;
            }

            for kw in web_keywords {
                if trimmed.contains(kw) {
                    found_web_server = true;
                    break;
                }
            }

            if upper.contains("PYTHONUNBUFFERED") {
                has_python_unbuffered = true;
            }
            if upper.contains("PYTHONDONTWRITEBYTECODE") {
                has_python_dontwritebytecode = true;
            }

            // DOC-002: Unpinned base image tag
            if upper.starts_with("FROM ") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if let Some(image) = parts.get(1) {
                    if !image.contains(':') || image.ends_with(":latest") {
                        diags.push(
                            Diagnostic::builder("DOC-002", Severity::Warning, LanguageTarget::Docker)
                                .message(format!("Unpinned base image tag '{}'", image))
                                .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(line_num, 1, 1)))
                                .explanation("Using untagged or ':latest' base images makes builds non-reproducible and vulnerable to breaking upstream changes.")
                                .remediation("Pin image to a specific version or digest, e.g. 'python:3.12-slim'.")
                                .build(),
                        );
                    }
                }
            }

            // DOC-006: Secrets in ARG / ENV
            if upper.starts_with("ARG ") || upper.starts_with("ENV ") {
                for secret_word in ["PASSWORD", "SECRET", "TOKEN", "API_KEY", "PRIVATE_KEY"] {
                    if upper.contains(secret_word) && !upper.contains("ARG_NAME") {
                        diags.push(
                            Diagnostic::builder("DOC-006", Severity::Critical, LanguageTarget::Docker)
                                .message(format!("Potential sensitive secret defined in Dockerfile: '{}'", secret_word))
                                .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(line_num, 1, 1)))
                                .explanation("Values in ARG and ENV instructions are permanently embedded in Docker image layer metadata and visible via 'docker inspect'.")
                                .remediation("Use Docker BuildKit secrets mount (--mount=type=secret) or runtime environment variables.")
                                .build(),
                        );
                        break;
                    }
                }
            }

            // DOC-007: apt-get without clean
            if upper.contains("APT-GET INSTALL") && !upper.contains("/VAR/LIB/APT/LISTS") {
                diags.push(
                    Diagnostic::builder("DOC-007", Severity::Warning, LanguageTarget::Docker)
                        .message("apt-get install without cleaning /var/lib/apt/lists/*")
                        .location(
                            DiagnosticLocation::new(path.to_path_buf())
                                .with_span(TextSpan::single_line(line_num, 1, 1)),
                        )
                        .explanation(
                            "Uncleaned apt package lists bloat the Docker image layer cache.",
                        )
                        .remediation(
                            "Append '&& rm -rf /var/lib/apt/lists/*' to the apt-get RUN command.",
                        )
                        .build(),
                );
            }

            // DOC-008: pip install without --no-cache-dir
            if upper.contains("PIP INSTALL")
                && !upper.contains("--NO-CACHE-DIR")
                && !upper.contains("PIP_NO_CACHE_DIR")
            {
                diags.push(
                    Diagnostic::builder("DOC-008", Severity::Warning, LanguageTarget::Docker)
                        .message("pip install without --no-cache-dir")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(line_num, 1, 1)))
                        .explanation("Pip stores downloaded wheel caches inside the layer, needlessly inflating container size.")
                        .remediation("Add '--no-cache-dir' to pip install or set 'ENV PIP_NO_CACHE_DIR=1'.")
                        .build(),
                );
            }

            // DOC-012: ADD used instead of COPY
            if upper.starts_with("ADD ")
                && !trimmed.contains(".tar")
                && !trimmed.contains(".gz")
                && !trimmed.contains("http://")
                && !trimmed.contains("https://")
            {
                diags.push(
                    Diagnostic::builder("DOC-012", Severity::Warning, LanguageTarget::Docker)
                        .message("Use COPY instead of ADD for local files")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(line_num, 1, 1)))
                        .explanation("ADD has unexpected behaviors such as automatic tar extraction and remote URL fetching. Docker best practice recommends COPY for local files.")
                        .remediation("Replace 'ADD' with 'COPY'.")
                        .build(),
                );
            }

            // DOC-013: sudo installed or used
            if upper.contains("SUDO ") || upper.contains("APT-GET INSTALL SUDO") {
                diags.push(
                    Diagnostic::builder("DOC-013", Severity::Error, LanguageTarget::Docker)
                        .message("Insecure sudo installed or executed in container")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(line_num, 1, 1)))
                        .explanation("Installing sudo in containers increases attack surface and introduces privilege escalation vulnerabilities.")
                        .remediation("Remove sudo and configure permissions directly using the USER instruction.")
                        .build(),
                );
            }

            // DOC-001: Layer caching check (COPY . . before pip install or pnpm install)
            if upper.starts_with("COPY . .") || upper.starts_with("COPY . /") {
                found_copy = true;
            }
            if found_copy
                && (upper.contains("PIP INSTALL")
                    || upper.contains("PNPM INSTALL")
                    || upper.contains("NPM INSTALL"))
            {
                has_copy_before_deps = true;
            }
        }

        // DOC-001: Layer Caching
        if has_copy_before_deps {
            diags.push(
                Diagnostic::builder("DOC-001", Severity::Warning, LanguageTarget::Docker)
                    .message("Source code copied before package dependency installation invalidating build cache")
                    .file(path)
                    .explanation("Copying the entire repository before installing requirements causes any source code change to invalidate the dependency install cache layer.")
                    .remediation("Copy requirement manifests first (e.g. 'COPY requirements.txt .'), install dependencies, then copy source code.")
                    .build(),
            );
        }

        // DOC-004: Root user execution
        if !has_user_instruction {
            diags.push(
                Diagnostic::builder("DOC-004", Severity::Error, LanguageTarget::Docker)
                    .message("Container executes as root user (missing USER instruction)")
                    .file(path)
                    .explanation("Running containers as root violates the principle of least privilege. In the event of a container breakout, the attacker gains host root privileges.")
                    .remediation("Create and switch to a non-root user (e.g. 'USER appuser' or 'USER 1001').")
                    .build(),
            );
        }

        // DOC-005: Missing HEALTHCHECK
        if found_web_server && !has_healthcheck {
            diags.push(
                Diagnostic::builder("DOC-005", Severity::Warning, LanguageTarget::Docker)
                    .message("Web application container missing HEALTHCHECK instruction")
                    .file(path)
                    .explanation("Container orchestrators (Docker Swarm, Kubernetes, ECS) rely on HEALTHCHECK to detect hung or unresponsive web processes.")
                    .remediation("Add a HEALTHCHECK instruction, e.g. 'HEALTHCHECK CMD curl -f http://localhost:8000/health || exit 1'.")
                    .build(),
            );
        }

        // DOC-011: Web server without EXPOSE
        if found_web_server && !has_expose {
            diags.push(
                Diagnostic::builder("DOC-011", Severity::Warning, LanguageTarget::Docker)
                    .message("Web server detected but no EXPOSE instruction declared")
                    .file(path)
                    .explanation("Declaring EXPOSE documents intended communication ports for documentation and automatic port mapping.")
                    .remediation("Add 'EXPOSE 8000' or appropriate port.")
                    .build(),
            );
        }

        // DOC-009: Missing PYTHONUNBUFFERED
        if content.contains("python") && !has_python_unbuffered {
            diags.push(
                Diagnostic::builder("DOC-009", Severity::Info, LanguageTarget::Docker)
                    .message("Python container missing ENV PYTHONUNBUFFERED=1")
                    .file(path)
                    .explanation("Without PYTHONUNBUFFERED=1, Python buffers stdout/stderr, delaying container log streaming in docker logs and cloud log aggregators.")
                    .remediation("Add 'ENV PYTHONUNBUFFERED=1' near top of Dockerfile.")
                    .build(),
            );
        }

        // DOC-010: Missing PYTHONDONTWRITEBYTECODE
        if content.contains("python") && !has_python_dontwritebytecode {
            diags.push(
                Diagnostic::builder("DOC-010", Severity::Info, LanguageTarget::Docker)
                    .message("Python container missing ENV PYTHONDONTWRITEBYTECODE=1")
                    .file(path)
                    .explanation("Prevents Python from writing unnecessary .pyc files to the container filesystem.")
                    .remediation("Add 'ENV PYTHONDONTWRITEBYTECODE=1'.")
                    .build(),
            );
        }
    }
}

impl Default for DockerAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer for DockerAnalyzer {
    fn name(&self) -> &'static str {
        "DockerAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::Docker
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        for dockerfile in &ctx.dockerfiles {
            if let Ok(content) = fs::read_to_string(dockerfile) {
                self.analyze_dockerfile(dockerfile, &content, &mut diagnostics, &ctx.root);
            }
        }
        Ok(diagnostics)
    }
}
