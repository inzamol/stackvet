use anyhow::Result;
use stackvet_core::{
    Analyzer, Diagnostic, DiagnosticLocation, LanguageTarget, ProjectContext, Severity, TextSpan,
};
use std::fs;
use std::path::Path;

pub struct SqlAlchemyAnalyzer;

impl SqlAlchemyAnalyzer {
    pub fn new() -> Self {
        Self
    }

    fn check_sqlalchemy_file(&self, path: &Path, content: &str, diags: &mut Vec<Diagnostic>) {
        if !content.contains("sqlalchemy")
            && !content.contains("Session")
            && !content.contains("create_engine")
            && !content.contains("Base")
            && !content.contains("DeclarativeBase")
        {
            return;
        }

        let lines: Vec<&str> = content.lines().collect();

        // SQL-008: create_engine missing pool_pre_ping=True
        if content.contains("create_engine(") && !content.contains("pool_pre_ping=True") {
            for (idx, line) in lines.iter().enumerate() {
                if line.contains("create_engine(") && !line.contains("sqlite") {
                    diags.push(
                        Diagnostic::builder("SQL-008", Severity::Warning, LanguageTarget::Python)
                            .message("create_engine() missing pool_pre_ping=True connection liveness check")
                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                            .explanation("Database connections closed by firewalls or server-side timeouts result in OperationalError on next query unless pool_pre_ping=True is enabled.")
                            .remediation("Add 'pool_pre_ping=True' to create_engine(...).")
                            .build(),
                    );
                    break;
                }
            }
        }

        // SQL-010: Hardcoded database credentials
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("postgresql://") || line.contains("mysql://") || line.contains("mysql+pymysql://") {
                if line.contains(':') && line.contains('@') && !line.contains("os.environ") && !line.contains("os.getenv") && !line.contains("settings.") {
                    diags.push(
                        Diagnostic::builder("SQL-010", Severity::Critical, LanguageTarget::Python)
                            .message("Plaintext database credentials hardcoded in connection URL")
                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                            .explanation("Hardcoded database connection strings expose sensitive production passwords in version control.")
                            .remediation("Load database credentials from environment variables (e.g. DATABASE_URL).")
                            .build(),
                    );
                    break;
                }
            }
        }

        // SQL-004: Raw SQL string interpolation (SQL Injection Risk)
        for (idx, line) in lines.iter().enumerate() {
            if (line.contains("execute(f\"") || line.contains("execute(f'") || line.contains("text(f\"") || line.contains("text(f'"))
                && (line.contains("SELECT") || line.contains("INSERT") || line.contains("UPDATE") || line.contains("DELETE") || line.contains("select") || line.contains("where"))
            {
                diags.push(
                    Diagnostic::builder("SQL-004", Severity::Critical, LanguageTarget::Python)
                        .message("Raw SQL constructed via f-string interpolation (Critical SQL Injection Risk)")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                        .explanation("Interpolating variables directly into SQL statements using f-strings creates severe SQL Injection vulnerabilities.")
                        .remediation("Use bound parameters: 'execute(text(\"SELECT ... WHERE id = :id\"), {\"id\": user_id})'.")
                        .build(),
                );
            }
        }

        // SQL-005: Legacy session.query() syntax in 2.0
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("session.query(") || line.contains("db.query(") {
                diags.push(
                    Diagnostic::builder("SQL-005", Severity::Warning, LanguageTarget::Python)
                        .message("Legacy SQLAlchemy 1.x session.query() syntax used instead of select()")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                        .explanation("SQLAlchemy 2.0 deprecates Query objects in favor of execution through modern select(...) statements: 'session.scalars(select(Model))'.")
                        .remediation("Replace session.query(Model) with session.execute(select(Model)) or session.scalars(select(Model)).")
                        .build(),
                );
                break;
            }
        }

        // SQL-009: expire_on_commit=True in AsyncSession
        for (idx, line) in lines.iter().enumerate() {
            if (line.contains("AsyncSession") || line.contains("async_sessionmaker"))
                && (line.contains("expire_on_commit=True") || (!content.contains("expire_on_commit=False") && line.contains("sessionmaker(")))
            {
                diags.push(
                    Diagnostic::builder("SQL-009", Severity::Error, LanguageTarget::Python)
                        .message("expire_on_commit=True in AsyncSession causes MissingGreenlet attribute errors")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                        .explanation("In AsyncSession, accessing attributes on expired objects triggers lazy loading, which fails asynchronously with MissingGreenlet.")
                        .remediation("Set 'expire_on_commit=False' when creating async sessions.")
                        .build(),
                );
                break;
            }
        }

        // SQL-014: Deprecated declarative_base()
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("declarative_base()") {
                diags.push(
                    Diagnostic::builder("SQL-014", Severity::Warning, LanguageTarget::Python)
                        .message("Deprecated declarative_base() used instead of DeclarativeBase class")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                        .explanation("SQLAlchemy 2.0 recommends inheriting from 'sqlalchemy.orm.DeclarativeBase' rather than calling legacy declarative_base().")
                        .remediation("Define Base as 'class Base(DeclarativeBase): pass'.")
                        .build(),
                );
                break;
            }
        }

        // SQL-025: Deprecated engine.execute()
        for (idx, line) in lines.iter().enumerate() {
            if line.contains("engine.execute(") || line.contains("engine.scalar(") {
                diags.push(
                    Diagnostic::builder("SQL-025", Severity::Error, LanguageTarget::Python)
                        .message("Direct engine.execute() is removed in SQLAlchemy 2.0")
                        .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                        .explanation("Direct engine-level execution was removed. Execution must take place through an explicit connection context manager.")
                        .remediation("Use 'with engine.connect() as conn: conn.execute(...)'.")
                        .build(),
                );
                break;
            }
        }

        // SQL-001: N+1 query in loops
        let mut in_loop = false;
        let mut loop_indent = 0;
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("for ") || trimmed.starts_with("while ") {
                in_loop = true;
                loop_indent = line.chars().take_while(|c| c.is_whitespace()).count();
            } else if in_loop {
                let current_indent = line.chars().take_while(|c| c.is_whitespace()).count();
                if current_indent <= loop_indent && !trimmed.is_empty() {
                    in_loop = false;
                } else if trimmed.contains("session.execute(") || trimmed.contains("session.query(") || trimmed.contains(".filter(") {
                    diags.push(
                        Diagnostic::builder("SQL-001", Severity::Warning, LanguageTarget::Python)
                            .message("Potential N+1 database query executed inside loop")
                            .location(DiagnosticLocation::new(path.to_path_buf()).with_span(TextSpan::single_line(idx + 1, 1, 1)))
                            .explanation("Executing database queries inside iteration loops creates N+1 query amplification, degrading database performance and throughput.")
                            .remediation("Batch records using WHERE IN, join queries, or eager loading (joinedload / selectinload).")
                            .build(),
                    );
                    break;
                }
            }
        }
    }
}

impl Analyzer for SqlAlchemyAnalyzer {
    fn name(&self) -> &'static str {
        "SqlAlchemyAnalyzer"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::Python
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();

        for file in &ctx.source_files {
            if file.extension().and_then(|s| s.to_str()) == Some("py") {
                if let Ok(content) = fs::read_to_string(file) {
                    self.check_sqlalchemy_file(file, &content, &mut diagnostics);
                }
            }
        }

        Ok(diagnostics)
    }
}
