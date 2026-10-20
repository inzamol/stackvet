use anyhow::Result;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, LanguageTarget, ProjectContext, Severity, TextSpan,
};
use std::fs;
use std::path::Path;

pub struct ReactAnalyzer;

impl Default for ReactAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl ReactAnalyzer {
    pub fn new() -> Self {
        Self
    }

    fn check_react_file(&self, path: &Path, content: &str, diags: &mut Vec<Diagnostic>) {
        let is_tsx = path.extension().and_then(|s| s.to_str()) == Some("tsx");
        let is_jsx = path.extension().and_then(|s| s.to_str()) == Some("jsx");
        let is_app_router = path.to_string_lossy().replace('\\', "/").contains("/app/");
        let has_use_client = content.trim_start().starts_with("\"use client\"")
            || content.trim_start().starts_with("'use client'");

        // NXT-001: Next.js Server Component using Client APIs
        if is_app_router && !has_use_client && (is_tsx || is_jsx) {
            for (idx, line) in content.lines().enumerate() {
                if line.contains("useState(")
                    || line.contains("useEffect(")
                    || line.contains("useCallback(")
                    || line.contains("useRef(")
                    || line.contains("window.")
                    || line.contains("localStorage.")
                {
                    diags.push(
                        Diagnostic::builder("NXT-001", Severity::Error, LanguageTarget::Node)
                            .message(format!(
                                "Client hook or browser API used in Server Component without '\"use client\"' directive at line {}",
                                idx + 1
                            ))
                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                            .explanation("In Next.js App Router, components are Server Components by default. Hooks and browser globals cannot execute during server-side rendering.")
                            .remediation("Add '\"use client\";' at the top of the file, or move interactive logic to a client component.")
                            .build(),
                    );
                    break;
                }
            }
        }

        // NXT-002: Leaked Client Secret Env Variable
        if has_use_client {
            for (idx, line) in content.lines().enumerate() {
                if let Some(pos) = line.find("process.env.") {
                    let env_var = &line[pos + "process.env.".len()..];
                    let var_name: String = env_var
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if !var_name.is_empty()
                        && !var_name.starts_with("NEXT_PUBLIC_")
                        && var_name != "NODE_ENV"
                    {
                        diags.push(
                            Diagnostic::builder("NXT-002", Severity::Critical, LanguageTarget::Node)
                                .message(format!(
                                    "Private environment variable 'process.env.{}' accessed in Client Component",
                                    var_name
                                ))
                                .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                                .explanation(
                                    "Server secrets (API keys, database credentials) must never be accessed in client code. Only NEXT_PUBLIC_* variables are bundled safely.",
                                )
                                .remediation(format!("Prefix variable with NEXT_PUBLIC_{} if intended for the browser, or move to server action/API route.", var_name))
                                .build(),
                        );
                    }
                }
            }
        }

        // RCT-005: Barrel Import Bloat
        for (idx, line) in content.lines().enumerate() {
            if (line.contains("from '@mui/material'")
                || line.contains("from \"@mui/material\"")
                || line.contains("from 'lodash'")
                || line.contains("from \"lodash\""))
                && !line.contains("import type")
            {
                diags.push(
                    Diagnostic::builder("RCT-005", Severity::Warning, LanguageTarget::Node)
                        .message("Import from top-level barrel file causes bundle bloat")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                        .explanation("Importing directly from library root packages without path imports or babel transform plugins degrades build speeds and bundles unused code.")
                        .remediation("Import from specific modular subpaths, e.g. '@mui/material/Button' or 'lodash-es'.")
                        .build(),
                );
            }
        }

        // RCT-002: Conditional Hook Call
        let lines: Vec<&str> = content.lines().collect();
        let mut in_if = false;
        let mut if_indent = 0;

        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("if (") || trimmed.starts_with("if(") {
                in_if = true;
                if_indent = line.chars().take_while(|c| c.is_whitespace()).count();
            } else if in_if {
                let current_indent = line.chars().take_while(|c| c.is_whitespace()).count();
                if current_indent <= if_indent && !trimmed.is_empty() && trimmed.starts_with('}') {
                    in_if = false;
                } else if trimmed.starts_with("use")
                    && (trimmed.contains("useState")
                        || trimmed.contains("useEffect")
                        || trimmed.contains("useMemo"))
                {
                    diags.push(
                        Diagnostic::builder("RCT-002", Severity::Error, LanguageTarget::Node)
                            .message("React Hook invoked conditionally inside if-statement")
                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                            .explanation("Rules of Hooks: React hooks must be invoked unconditionally at the top level of component functions to preserve call order.")
                            .remediation("Move hook invocation out of conditional branches.")
                            .build(),
                    );
                }
            }
        }

        // RCT-004: Missing Key in .map()
        for (idx, line) in lines.iter().enumerate() {
            if line.contains(".map(")
                && (lines.get(idx + 1).is_some_and(|l| l.contains('<')) || line.contains('<'))
            {
                // If it renders JSX without key=
                let check_block = format!("{} {}", line, lines.get(idx + 1).unwrap_or(&""));
                if check_block.contains('<')
                    && !check_block.contains("key=")
                    && !check_block.contains("key={")
                {
                    diags.push(
                        Diagnostic::builder("RCT-004", Severity::Warning, LanguageTarget::Node)
                            .message("JSX element in .map() loop is missing a unique 'key' prop")
                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                            .explanation("React relies on key props to identify which items have changed, added, or removed in lists. Missing keys trigger UI rendering bugs and reconcile thrashing.")
                            .remediation("Provide a stable unique identifier as the key prop (e.g. key={item.id}).")
                            .build(),
                    );
                }
            }
        }
    }
}

impl Analyzer for ReactAnalyzer {
    fn name(&self) -> &'static str {
        "ReactAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::Node
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();

        for file in &ctx.source_files {
            if let Some(ext) = file.extension().and_then(|s| s.to_str()) {
                if matches!(ext, "tsx" | "jsx" | "ts" | "js") {
                    if let Ok(content) = fs::read_to_string(file) {
                        self.check_react_file(file, &content, &mut diagnostics);
                    }
                }
            }
        }

        Ok(diagnostics)
    }
}
