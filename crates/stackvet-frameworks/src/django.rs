use anyhow::Result;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, LanguageTarget, ProjectContext, Severity, TextSpan,
};
use std::fs;
use std::path::Path;

pub struct DjangoAnalyzer;

impl DjangoAnalyzer {
    pub fn new() -> Self {
        Self
    }

    fn check_django_file(&self, path: &Path, content: &str, diags: &mut Vec<Diagnostic>) {
        let is_settings = path.to_string_lossy().contains("settings");
        let lines: Vec<&str> = content.lines().collect();

        if is_settings {
            // DJG-001: Hardcoded DEBUG = True
            for (idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed == "DEBUG = True" || trimmed.starts_with("DEBUG = True #") {
                    diags.push(
                        Diagnostic::builder("DJG-001", Severity::Warning, LanguageTarget::Python)
                            .message("Hardcoded DEBUG = True in Django settings")
                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                            .explanation("Running with DEBUG=True in production exposes full stack traces, server environment variables, and database queries to end users.")
                            .remediation("Use 'DEBUG = os.getenv(\"DJANGO_DEBUG\", \"False\").lower() == \"true\"'.")
                            .build(),
                    );
                }
            }

            // DJG-002: Hardcoded django-insecure SECRET_KEY
            for (idx, line) in lines.iter().enumerate() {
                if line.contains("django-insecure-") || (line.contains("SECRET_KEY = '") && line.len() < 30) {
                    diags.push(
                        Diagnostic::builder("DJG-002", Severity::Critical, LanguageTarget::Python)
                            .message("Insecure default or hardcoded SECRET_KEY in Django settings")
                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                            .explanation("The default django-insecure SECRET_KEY is publicly known. An attacker can forge cryptographic signatures, session cookies, and password reset tokens.")
                            .remediation("Load SECRET_KEY securely from environment variables.")
                            .build(),
                    );
                    break;
                }
            }

            // DJG-003: ALLOWED_HOSTS = ['*']
            for (idx, line) in lines.iter().enumerate() {
                if (line.contains("ALLOWED_HOSTS = ['*']") || line.contains("ALLOWED_HOSTS = [\"*\"]")) && !content.contains("DEBUG = True") {
                    diags.push(
                        Diagnostic::builder("DJG-003", Severity::Error, LanguageTarget::Python)
                            .message("Wildcard ALLOWED_HOSTS = ['*'] configured in production")
                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                            .explanation("Wildcard host headers allow HTTP Host header poisoning, password reset poisoning, and cache poisoning attacks.")
                            .remediation("Specify explicit trusted domain names in ALLOWED_HOSTS.")
                            .build(),
                    );
                    break;
                }
            }

            // DJG-004: Insecure cookie settings
            if content.contains("SESSION_COOKIE_SECURE = False") || content.contains("CSRF_COOKIE_SECURE = False") {
                diags.push(
                    Diagnostic::builder("DJG-004", Severity::Error, LanguageTarget::Python)
                        .message("Insecure cookie configuration: Secure flag disabled for session or CSRF cookies")
                        .file(path)
                        .explanation("Disabling the Secure flag allows cookies to be transmitted over plaintext HTTP, susceptible to man-in-the-middle sniffing.")
                        .remediation("Set 'SESSION_COOKIE_SECURE = True' and 'CSRF_COOKIE_SECURE = True' in production.")
                        .build(),
                );
            }
        }

        // DJG-006: @csrf_exempt
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("@csrf_exempt") {
                diags.push(
                    Diagnostic::builder("DJG-006", Severity::Warning, LanguageTarget::Python)
                        .message("@csrf_exempt decorator applied to view handler")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                        .explanation("Exempting views from CSRF protection allows unauthorized cross-origin requests to trigger state-changing mutations on behalf of authenticated users.")
                        .remediation("Ensure CSRF tokens are validated, or verify authentication tokens via Authorization headers.")
                        .build(),
                );
            }
        }
    }
}

impl Analyzer for DjangoAnalyzer {
    fn name(&self) -> &'static str {
        "DjangoAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::Python
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        for file in &ctx.source_files {
            if file.extension().and_then(|s| s.to_str()) == Some("py") {
                if let Ok(content) = fs::read_to_string(file) {
                    if content.contains("django") || file.to_string_lossy().contains("settings") || file.to_string_lossy().contains("urls") {
                        self.check_django_file(file, &content, &mut diagnostics);
                    }
                }
            }
        }
        Ok(diagnostics)
    }
}
