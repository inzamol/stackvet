use anyhow::Result;
use ruff_python_ast::{Expr, Stmt, StmtFunctionDef};
use ruff_python_parser::parse_module;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, LanguageTarget, ProjectContext, Severity,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

pub struct FastApiAnalyzer;

impl FastApiAnalyzer {
    pub fn new() -> Self {
        Self
    }

    fn check_fastapi_file(&self, path: &Path, content: &str, diags: &mut Vec<Diagnostic>) {
        if !content.contains("FastAPI")
            && !content.contains("APIRouter")
            && !content.contains("@app.")
            && !content.contains("@router.")
        {
            return;
        }

        // FAP-004: Insecure CORS
        if content.contains("CORSMiddleware")
            && (content.contains("allow_origins=[\"*\"]") || content.contains("allow_origins=['*']"))
            && content.contains("allow_credentials=True")
        {
            diags.push(
                Diagnostic::builder("FAP-004", Severity::Critical, LanguageTarget::Python)
                    .message("Insecure CORS configuration: allow_origins=['*'] combined with allow_credentials=True")
                    .file(path)
                    .explanation(
                        "CORS specification explicitly forbids wildcard '*' origins with allow_credentials=True. Browsers will reject responses, and it introduces security vulnerabilities.",
                    )
                    .remediation("Specify explicit trusted origins in allow_origins instead of '*'.")
                    .build(),
            );
        }

        // FAP-008: Deprecated on_event
        if content.contains("@app.on_event(\"startup\")")
            || content.contains("@app.on_event('startup')")
            || content.contains("@app.on_event(\"shutdown\")")
            || content.contains("@app.on_event('shutdown')")
        {
            diags.push(
                Diagnostic::builder("FAP-008", Severity::Warning, LanguageTarget::Python)
                    .message("Deprecated @app.on_event used instead of lifespan context manager")
                    .file(path)
                    .explanation(
                        "FastAPI has deprecated on_event('startup') / on_event('shutdown') in favor of standard asynccontextmanager lifespan handlers.",
                    )
                    .remediation("Migrate startup/shutdown logic to a lifespan context manager on FastAPI(lifespan=lifespan).")
                    .build(),
            );
        }

        let parsed = match parse_module(content) {
            Ok(p) => p,
            Err(_) => return,
        };

        let mut route_registry: HashMap<(String, String), usize> = HashMap::new();
        let module = parsed.into_syntax();

        for stmt in &module.body {
            if let Stmt::FunctionDef(func) = stmt {
                if func.is_async {
                    self.inspect_async_route(path, content, func, &mut route_registry, diags);
                } else {
                    self.inspect_sync_function(path, content, func, diags);
                }
            }
        }
    }

    fn inspect_async_route(
        &self,
        path: &Path,
        content: &str,
        func: &StmtFunctionDef,
        route_registry: &mut HashMap<(String, String), usize>,
        diags: &mut Vec<Diagnostic>,
    ) {
        let fn_name = func.name.to_string();
        let offset = usize::from(func.range.start());
        let line = content[..offset.min(content.len())].matches('\n').count() + 1;

        // Check route decorators
        for decorator in &func.decorator_list {
            if let Expr::Call(call) = &decorator.expression {
                if let Expr::Attribute(attr) = &*call.func {
                    let method = attr.attr.to_string(); // get, post, put, delete, etc.
                    if matches!(
                        method.as_str(),
                        "get" | "post" | "put" | "delete" | "patch" | "options"
                    ) {
                        // Extract path template if first argument is string
                        let mut route_path = String::new();
                        if let Some(first_arg) = call.arguments.args.first() {
                            if let Expr::StringLiteral(s) = first_arg {
                                route_path = s.value.to_string();
                            }
                        }

                        // FAP-010: Duplicate route check
                        if !route_path.is_empty() {
                            let key = (method.clone(), route_path.clone());
                            if let Some(first_seen_line) = route_registry.get(&key) {
                                diags.push(
                                    Diagnostic::builder("FAP-010", Severity::Error, LanguageTarget::Python)
                                        .message(format!(
                                            "Duplicate route definition: {} '{}' already registered at line {}",
                                            method.to_uppercase(),
                                            route_path,
                                            first_seen_line
                                        ))
                                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(stackvet_core::TextSpan::single_line(line, 1, 1)))
                                        .explanation("FastAPI only invokes the first registered route handler for identical method and path. Subsequent routes are shadowed.")
                                        .remediation("Change the path or consolidate handlers.")
                                        .build(),
                                );
                            } else {
                                route_registry.insert(key, line);
                            }

                            // FAP-006: Path template placeholder parameter check
                            let placeholders: Vec<&str> = route_path
                                .split('{')
                                .skip(1)
                                .filter_map(|part| part.split('}').next())
                                .collect();

                            let param_names: HashSet<String> = func
                                .parameters
                                .args
                                .iter()
                                .map(|p| p.parameter.name.to_string())
                                .collect();

                            for placeholder in placeholders {
                                if !param_names.contains(placeholder) {
                                    diags.push(
                                        Diagnostic::builder("FAP-006", Severity::Error, LanguageTarget::Python)
                                            .message(format!(
                                                "Route path parameter '{{{}}}' missing from function signature in '{}'",
                                                placeholder, fn_name
                                            ))
                                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(stackvet_core::TextSpan::single_line(line, 1, 1)))
                                            .explanation(format!(
                                                "The URL template expects '{{{}}}', but parameter '{}' is not accepted by handler function '{}'.",
                                                placeholder, placeholder, fn_name
                                            ))
                                            .remediation(format!("Add '{}: str' or appropriate type to handler parameters.", placeholder))
                                            .build(),
                                    );
                                }
                            }
                        }

                        // FAP-003: Missing response_model / return type
                        let has_response_model = call
                            .arguments
                            .keywords
                            .iter()
                            .any(|kw| kw.arg.as_ref().map(|a| a.as_str()) == Some("response_model"));
                        let has_return_type = func.returns.is_some();

                        if !has_response_model && !has_return_type {
                            diags.push(
                                Diagnostic::builder("FAP-003", Severity::Warning, LanguageTarget::Python)
                                    .message(format!(
                                        "Route handler '{}' missing response_model or return type annotation",
                                        fn_name
                                    ))
                                    .location(DiagnosticLocation::new(path.to_path_buf()).with_span(stackvet_core::TextSpan::single_line(line, 1, 1)))
                                    .explanation("FastAPI OpenAPI schema generation and response data serialization require response_model or return type annotations.")
                                    .remediation("Add 'response_model=...' to the decorator or annotate return type '-> Model'.")
                                    .build(),
                            );
                        }

                        // Inspect body for blocking calls (FAP-001) & sync file I/O (FAP-015)
                        self.check_blocking_in_async(path, content, &func.body, diags);
                    }
                }
            }
        }
    }

    fn inspect_sync_function(
        &self,
        path: &Path,
        content: &str,
        func: &StmtFunctionDef,
        diags: &mut Vec<Diagnostic>,
    ) {
        // FAP-011: Mutable default parameter
        for arg in &func.parameters.args {
            if let Some(default) = &arg.default {
                if matches!(default.as_ref(), Expr::List(_) | Expr::Dict(_) | Expr::Set(_)) {
                    let offset = usize::from(func.range.start());
                    let line = content[..offset.min(content.len())].matches('\n').count() + 1;
                    diags.push(
                        Diagnostic::builder("FAP-011", Severity::Error, LanguageTarget::Python)
                            .message(format!(
                                "Mutable default argument used in function '{}'",
                                func.name
                            ))
                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(stackvet_core::TextSpan::single_line(line, 1, 1)))
                            .explanation("Default argument values are evaluated once when function is defined. Mutable defaults persist modifications across requests.")
                            .remediation("Use 'None' as default value and initialize inside function body.")
                            .build(),
                    );
                }
            }
        }
    }

    fn check_blocking_in_async(
        &self,
        path: &Path,
        content: &str,
        body: &[Stmt],
        diags: &mut Vec<Diagnostic>,
    ) {
        for stmt in body {
            // Check with open(...) -> FAP-015
            if let Stmt::With(with_stmt) = stmt {
                for item in &with_stmt.items {
                    if let Expr::Call(call) = &item.context_expr {
                        if let Expr::Name(n) = &*call.func {
                            if n.id.as_str() == "open" {
                                let offset = usize::from(with_stmt.range.start());
                                let line = content[..offset.min(content.len())].matches('\n').count() + 1;
                                diags.push(
                                    Diagnostic::builder("FAP-015", Severity::Error, LanguageTarget::Python)
                                        .message("Synchronous file I/O with open() inside async route handler")
                                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(stackvet_core::TextSpan::single_line(line, 1, 1)))
                                        .explanation("Synchronous file operations block the asyncio event loop, degrading concurrency for all concurrent requests.")
                                        .remediation("Use 'aiofiles' or execute blocking I/O with 'asyncio.to_thread()'.")
                                        .build(),
                                );
                            }
                        }
                    }
                }
            }

            // Check calls: time.sleep, requests.get, subprocess -> FAP-001
            if let Stmt::Expr(expr_stmt) = stmt {
                if let Expr::Call(call) = &*expr_stmt.value {
                    if let Expr::Attribute(attr) = &*call.func {
                        if let Expr::Name(obj) = &*attr.value {
                            if (obj.id.as_str() == "time" && attr.attr.as_str() == "sleep")
                                || (obj.id.as_str() == "requests")
                                || (obj.id.as_str() == "subprocess")
                            {
                                let offset = usize::from(expr_stmt.range.start());
                                let line = content[..offset.min(content.len())].matches('\n').count() + 1;
                                diags.push(
                                    Diagnostic::builder("FAP-001", Severity::Critical, LanguageTarget::Python)
                                        .message(format!(
                                            "Blocking call '{}.{}()' executed inside async def route handler",
                                            obj.id, attr.attr
                                        ))
                                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(stackvet_core::TextSpan::single_line(line, 1, 1)))
                                        .explanation("Blocking functions freeze the Python event loop thread, preventing other HTTP requests from being processed.")
                                        .remediation("Use asynchronous equivalents (e.g. 'await asyncio.sleep()', 'httpx.AsyncClient').")
                                        .build(),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Analyzer for FastApiAnalyzer {
    fn name(&self) -> &'static str {
        "FastApiAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::Python
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();

        for file in &ctx.source_files {
            if file.extension().and_then(|s| s.to_str()) == Some("py") {
                if let Ok(content) = fs::read_to_string(file) {
                    self.check_fastapi_file(file, &content, &mut diagnostics);
                }
            }
        }

        Ok(diagnostics)
    }
}
