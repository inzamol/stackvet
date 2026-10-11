use anyhow::Result;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, LanguageTarget, ProjectContext, Severity, TextSpan,
};
use std::fs;
use std::path::Path;

pub struct CeleryAnalyzer;

impl CeleryAnalyzer {
    pub fn new() -> Self {
        Self
    }

    fn check_celery_file(&self, path: &Path, content: &str, diags: &mut Vec<Diagnostic>) {
        if !content.contains("celery") && !content.contains("@app.task") && !content.contains("@shared_task") {
            return;
        }

        let lines: Vec<&str> = content.lines().collect();

        // CEL-001: task.apply() called synchronously
        for (idx, line) in lines.iter().enumerate() {
            if line.contains(".apply(") && !line.contains(".apply_async(") && !line.contains("def ") && !path.to_string_lossy().contains("test") {
                diags.push(
                    Diagnostic::builder("CEL-001", Severity::Warning, LanguageTarget::Python)
                        .message("Synchronous task execution using task.apply() instead of asynchronous .delay() or .apply_async()")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                        .explanation("Calling task.apply() runs the task synchronously in the local thread rather than sending it to background workers.")
                        .remediation("Use 'task.delay(...)' or 'task.apply_async(...)'.")
                        .build(),
                );
            }
        }

        // CEL-003: Task missing retry configuration
        for (idx, line) in lines.iter().enumerate() {
            if (line.contains("@app.task") || line.contains("@shared_task"))
                && !line.contains("autoretry_for")
                && !line.contains("max_retries")
            {
                diags.push(
                    Diagnostic::builder("CEL-003", Severity::Info, LanguageTarget::Python)
                        .message("Celery task definition missing automated retry configuration (autoretry_for)")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                        .explanation("Tasks performing network calls or third-party operations should configure autoretry_for=(Exception,) and retry_backoff=True to handle transient failures gracefully.")
                        .remediation("Add 'autoretry_for=(Exception,), retry_backoff=True' to task decorator.")
                        .build(),
                );
            }
        }

        // CEL-004: Task missing time limit
        for (idx, line) in lines.iter().enumerate() {
            if (line.contains("@app.task") || line.contains("@shared_task"))
                && !line.contains("time_limit")
                && !line.contains("soft_time_limit")
            {
                diags.push(
                    Diagnostic::builder("CEL-004", Severity::Info, LanguageTarget::Python)
                        .message("Celery task missing time_limit or soft_time_limit constraint")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                        .explanation("Without a time limit, runaway or blocked worker tasks can consume worker concurrency indefinitely.")
                        .remediation("Specify 'time_limit=300' or configure CELERY_TASK_TIME_LIMIT in settings.")
                        .build(),
                );
            }
        }
    }
}

impl Analyzer for CeleryAnalyzer {
    fn name(&self) -> &'static str {
        "CeleryAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::Python
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        for file in &ctx.source_files {
            if file.extension().and_then(|s| s.to_str()) == Some("py") {
                if let Ok(content) = fs::read_to_string(file) {
                    self.check_celery_file(file, &content, &mut diagnostics);
                }
            }
        }
        Ok(diagnostics)
    }
}
