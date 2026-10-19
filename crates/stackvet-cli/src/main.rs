use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};
use stackvet_analyzers::run_all_analyzers;
use stackvet_core::{
    get_rule_meta, BaselineData, HealthScoreExplanation, ProjectContext, Severity, ALL_RULES,
};
use stackvet_correlation::{
    calculate_health_score_explained, deduplicate_diagnostics, score_grade, CorrelationEngine,
};
use stackvet_frameworks::run_all_framework_analyzers;
use stackvet_graph::ImportGraph;
use stackvet_node::detect_node_project;
use stackvet_python::detect_python_project;
use stackvet_remediation::RemediationEngine;
use stackvet_reporter::Reporter;
use stackvet_scanner::ProjectScanner;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

#[derive(Parser, Debug)]
#[command(
    name = "stackvet",
    about = "Universal project health and static diagnostics engine",
    version,
    author = "StackVet Engineering"
)]
pub struct Cli {
    #[arg(short, long, default_value = ".", help = "Target project directory")]
    pub path: PathBuf,

    #[arg(
        short,
        long,
        value_enum,
        default_value_t = OutputFormat::Text,
        help = "Output report format"
    )]
    pub format: OutputFormat,

    #[arg(long, help = "Run offline without querying vulnerability databases")]
    pub offline: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(
        about = "Unified health scorecard, grade (A-F), and top root causes",
        aliases = ["diagnose", "health"]
    )]
    Doctor {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(
            long,
            help = "Fail with exit code 1 if health score is below threshold"
        )]
        min_score: Option<u32>,

        #[arg(
            long,
            help = "Display in-depth explanation and diagnostic contributions to the health score"
        )]
        explain: bool,
    },

    #[command(
        about = "Fast CI gate (returns 0 for clean, 1 for errors)",
        aliases = ["scan", "audit"]
    )]
    Check {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(long, help = "Treat warnings as errors")]
        strict: bool,
    },

    #[command(
        about = "Detailed inspection with code snippets and remediation guidance",
        aliases = ["examine", "details"]
    )]
    Inspect {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(long, help = "Filter by rule ID prefix (e.g. FAP, DEP, RCT, DOC, SQL)")]
        rule: Option<String>,
    },

    #[command(about = "Display in-depth explanation and remediation guide for a specific rule ID")]
    Explain {
        #[arg(help = "Rule ID to explain (e.g. FAP-001, DOC-001, SQL-004)")]
        rule_id: String,
    },

    #[command(
        about = "Apply automated safe remediations",
        aliases = ["remediate", "auto-fix"]
    )]
    Fix {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(long, help = "Automatically apply all safe fixes without confirmation")]
        yes: bool,

        #[arg(
            long,
            help = "Preview proposed changes and diffs without modifying files"
        )]
        dry_run: bool,

        #[arg(long, help = "Rollback the last applied auto-fix session")]
        rollback: bool,
    },

    #[command(
        about = "Visualize dependency or module import graph",
        aliases = ["architecture", "dependencies"]
    )]
    Graph {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(long, value_enum, default_value_t = GraphFormat::Ascii)]
        style: GraphFormat,
    },

    #[command(about = "Manage legacy issue baselines to prevent CI regressions")]
    Baseline {
        #[command(subcommand)]
        action: BaselineAction,
    },

    #[command(
        about = "Initialize StackVet configuration or GitHub Actions CI workflow",
        aliases = ["setup"]
    )]
    Init {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(
            long,
            help = "Generate GitHub Actions CI workflow (.github/workflows/stackvet.yml)"
        )]
        ci: bool,
    },

    #[command(
        about = "List and search all supported diagnostic rules",
        aliases = ["catalog", "list-rules"]
    )]
    Rules {
        #[arg(help = "Filter rules by category or keyword (e.g. docker, fastapi, react)")]
        filter: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum BaselineAction {
    #[command(about = "Record current project diagnostics into a baseline file")]
    Record {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(
            short,
            long,
            default_value = ".stackvet-baseline.json",
            help = "Output baseline filepath"
        )]
        output: PathBuf,
    },

    #[command(about = "Verify that no new regressions have been introduced against baseline")]
    Verify {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(
            short,
            long,
            default_value = ".stackvet-baseline.json",
            help = "Baseline filepath to check against"
        )]
        baseline: PathBuf,

        #[arg(long, help = "Treat warnings as errors")]
        strict: bool,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
    Sarif,
    Html,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphFormat {
    Ascii,
    Mermaid,
    Dot,
}

fn build_full_context(root: &Path) -> Result<ProjectContext> {
    let scanner = ProjectScanner::new(root);
    let mut ctx = scanner.build_context()?;
    ctx.python = detect_python_project(root);
    ctx.node = detect_node_project(root);
    Ok(ctx)
}

fn collect_all_diagnostics(
    ctx: &ProjectContext,
    offline: bool,
) -> Result<Vec<stackvet_core::Diagnostic>> {
    let mut diagnostics = run_all_analyzers(ctx, offline)?;
    let mut fw_diags = run_all_framework_analyzers(ctx)?;
    diagnostics.append(&mut fw_diags);

    // Deduplicate diagnostics to prevent duplicate findings from distorting scores or reports
    let (mut deduped, _) = deduplicate_diagnostics(&diagnostics);

    // Deterministic sorting across repeated runs:
    // Primary: severity descending (Critical > Error > Warning > Info > Hint)
    // Secondary: rule_id ascending
    // Tertiary: file_path ascending
    // Quaternary: line ascending
    // Quinary: message ascending
    deduped.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.rule_id.0.cmp(&b.rule_id.0))
            .then_with(|| a.location.file_path.cmp(&b.location.file_path))
            .then_with(|| {
                let a_line = a.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);
                let b_line = b.location.span.as_ref().map(|s| s.start_line).unwrap_or(0);
                a_line.cmp(&b_line)
            })
            .then_with(|| a.message.cmp(&b.message))
    });

    Ok(deduped)
}

fn print_rules_catalog(filter_term: Option<&str>) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            Cell::new("Rule ID").fg(Color::Cyan),
            Cell::new("Category"),
            Cell::new("Severity"),
            Cell::new("Title"),
            Cell::new("Description"),
        ]);

    let mut count = 0;
    for rule in ALL_RULES {
        if let Some(term) = filter_term {
            let lower = term.to_lowercase();
            if !rule.id.to_lowercase().contains(&lower)
                && !rule.category.to_lowercase().contains(&lower)
                && !rule.title.to_lowercase().contains(&lower)
                && !rule.description.to_lowercase().contains(&lower)
            {
                continue;
            }
        }

        let sev_cell = match rule.severity {
            "CRITICAL" => Cell::new(rule.severity).fg(Color::DarkRed),
            "ERROR" => Cell::new(rule.severity).fg(Color::Red),
            "WARN" | "WARNING" => Cell::new(rule.severity).fg(Color::Yellow),
            _ => Cell::new(rule.severity).fg(Color::Blue),
        };

        table.add_row(Row::from(vec![
            Cell::new(rule.id),
            Cell::new(rule.category),
            sev_cell,
            Cell::new(rule.title),
            Cell::new(rule.description),
        ]));
        count += 1;
    }

    println!(
        "\n{} (Showing {} of {} rules):",
        "StackVet Diagnostic Rules Catalog".cyan().bold(),
        count,
        ALL_RULES.len()
    );
    println!("{}\n", table);
}

fn print_rule_explanation(rule_id: &str) {
    if let Some(rule) = get_rule_meta(rule_id) {
        println!(
            "\n{}",
            "=======================================================================".cyan()
        );
        println!(
            "  Rule Details: {} - {}",
            rule.id.bold().cyan(),
            rule.title.bold()
        );
        println!(
            "{}\n",
            "=======================================================================".cyan()
        );

        let sev_badge = match rule.severity {
            "CRITICAL" => "[CRITICAL]".red().bold(),
            "ERROR" => "[ERROR]".red(),
            "WARN" | "WARNING" => "[WARN]".yellow(),
            _ => "[INFO]".cyan(),
        };

        println!("  Severity:    {}", sev_badge);
        println!("  Category:    {}", rule.category.bold());
        println!("\n  Description:");
        println!("    {}\n", rule.description);
        println!("  Recommended Remediation:");
        println!("    {}\n", rule.remediation.green());
    } else {
        eprintln!(
            "{}",
            format!(
                "Error: Unknown rule ID '{}'. Run 'stackvet rules' to browse all available rules.",
                rule_id
            )
            .red()
            .bold()
        );
        exit(1);
    }
}

fn build_import_graph(ctx: &ProjectContext) -> ImportGraph {
    let mut graph = ImportGraph::new();
    for file in &ctx.source_files {
        let rel_path = file
            .strip_prefix(&ctx.root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        graph.add_node(rel_path.clone());

        let ext = file.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext == "py" {
            if let Ok(content) = fs::read_to_string(file) {
                let imports = stackvet_python::parse_python_imports(&content, file);
                for imp in imports {
                    if imp.is_relative {
                        let target_str = format!("./{}", imp.module.replace('.', "/"));
                        graph.add_import(rel_path.clone(), target_str);
                    }
                }
            }
        } else if matches!(ext, "ts" | "tsx" | "js" | "jsx") {
            if let Ok(content) = fs::read_to_string(file) {
                let imports = stackvet_node::parse_node_imports(&content, file);
                for imp in imports {
                    if imp.specifier.starts_with('.') {
                        graph.add_import(rel_path.clone(), imp.specifier);
                    }
                }
            }
        }
    }
    graph
}

fn initialize_project(root: &Path, generate_ci: bool) -> Result<()> {
    let config_file = root.join(".stackvet.toml");
    if !config_file.exists() {
        let default_config = r#"# StackVet Project Configuration
[project]
name = "my-application"
min_score = 80
strict = false

[analyzers]
python = true
node = true
docker = true
frameworks = true

[ignore]
rules = []
"#;
        fs::write(&config_file, default_config)?;
        println!(
            "Created configuration file: {}",
            config_file.display().to_string().green()
        );
    } else {
        println!(
            "Configuration file already exists: {}",
            config_file.display().to_string().yellow()
        );
    }

    if generate_ci {
        let workflows_dir = root.join(".github").join("workflows");
        fs::create_dir_all(&workflows_dir)?;
        let workflow_file = workflows_dir.join("stackvet.yml");

        let ci_content = r#"name: StackVet Health & Security Audit

on:
  push:
    branches: [main, master]
  pull_request:

jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install StackVet
        run: cargo install stackvet

      - name: Run CI Gate Check
        run: stackvet check --strict

      - name: Generate Security SARIF Report
        if: always()
        run: stackvet --format sarif doctor > stackvet.sarif

      - name: Upload SARIF to GitHub Security Tab
        uses: github/codeql-action/upload-sarif@v3
        if: always()
        with:
          sarif_file: stackvet.sarif
"#;
        fs::write(&workflow_file, ci_content)?;
        println!(
            "Created GitHub Actions CI workflow: {}",
            workflow_file.display().to_string().green()
        );
    }

    println!("{}", "StackVet initialized successfully!".green().bold());
    Ok(())
}

fn print_score_explanation_details(explanation: &HealthScoreExplanation) {
    println!(
        "\n{}",
        "=======================================================================".cyan()
    );
    println!(
        "{}",
        "  Health Score Explanation & Diagnostic Attribution Details"
            .cyan()
            .bold()
    );
    println!(
        "{}\n",
        "=======================================================================".cyan()
    );

    println!("  Base Score:          {}", explanation.base_score);
    println!(
        "  Total Deductions:   -{} pts",
        explanation.total_deductions
    );
    println!("  Final Score:         {} / 100", explanation.final_score);
    println!("  Letter Grade:        {}", explanation.grade.bold());
    println!(
        "  Unique Findings:     {}",
        explanation.unique_findings_count
    );
    println!(
        "  Duplicates Ignored:  {}\n",
        explanation.duplicate_findings_ignored
    );

    if !explanation.diagnostic_details.is_empty() {
        println!("{}", "Contributing Diagnostics Breakdown:".bold());
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .set_header(vec![
                Cell::new("Rule").fg(Color::Cyan),
                Cell::new("Severity"),
                Cell::new("Penalty"),
                Cell::new("Location"),
                Cell::new("Message"),
            ]);

        for item in &explanation.diagnostic_details {
            let sev_cell = match item.severity {
                Severity::Critical => Cell::new("CRITICAL").fg(Color::DarkRed),
                Severity::Error => Cell::new("ERROR").fg(Color::Red),
                Severity::Warning => Cell::new("WARN").fg(Color::Yellow),
                _ => Cell::new("INFO").fg(Color::Blue),
            };

            let loc_str = if let Some(line) = item.line {
                format!("{}:{}", item.file_path, line)
            } else {
                item.file_path.clone()
            };

            table.add_row(Row::from(vec![
                Cell::new(&item.rule_id),
                sev_cell,
                Cell::new(format!("-{} pts", item.penalty)),
                Cell::new(loc_str),
                Cell::new(&item.message),
            ]));
        }

        println!("{}\n", table);
    } else {
        println!(
            "  {}\n",
            "No diagnostic deductions applied. Project health score is 100%!".green()
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Command: rules
    if let Some(Commands::Rules { filter }) = &cli.command {
        print_rules_catalog(filter.as_deref());
        return Ok(());
    }

    // Command: explain
    if let Some(Commands::Explain { rule_id }) = &cli.command {
        print_rule_explanation(rule_id);
        return Ok(());
    }

    // Determine target directory from subcommand argument or top-level -p option
    let cmd_path = match &cli.command {
        Some(Commands::Doctor { path, .. }) => path.clone(),
        Some(Commands::Check { path, .. }) => path.clone(),
        Some(Commands::Inspect { path, .. }) => path.clone(),
        Some(Commands::Fix { path, .. }) => path.clone(),
        Some(Commands::Graph { path, .. }) => path.clone(),
        Some(Commands::Init { path, .. }) => path.clone(),
        Some(Commands::Baseline { action }) => match action {
            BaselineAction::Record { path, .. } => path.clone(),
            BaselineAction::Verify { path, .. } => path.clone(),
        },
        _ => None,
    };

    let target_dir = cmd_path.unwrap_or(cli.path);
    let root = target_dir.canonicalize().unwrap_or(target_dir);

    if !root.exists() {
        eprintln!(
            "{}",
            format!("Error: Target path does not exist: {}", root.display())
                .red()
                .bold()
        );
        exit(1);
    }

    // Command: init
    if let Some(Commands::Init { ci, .. }) = &cli.command {
        return initialize_project(&root, *ci);
    }

    // Build context
    let ctx = build_full_context(&root)?;
    let diagnostics = collect_all_diagnostics(&ctx, cli.offline)?;
    let explanation = calculate_health_score_explained(&diagnostics);
    let score = explanation.final_score;
    let grade = score_grade(score);

    let correlator = CorrelationEngine::new();
    let root_causes = correlator.correlate(diagnostics.clone());
    let reporter = Reporter::new();

    match &cli.command {
        None | Some(Commands::Doctor { .. }) => {
            let output = match cli.format {
                OutputFormat::Text => reporter.render_text(
                    score,
                    grade,
                    &root_causes,
                    &diagnostics,
                    Some(&explanation),
                ),
                OutputFormat::Json => reporter.render_json(
                    score,
                    grade,
                    &root_causes,
                    &diagnostics,
                    Some(&explanation),
                )?,
                OutputFormat::Sarif => reporter.render_sarif(&diagnostics)?,
                OutputFormat::Html => reporter.render_html(
                    score,
                    grade,
                    &root_causes,
                    &diagnostics,
                    Some(&explanation),
                ),
            };
            println!("{}", output);

            if let Some(Commands::Doctor { explain: true, .. }) = &cli.command {
                if cli.format == OutputFormat::Text {
                    print_score_explanation_details(&explanation);
                }
            }

            if let Some(Commands::Doctor {
                min_score: Some(threshold),
                ..
            }) = &cli.command
            {
                if score < *threshold {
                    eprintln!(
                        "{}",
                        format!(
                            "FAILED: Health score {} is below minimum threshold {}.",
                            score, threshold
                        )
                        .red()
                        .bold()
                    );
                    exit(1);
                }
            }
        }
        Some(Commands::Check { strict, .. }) => {
            let errors = diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error || d.severity == Severity::Critical)
                .count();
            let warnings = diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Warning)
                .count();

            if cli.format == OutputFormat::Text {
                if errors == 0 && (!*strict || warnings == 0) {
                    println!(
                        "{}",
                        "✓ CI Check Passed: No critical issues or errors."
                            .green()
                            .bold()
                    );
                    exit(0);
                } else {
                    eprintln!(
                        "{}",
                        format!(
                            "✗ CI Check Failed: {} error(s), {} warning(s).",
                            errors, warnings
                        )
                        .red()
                        .bold()
                    );
                    for d in &diagnostics {
                        if d.severity >= Severity::Error
                            || (*strict && d.severity >= Severity::Warning)
                        {
                            eprintln!(
                                "  [{}] {}: {}",
                                d.severity.as_str().red(),
                                d.rule_id.as_str().cyan(),
                                d.message
                            );
                        }
                    }
                    exit(1);
                }
            } else {
                let json = reporter.render_json(
                    score,
                    grade,
                    &root_causes,
                    &diagnostics,
                    Some(&explanation),
                )?;
                println!("{}", json);
                if errors > 0 || (*strict && warnings > 0) {
                    exit(1);
                }
            }
        }
        Some(Commands::Inspect { rule, .. }) => {
            let filtered: Vec<_> = if let Some(prefix) = rule {
                diagnostics
                    .into_iter()
                    .filter(|d| d.rule_id.0.starts_with(prefix))
                    .collect()
            } else {
                diagnostics
            };

            println!("\n{}", "--- Detailed Deep Inspection ---".cyan().bold());
            for d in &filtered {
                println!(
                    "\n[{}] {} ({})",
                    d.severity.as_str().red().bold(),
                    d.rule_id.0.cyan().bold(),
                    d.language.as_str()
                );
                println!("  Location: {}", d.location.file_path.display());
                if let Some(span) = &d.location.span {
                    println!("  Line: {}", span.start_line);
                }
                println!("  Message: {}", d.message.bold());
                println!("  Explanation: {}", d.explanation);
                println!("  Remediation: {}", d.suggested_remediation.green());
                if let Some(fix) = &d.fix {
                    println!("  Safe Fix Available: {}", fix.description.yellow());
                }
            }
            println!();
        }
        Some(Commands::Fix {
            yes,
            dry_run,
            rollback,
            path,
        }) => {
            let root = path.as_deref().unwrap_or_else(|| Path::new("."));
            let engine = RemediationEngine::new();

            if *rollback {
                if !engine.has_rollback_session(root) {
                    println!(
                        "{}",
                        "No active rollback session found in target directory.".yellow()
                    );
                    return Ok(());
                }
                let restored = engine.rollback_session(root)?;
                println!(
                    "{}",
                    format!("Successfully rolled back {} file(s):", restored.len())
                        .green()
                        .bold()
                );
                for p in restored {
                    println!("  Restored: {}", p.display().to_string().cyan());
                }
                return Ok(());
            }

            let fixable: Vec<_> = diagnostics.iter().filter_map(|d| d.fix.clone()).collect();
            if fixable.is_empty() {
                println!(
                    "{}",
                    "No automated fixes available for current issues.".yellow()
                );
                return Ok(());
            }

            println!("Found {} automated fix(es):", fixable.len());
            for (idx, fix) in fixable.iter().enumerate() {
                println!(
                    "{}. {} -> {}",
                    idx + 1,
                    fix.file_path.display(),
                    fix.description
                );
            }

            // Generate diff previews
            let previews = engine.preview_fixes(&fixable)?;
            let has_any_diff = previews.iter().any(|p| p.has_changes);
            if has_any_diff {
                println!("\n{}", "--- Proposed Changes (Preview) ---".cyan().bold());
                for preview in &previews {
                    if preview.has_changes {
                        println!("\nFile: {}", preview.file_path.display().to_string().bold());
                        for line in preview.diff.lines() {
                            if line.starts_with('+') && !line.starts_with("+++") {
                                println!("{}", line.green());
                            } else if line.starts_with('-') && !line.starts_with("---") {
                                println!("{}", line.red());
                            } else if line.starts_with('@') {
                                println!("{}", line.cyan());
                            } else {
                                println!("{}", line);
                            }
                        }
                    }
                }
            }

            if *dry_run {
                println!("\n{}", "Dry-run complete. No files were modified.".yellow());
                return Ok(());
            }

            if !yes {
                println!("\nRun with '--yes' to apply all fixes automatically.");
                return Ok(());
            }

            let report = engine.apply_fixes_transactional(&fixable, Some(root))?;
            for fix in &fixable {
                println!("Applied fix: {}", fix.description.green());
            }
            println!(
                "{}",
                format!(
                    "All safe fixes applied successfully ({} file(s) updated)!",
                    report.applied_count
                )
                .green()
                .bold()
            );
            println!(
                "{}",
                "To revert these changes, run 'stackvet fix --rollback'.".cyan()
            );
        }
        Some(Commands::Graph { style, .. }) => {
            let graph = build_import_graph(&ctx);
            let rendered = match style {
                GraphFormat::Ascii => graph.to_ascii(),
                GraphFormat::Mermaid => graph.to_mermaid(),
                GraphFormat::Dot => graph.to_dot(),
            };
            println!("{}", rendered);
        }
        Some(Commands::Baseline { action }) => {
            match action {
                BaselineAction::Record { output, .. } => {
                    let baseline_file = if output.is_absolute() {
                        output.clone()
                    } else if output
                        .parent()
                        .map(|p| p.as_os_str().is_empty())
                        .unwrap_or(true)
                    {
                        root.join(output)
                    } else {
                        output.clone()
                    };
                    let baseline_data = BaselineData::from_diagnostics(&diagnostics);
                    baseline_data.save_to_file(&baseline_file)?;
                    println!(
                        "{}",
                        format!(
                            "Baseline recorded successfully with {} issue fingerprint(s) to '{}'.",
                            baseline_data.issue_fingerprints.len(),
                            baseline_file.display()
                        )
                        .green()
                        .bold()
                    );
                }
                BaselineAction::Verify {
                    baseline, strict, ..
                } => {
                    let baseline_file = if baseline.is_absolute() || baseline.is_file() {
                        baseline.clone()
                    } else {
                        root.join(baseline)
                    };
                    if !baseline_file.is_file() {
                        eprintln!("{}", format!("Baseline file not found: '{}'. Run 'stackvet baseline record' first.", baseline_file.display()).red().bold());
                        exit(1);
                    }

                    let baseline_data = BaselineData::load_from_file(&baseline_file)?;
                    let new_issues = baseline_data.filter_new_issues(diagnostics);

                    if new_issues.is_empty() {
                        println!(
                            "{}",
                            "✓ Baseline Verified: No new regressions detected."
                                .green()
                                .bold()
                        );
                        exit(0);
                    } else {
                        let errors = new_issues
                            .iter()
                            .filter(|d| d.severity >= Severity::Error)
                            .count();
                        let warnings = new_issues
                            .iter()
                            .filter(|d| d.severity == Severity::Warning)
                            .count();

                        eprintln!(
                        "{}",
                        format!("✗ Baseline Regression Detected: {} new error(s), {} new warning(s).", errors, warnings).red().bold()
                    );
                        for d in &new_issues {
                            if d.severity >= Severity::Error
                                || (*strict && d.severity >= Severity::Warning)
                            {
                                eprintln!(
                                    "  [{}] {}: {}",
                                    d.severity.as_str().red(),
                                    d.rule_id.as_str().cyan(),
                                    d.message
                                );
                            }
                        }

                        if errors > 0 || (*strict && warnings > 0) {
                            exit(1);
                        }
                    }
                }
            }
        }
        Some(Commands::Explain { .. })
        | Some(Commands::Rules { .. })
        | Some(Commands::Init { .. }) => {
            // Handled above
        }
    }

    Ok(())
}
