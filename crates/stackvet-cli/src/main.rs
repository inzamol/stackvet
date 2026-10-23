use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{
    Attribute, Cell, Color, ColumnConstraint, ContentArrangement, Row, Table, Width,
};
use stackvet_core::{
    apply_suppressions, config::parse_severity, get_rule_meta, BaselineData, ConfigError, ExitCode,
    HealthScoreExplanation, ProjectContext, Severity, StackvetConfig, ALL_RULES,
};
use stackvet_correlation::{
    calculate_health_score_explained, deduplicate_diagnostics, score_grade, CorrelationEngine,
};
use stackvet_frameworks::run_all_framework_analyzers;
use stackvet_graph::ImportGraph;
use stackvet_node::detect_node_project_with_diagnostics;
use stackvet_python::detect_python_project_with_diagnostics;
use stackvet_remediation::RemediationEngine;
use stackvet_reporter::Reporter;
use stackvet_scanner::{
    collect_git_diff, hash_bytes, hash_file_content, normalize_rel_path, parse_explicit_files,
    parse_patch_file, CacheValidationResult, FileCacheEntry, IncrementalCacheManager,
    PrAnalysisResult, ProjectScanner, ScanCacheData,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

mod bug_report;

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
        global = true,
        help = "Output report format"
    )]
    pub format: OutputFormat,

    #[arg(
        short,
        long,
        global = true,
        help = "Path to custom configuration file (.toml)"
    )]
    pub config: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        help = "Run offline without querying vulnerability databases"
    )]
    pub offline: bool,

    #[arg(
        long = "refresh-vuln-cache",
        global = true,
        help = "Force refresh of local vulnerability advisory database cache"
    )]
    pub refresh_vuln_cache: bool,

    #[arg(
        long,
        global = true,
        help = "Maximum duration in seconds before halting scan with a timeout"
    )]
    pub timeout: Option<u64>,

    #[arg(
        long = "ignore",
        help = "Additional glob pattern to ignore (can be specified multiple times)",
        action = clap::ArgAction::Append
    )]
    pub ignore: Vec<String>,

    #[arg(
        short,
        long,
        global = true,
        help = "Show verbose diagnostic and scan details"
    )]
    pub verbose: bool,

    #[arg(
        long,
        global = true,
        help = "Enable incremental scanning reusing cached file diagnostics"
    )]
    pub incremental: bool,

    #[arg(long, global = true, help = "Disable scan caching (force full scan)")]
    pub no_cache: bool,

    #[arg(
        long,
        global = true,
        help = "Custom directory for incremental scan cache"
    )]
    pub cache_dir: Option<PathBuf>,

    #[arg(
        long,
        help = "Fail with exit code 4 if analysis is incomplete due to unreadable, malformed, or skipped files"
    )]
    pub fail_on_incomplete: bool,

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

        #[arg(long, help = "Path to baseline file to compare against")]
        baseline: Option<PathBuf>,

        #[arg(
            long = "ignore",
            help = "Additional glob pattern to ignore (can be specified multiple times)",
            action = clap::ArgAction::Append
        )]
        ignore: Vec<String>,
    },

    #[command(
        about = "Fast CI gate (returns 0 for clean, 1 for errors)",
        aliases = ["scan", "audit"]
    )]
    Check {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(
            long,
            help = "Treat warnings as errors (equivalent to --fail-on warning)"
        )]
        strict: bool,

        #[arg(
            long,
            help = "Fail CI if findings meet or exceed this severity: hint, info, warning, error, critical"
        )]
        fail_on: Option<String>,

        #[arg(long, help = "Path to baseline file to compare against")]
        baseline: Option<PathBuf>,

        #[arg(
            long,
            help = "Update baseline file with current findings and remove resolved issues"
        )]
        update_baseline: bool,

        #[arg(
            long,
            help = "Fail with exit code 4 if analysis is incomplete due to unreadable, malformed, or skipped files"
        )]
        fail_on_incomplete: bool,

        #[arg(
            long = "ignore",
            help = "Additional glob pattern to ignore (can be specified multiple times)",
            action = clap::ArgAction::Append
        )]
        ignore: Vec<String>,
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

        #[arg(
            long = "ignore",
            help = "Additional glob pattern to ignore (can be specified multiple times)",
            action = clap::ArgAction::Append
        )]
        ignore: Vec<String>,
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

        #[arg(
            long,
            help = "Verify fixes by re-running diagnostics and confirming resolution"
        )]
        verify: bool,
    },

    #[command(
        about = "Verify that applied fixes resolved issues without introducing regressions",
        aliases = ["check-fixes"]
    )]
    Verify {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,
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

    #[command(
        about = "Pull request intelligence analyzing changed code vs base branch",
        aliases = ["pr-analysis"]
    )]
    Pr {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(
            short = 'b',
            long,
            help = "Base git branch or ref (default: auto-detected or origin/main)"
        )]
        base: Option<String>,

        #[arg(long, default_value = "HEAD", help = "Head git commit or ref")]
        head: String,

        #[arg(
            long,
            help = "Comma-separated list of changed files (e.g. 'users.py,requirements.txt')"
        )]
        files: Option<String>,

        #[arg(long, help = "Path to unified diff patch file")]
        diff: Option<PathBuf>,

        #[arg(long, help = "Treat warnings as errors")]
        strict: bool,

        #[arg(
            long,
            help = "Run non-interactively with step summary & inline annotations"
        )]
        ci: bool,

        #[arg(
            long,
            help = "Emits GitHub Actions inline workflow command annotations"
        )]
        github_annotations: bool,

        #[arg(
            long,
            help = "Outputs formatted Markdown suitable for GitHub PR comments"
        )]
        comment: bool,

        #[arg(long, help = "Emits PR analysis results as structured JSON")]
        json: bool,

        #[arg(long, help = "Emits new PR findings in standard SARIF v2.1.0 format")]
        sarif: bool,

        #[arg(long, help = "Compare against an established baseline file")]
        baseline: Option<PathBuf>,

        #[arg(short = 'o', long, help = "Writes report directly to a file")]
        output: Option<PathBuf>,
    },

    #[command(
        about = "Generate a reproducible, sanitized environment and diagnostic report for bug reporting",
        aliases = ["report-bug", "sysinfo"]
    )]
    BugReport {
        #[arg(help = "Target project directory [default: .]")]
        path: Option<PathBuf>,

        #[arg(short = 'o', long, help = "Output report to file instead of stdout")]
        output: Option<PathBuf>,

        #[arg(long, help = "Output report as JSON")]
        json: bool,
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
            help = "Baseline filepath to check against [default: .stackvet-baseline.json]"
        )]
        baseline: Option<PathBuf>,

        #[arg(long, help = "Treat warnings as errors")]
        strict: bool,

        #[arg(
            long,
            help = "Fail CI if findings meet or exceed this severity: hint, info, warning, error, critical"
        )]
        fail_on: Option<String>,

        #[arg(
            long,
            help = "Update baseline file with current findings and remove resolved issues"
        )]
        update: bool,
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

fn find_enclosing_project_root(path: &Path) -> PathBuf {
    let start = if path.is_file() {
        path.parent().unwrap_or(path)
    } else {
        path
    };
    let mut curr = Some(start);
    while let Some(dir) = curr {
        if dir.join("package.json").is_file()
            || dir.join("pyproject.toml").is_file()
            || dir.join("requirements.txt").is_file()
            || dir.join("Cargo.toml").is_file()
            || dir.join(".git").exists()
        {
            return dir.to_path_buf();
        }
        curr = dir.parent();
    }
    start.to_path_buf()
}

fn build_full_context(
    target: &Path,
    project_root: &Path,
    config: &StackvetConfig,
    cli_ignores: &[String],
    verbose: bool,
    cancellation_token: Option<stackvet_core::CancellationToken>,
) -> Result<ProjectContext> {
    let scan_root = if target.is_file() {
        project_root
    } else {
        target
    };

    let mut scanner = ProjectScanner::new(scan_root)
        .with_default_exclusions(config.ignore.use_defaults())
        .with_ignores(config.ignore.patterns().iter().cloned())
        .with_ignores(cli_ignores.iter().cloned())
        .with_max_file_size(config.scanner.max_file_size)
        .with_max_depth(config.scanner.max_depth)
        .with_max_files(config.scanner.max_files)
        .with_follow_symlinks(config.scanner.follow_symlinks)
        .with_verbose(verbose);
    if let Some(token) = cancellation_token {
        scanner = scanner.with_cancellation_token(token);
    }
    let mut ctx = scanner.build_context()?;
    ctx.root = project_root.to_path_buf();

    if target.is_file() {
        let canonical_target = target
            .canonicalize()
            .unwrap_or_else(|_| target.to_path_buf());
        let already_included = ctx.source_files.iter().any(|f| {
            f == target
                || f == &canonical_target
                || f.canonicalize()
                    .map(|cf| cf == canonical_target)
                    .unwrap_or(false)
        });
        if !already_included {
            ctx.source_files.push(target.to_path_buf());
        }
    }

    let (py_proj, py_diags) = detect_python_project_with_diagnostics(project_root);
    ctx.python = py_proj;
    ctx.discovery_diagnostics.extend(py_diags);

    let (node_proj, node_diags) = detect_node_project_with_diagnostics(project_root);
    ctx.node = node_proj;
    ctx.discovery_diagnostics.extend(node_diags);
    ctx.architecture = Some(config.architecture.clone());
    Ok(ctx)
}

fn sort_diagnostics_deterministic(diagnostics: &mut [stackvet_core::Diagnostic]) {
    diagnostics.sort_by(|a, b| {
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
}

fn collect_all_diagnostics(
    ctx: &ProjectContext,
    config: &StackvetConfig,
    offline: bool,
    refresh_vuln_cache: bool,
) -> Result<(Vec<stackvet_core::Diagnostic>, usize)> {
    let mut diagnostics =
        stackvet_analyzers::run_all_analyzers_with_options(ctx, offline, refresh_vuln_cache)?;
    let mut fw_diags = run_all_framework_analyzers(ctx)?;
    diagnostics.append(&mut fw_diags);

    // Custom user rules evaluation (Issue #27)
    let custom_diags = stackvet_core::config::evaluate_custom_rules(
        &ctx.root,
        &ctx.source_files,
        &config.custom_rules,
    );
    diagnostics.extend(custom_diags);

    // Deduplicate diagnostics to prevent duplicate findings from distorting scores or reports
    let (mut deduped, duplicates_count) = deduplicate_diagnostics(&diagnostics);
    sort_diagnostics_deterministic(&mut deduped);

    Ok((deduped, duplicates_count))
}

#[allow(clippy::too_many_arguments)]
fn collect_diagnostics_incremental(
    root: &Path,
    ctx: &ProjectContext,
    config: &StackvetConfig,
    cli_ignores: &[String],
    offline: bool,
    refresh_vuln_cache: bool,
    verbose: bool,
    custom_cache_dir: Option<&Path>,
) -> Result<(Vec<stackvet_core::Diagnostic>, usize, bool)> {
    let resolve_cache_dir = |dir: &Path| -> PathBuf {
        if dir.is_absolute() {
            dir.to_path_buf()
        } else {
            root.join(dir)
        }
    };
    let cache_mgr = if let Some(dir) = custom_cache_dir {
        IncrementalCacheManager::with_custom_cache_dir(root, resolve_cache_dir(dir))
    } else if let Some(dir) = &config.cache.dir {
        IncrementalCacheManager::with_custom_cache_dir(root, resolve_cache_dir(dir))
    } else {
        IncrementalCacheManager::new(root)
    };

    let config_val = serde_json::to_value(config)
        .context("Failed to serialize configuration for cache validation")?;
    let mut config_bytes = serde_json::to_vec(&config_val)
        .context("Failed to serialize configuration value for cache validation")?;
    for ign in cli_ignores {
        config_bytes.push(b'\0');
        config_bytes.extend_from_slice(ign.as_bytes());
    }
    let config_hash = hash_bytes(&config_bytes);

    let validation = cache_mgr.validate(&ctx.manifest_files, &ctx.source_files, &config_hash);

    match validation {
        CacheValidationResult::Valid {
            mut cached_data,
            changes,
        } => {
            if !changes.has_changes() {
                if verbose {
                    println!(
                        "{}",
                        format!(
                            "Incremental scan: All {} file(s) up-to-date (cache hit).",
                            ctx.source_files.len()
                        )
                        .green()
                    );
                }
                let mut all_diags: Vec<stackvet_core::Diagnostic> = cached_data
                    .files
                    .values()
                    .flat_map(|e| e.diagnostics.clone())
                    .collect();
                all_diags.extend(cached_data.project_diagnostics.clone());

                let (mut deduped, duplicates_count) = deduplicate_diagnostics(&all_diags);
                sort_diagnostics_deterministic(&mut deduped);
                return Ok((deduped, duplicates_count, true));
            }

            if verbose {
                println!(
                    "{}",
                    format!(
                        "Incremental scan: {} file(s) reused from cache, {} file(s) re-analyzed.",
                        changes.unchanged_files.len(),
                        changes.modified_files.len() + changes.added_files.len()
                    )
                    .cyan()
                );
            }

            let unchanged_set: HashSet<String> = changes
                .unchanged_files
                .iter()
                .map(|p| normalize_rel_path(root, p))
                .collect();

            // 1. Restore cached diagnostics for unchanged source files
            let mut all_diags: Vec<stackvet_core::Diagnostic> = Vec::new();
            for (rel_path, entry) in &cached_data.files {
                if unchanged_set.contains(rel_path) {
                    all_diags.extend(entry.diagnostics.clone());
                }
            }

            // 2. Identify modified and added files
            let mut reanalyze_files = changes.modified_files.clone();
            reanalyze_files.extend(changes.added_files.clone());
            let reanalyze_set: HashSet<PathBuf> = reanalyze_files.iter().cloned().collect();
            let source_files_set: HashSet<PathBuf> = ctx.source_files.iter().cloned().collect();

            let is_source_file = |p: &Path| -> bool {
                source_files_set.contains(p) || source_files_set.contains(&root.join(p))
            };
            let is_reanalyzed = |p: &Path| -> bool {
                reanalyze_set.contains(p) || reanalyze_set.contains(&root.join(p))
            };

            // 3. Project-wide analyzers receive the full ctx.source_files so rules inspecting
            // full sets (unused deps, lockfile drift, import graph cycles) remain sound.
            let mut project_wide_diags = stackvet_analyzers::run_all_analyzers_with_options(
                ctx,
                offline,
                refresh_vuln_cache,
            )?;
            let custom_diags = stackvet_core::config::evaluate_custom_rules(
                &ctx.root,
                &ctx.source_files,
                &config.custom_rules,
            );
            project_wide_diags.extend(custom_diags);

            // 4. Per-file framework analyzers only inspect reanalyzed files
            let mut sub_ctx = ctx.clone();
            sub_ctx.source_files = reanalyze_files.clone();
            let fresh_fw_diags = run_all_framework_analyzers(&sub_ctx)?;

            let mut fresh_diags = project_wide_diags;
            fresh_diags.extend(fresh_fw_diags);

            let mut fresh_by_file: HashMap<PathBuf, Vec<stackvet_core::Diagnostic>> =
                HashMap::new();
            let mut fresh_project_diags: Vec<stackvet_core::Diagnostic> = Vec::new();

            for d in fresh_diags {
                if is_reanalyzed(&d.location.file_path) {
                    all_diags.push(d.clone());
                    let key = if d.location.file_path.is_absolute() {
                        d.location.file_path.clone()
                    } else {
                        root.join(&d.location.file_path)
                    };
                    fresh_by_file.entry(key).or_default().push(d);
                } else if !is_source_file(&d.location.file_path) {
                    fresh_project_diags.push(d);
                }
            }

            // Replace cached project diagnostics with fresh results
            cached_data.project_diagnostics = fresh_project_diags;
            all_diags.extend(cached_data.project_diagnostics.clone());

            // Remove deleted files from cache
            for del in &changes.deleted_files {
                let rel = normalize_rel_path(root, del);
                cached_data.files.remove(&rel);
            }

            // Update cache entries for reanalyzed files
            for p in &reanalyze_files {
                let rel = normalize_rel_path(root, p);
                if let Ok(metadata) = fs::metadata(p) {
                    let file_size = metadata.len();
                    let mtime = metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    let content_hash = hash_file_content(p).unwrap_or_default();
                    let diags = fresh_by_file
                        .remove(p)
                        .or_else(|| fresh_by_file.remove(&root.join(p)))
                        .unwrap_or_default();

                    cached_data.files.insert(
                        rel,
                        FileCacheEntry {
                            content_hash,
                            file_size,
                            mtime,
                            diagnostics: diags,
                        },
                    );
                }
            }

            let _ = cache_mgr.save(&cached_data);

            let (mut deduped, duplicates_count) = deduplicate_diagnostics(&all_diags);
            sort_diagnostics_deterministic(&mut deduped);
            Ok((deduped, duplicates_count, false))
        }
        CacheValidationResult::Invalid(reason) => {
            if verbose {
                println!(
                    "{}",
                    format!("Incremental scan cache invalidated: {reason}. Running full scan.")
                        .yellow()
                );
            }

            let (all_diags, duplicates_count) =
                collect_all_diagnostics(ctx, config, offline, refresh_vuln_cache)?;

            let mut new_cache = ScanCacheData::new(config_hash);
            for m in &ctx.manifest_files {
                let rel = normalize_rel_path(root, m);
                if let Ok(h) = hash_file_content(m) {
                    new_cache.manifest_hashes.insert(rel, h);
                }
            }

            let mut diags_by_file: HashMap<PathBuf, Vec<stackvet_core::Diagnostic>> =
                HashMap::new();
            for d in &all_diags {
                let key = if d.location.file_path.is_absolute() {
                    d.location.file_path.clone()
                } else {
                    root.join(&d.location.file_path)
                };
                diags_by_file.entry(key).or_default().push(d.clone());
            }

            for p in &ctx.source_files {
                let rel = normalize_rel_path(root, p);
                if let Ok(metadata) = fs::metadata(p) {
                    let file_size = metadata.len();
                    let mtime = metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    let content_hash = hash_file_content(p).unwrap_or_default();
                    let diags = diags_by_file
                        .remove(p)
                        .or_else(|| diags_by_file.remove(&root.join(p)))
                        .unwrap_or_default();

                    new_cache.files.insert(
                        rel,
                        FileCacheEntry {
                            content_hash,
                            file_size,
                            mtime,
                            diagnostics: diags,
                        },
                    );
                }
            }

            // Save all remaining diags_by_file entries into project_diagnostics
            let mut remaining_project_diags: Vec<stackvet_core::Diagnostic> = Vec::new();
            for diags in diags_by_file.into_values() {
                remaining_project_diags.extend(diags);
            }
            new_cache.project_diagnostics = remaining_project_diags;

            let _ = cache_mgr.save(&new_cache);

            Ok((all_diags, duplicates_count, false))
        }
    }
}

fn print_rules_catalog(filter_term: Option<&str>) {
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w.saturating_sub(2))
        .unwrap_or(100)
        .clamp(80, 130);

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(term_width)
        .set_header(vec![
            Cell::new("Rule ID")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new("Category")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new("Severity")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new("Title")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new("Description")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
        ]);

    if let Some(col) = table.column_mut(0) {
        col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(10)));
    }
    if let Some(col) = table.column_mut(1) {
        col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(14)));
    }
    if let Some(col) = table.column_mut(2) {
        col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(10)));
    }
    if let Some(col) = table.column_mut(3) {
        col.set_constraint(ColumnConstraint::UpperBoundary(Width::Fixed(32)));
    }

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
            "CRITICAL" => Cell::new(rule.severity)
                .fg(Color::DarkRed)
                .add_attribute(Attribute::Bold),
            "ERROR" => Cell::new(rule.severity).fg(Color::Red),
            "WARN" | "WARNING" => Cell::new(rule.severity).fg(Color::Yellow),
            _ => Cell::new(rule.severity).fg(Color::Blue),
        };

        table.add_row(Row::from(vec![
            Cell::new(rule.id).fg(Color::Cyan),
            Cell::new(rule.category),
            sev_cell,
            Cell::new(rule.title),
            Cell::new(rule.description),
        ]));
        count += 1;
    }

    if count == 0 {
        if let Some(term) = filter_term {
            println!(
                "\n{} No diagnostic rules matched '{}'. Run 'stackvet rules' to browse all rules.\n",
                "Notice:".yellow().bold(),
                term
            );
        } else {
            println!("\nNo diagnostic rules available.\n");
        }
        return;
    }

    println!(
        "\n{} (Showing {} of {} rules):",
        "StackVet Diagnostic Rules Catalog".cyan().bold(),
        count,
        ALL_RULES.len()
    );
    println!("{}\n", table);
    println!(
        "  {} Run {} to inspect rationale and remediation guidance.\n",
        "Tip:".green().bold(),
        "stackvet explain <RULE_ID>".cyan().bold()
    );
}

fn print_rule_explanation(rule_id: &str) {
    if let Some(rule) = get_rule_meta(rule_id) {
        println!();
        println!(
            "{}",
            format!("─── Diagnostic Rule Details: {} ───", rule.id)
                .cyan()
                .bold()
        );
        println!("  Title:       {}\n", rule.title.bold());

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
        ExitCode::ConfigurationError.exit();
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

[thresholds]
fail_on = "error"
min_score = 80

[ci]
fail_on = "error"
strict = false

[rules]
# Example per-rule overrides:
# DEP-001 = "warning"
# CEL-001 = "disabled"
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
    let title = "Health Score Explanation & Diagnostic Attribution Details";
    let banner = format!(
        "╭─────────────────────────────────────────────────────────────────────╮\n\
         │     {}     │\n\
         ╰─────────────────────────────────────────────────────────────────────╯\n",
        title
    );
    println!("\n{}", banner.cyan().bold());

    let grade_badge = match explanation.grade.chars().next().unwrap_or('F') {
        'A' => format!("[ Grade: {} ]", explanation.grade).green().bold(),
        'B' => format!("[ Grade: {} ]", explanation.grade).yellow().bold(),
        'C' => format!("[ Grade: {} ]", explanation.grade)
            .truecolor(255, 140, 0)
            .bold(),
        _ => format!("[ Grade: {} ]", explanation.grade).red().bold(),
    };

    println!("  Base Score:          {} pts", explanation.base_score);
    println!(
        "  Total Deductions:   -{} pts",
        explanation.total_deductions
    );
    println!(
        "  Final Score:         {} / 100   {}",
        explanation.final_score, grade_badge
    );
    println!(
        "  Findings Breakdown:  {} unique finding(s), {} duplicate(s) ignored\n",
        explanation.unique_findings_count, explanation.duplicate_findings_ignored
    );

    if !explanation.diagnostic_details.is_empty() {
        println!(
            "{}",
            "─── Contributing Diagnostics Breakdown ────────────────────────────────"
                .cyan()
                .bold()
        );
        let term_width = crossterm::terminal::size()
            .map(|(w, _)| w.saturating_sub(2))
            .unwrap_or(100)
            .clamp(80, 130);

        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_width(term_width)
            .set_header(vec![
                Cell::new("Rule")
                    .fg(Color::Cyan)
                    .add_attribute(Attribute::Bold),
                Cell::new("Severity")
                    .fg(Color::Cyan)
                    .add_attribute(Attribute::Bold),
                Cell::new("Penalty")
                    .fg(Color::Cyan)
                    .add_attribute(Attribute::Bold),
                Cell::new("Location")
                    .fg(Color::Cyan)
                    .add_attribute(Attribute::Bold),
                Cell::new("Message")
                    .fg(Color::Cyan)
                    .add_attribute(Attribute::Bold),
            ]);

        if let Some(col) = table.column_mut(0) {
            col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(9)));
        }
        if let Some(col) = table.column_mut(1) {
            col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(9)));
        }
        if let Some(col) = table.column_mut(2) {
            col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(9)));
        }
        if let Some(col) = table.column_mut(3) {
            col.set_constraint(ColumnConstraint::UpperBoundary(Width::Fixed(35)));
        }

        for item in &explanation.diagnostic_details {
            let sev_cell = match item.severity {
                Severity::Critical => Cell::new("CRITICAL")
                    .fg(Color::DarkRed)
                    .add_attribute(Attribute::Bold),
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
                Cell::new(&item.rule_id).fg(Color::Cyan),
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

fn scan_and_collect_diagnostics(
    root: &Path,
    cli_ignore: &[String],
    offline: bool,
    config: &StackvetConfig,
) -> Result<Vec<stackvet_core::Diagnostic>> {
    let project_root = if root.is_file() {
        find_enclosing_project_root(root)
    } else {
        root.to_path_buf()
    };
    let ctx = build_full_context(root, &project_root, config, cli_ignore, false, None)?;
    let (raw, _) = collect_all_diagnostics(&ctx, config, offline, false)?;
    let after_rules = config.apply_rule_overrides(raw)?;
    let outcome = apply_suppressions(&project_root, &config.suppressions, after_rules, None);
    let mut diagnostics = outcome.active_diagnostics;
    if root.is_file() {
        let canonical_target = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        diagnostics.retain(|d| {
            let p = &d.location.file_path;
            let full_p = if p.is_relative() {
                project_root.join(p)
            } else {
                p.clone()
            };
            full_p == root
                || full_p == canonical_target
                || full_p
                    .canonicalize()
                    .map(|cp| cp == canonical_target)
                    .unwrap_or(false)
        });
    }
    Ok(diagnostics)
}

fn print_verification_summary(result: &stackvet_remediation::FixVerificationResult) {
    let term_width = crossterm::terminal::size()
        .map(|(w, _)| w.saturating_sub(2))
        .unwrap_or(100)
        .min(130);

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(term_width)
        .set_header(Row::from(vec![
            Cell::new("Status")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new("Rule")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new("File")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
            Cell::new("Details")
                .fg(Color::Cyan)
                .add_attribute(Attribute::Bold),
        ]));

    if let Some(col) = table.column_mut(0) {
        col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(12)));
    }
    if let Some(col) = table.column_mut(1) {
        col.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(9)));
    }
    if let Some(col) = table.column_mut(2) {
        col.set_constraint(ColumnConstraint::UpperBoundary(Width::Fixed(35)));
    }

    for d in &result.resolved_diagnostics {
        table.add_row(Row::from(vec![
            Cell::new("RESOLVED").fg(Color::Green),
            Cell::new(&d.rule_id.0).fg(Color::Cyan),
            Cell::new(d.location.file_path.display().to_string()),
            Cell::new(&d.message),
        ]));
    }

    for d in &result.unresolved_diagnostics {
        table.add_row(Row::from(vec![
            Cell::new("UNRESOLVED").fg(Color::Yellow),
            Cell::new(&d.rule_id.0).fg(Color::Cyan),
            Cell::new(d.location.file_path.display().to_string()),
            Cell::new(&d.message),
        ]));
    }

    for d in &result.new_regressions {
        table.add_row(Row::from(vec![
            Cell::new("REGRESSION").fg(Color::Red),
            Cell::new(&d.rule_id.0).fg(Color::Cyan),
            Cell::new(d.location.file_path.display().to_string()),
            Cell::new(&d.message),
        ]));
    }

    println!("\n{}", "--- Fix Verification Report ---".cyan().bold());
    println!("{}\n", table);

    if result.is_clean {
        println!(
            "{}",
            format!(
                "Verification SUCCESS: {} issue(s) confirmed resolved with 0 regressions.",
                result.resolved_diagnostics.len()
            )
            .green()
            .bold()
        );
    } else {
        eprintln!(
            "{}",
            format!(
                "Verification WARNING: {} unresolved issue(s), {} new regression(s) detected.",
                result.unresolved_diagnostics.len(),
                result.new_regressions.len()
            )
            .red()
            .bold()
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
        Some(Commands::Verify { path }) => path.clone(),
        Some(Commands::Graph { path, .. }) => path.clone(),
        Some(Commands::Init { path, .. }) => path.clone(),
        Some(Commands::Pr { path, .. }) => path.clone(),
        Some(Commands::BugReport { path, .. }) => path.clone(),
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
        ExitCode::ExecutionError.exit();
    }

    let project_root = if root.is_file() {
        find_enclosing_project_root(&root)
    } else {
        root.clone()
    };

    // Load project configuration (explicit path or discovered in project directory)
    let config = if let Some(cfg_path) = &cli.config {
        let p = if cfg_path.is_absolute() {
            cfg_path.clone()
        } else {
            project_root.join(cfg_path)
        };
        match StackvetConfig::from_file(&p) {
            Ok(cfg) => cfg,
            Err(ConfigError::NotFound(path_str)) => {
                eprintln!(
                    "{}",
                    format!("Configuration file not found: {path_str}")
                        .red()
                        .bold()
                );
                ExitCode::ExecutionError.exit();
            }
            Err(ConfigError::IoError(e)) => {
                eprintln!(
                    "{}",
                    format!("Failed to read configuration: {e}").red().bold()
                );
                ExitCode::ExecutionError.exit();
            }
            Err(ConfigError::ParseError(e)) | Err(ConfigError::ValidationError(e)) => {
                eprintln!("{}", format!("Invalid configuration: {e}").red().bold());
                ExitCode::ConfigurationError.exit();
            }
        }
    } else {
        match StackvetConfig::discover_in_dir(&project_root) {
            Ok(Some(cfg)) => cfg,
            Ok(None) => StackvetConfig::default(),
            Err(ConfigError::ParseError(e)) | Err(ConfigError::ValidationError(e)) => {
                eprintln!("{}", format!("Invalid configuration: {e}").red().bold());
                ExitCode::ConfigurationError.exit();
            }
            Err(e) => {
                eprintln!("{}", format!("Configuration error: {e}").red().bold());
                ExitCode::ExecutionError.exit();
            }
        }
    };

    // Command: init
    if let Some(Commands::Init { ci, .. }) = &cli.command {
        initialize_project(&project_root, *ci)?;
        ExitCode::Success.exit();
    }

    // Gather all CLI ignore patterns from global and subcommand flags
    let mut all_cli_ignores = cli.ignore.clone();
    match &cli.command {
        Some(Commands::Doctor { ignore, .. }) => all_cli_ignores.extend(ignore.clone()),
        Some(Commands::Check { ignore, .. }) => all_cli_ignores.extend(ignore.clone()),
        Some(Commands::Inspect { ignore, .. }) => all_cli_ignores.extend(ignore.clone()),
        _ => {}
    }

    let cancellation_token = stackvet_core::CancellationToken::new();

    if let Some(timeout_secs) = cli.timeout {
        let token = cancellation_token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(timeout_secs)).await;
            token.cancel();
        });
    }

    {
        let token = cancellation_token.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("\n{}", "Scan interrupted by user (SIGINT)".yellow().bold());
                token.cancel();
            }
        });
    }

    // Build context with ignore filtering (CLI --ignore > config.ignore > .stackvetignore > default exclusions)
    let ctx = match build_full_context(
        &root,
        &project_root,
        &config,
        &all_cli_ignores,
        cli.verbose,
        Some(cancellation_token.clone()),
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{}",
                format!("Failed to analyze project context: {e}")
                    .red()
                    .bold()
            );
            ExitCode::ExecutionError.exit();
        }
    };

    if cancellation_token.is_cancelled() {
        eprintln!(
            "{}",
            "Scan cancelled due to timeout or user interruption."
                .red()
                .bold()
        );
        ExitCode::Cancelled.exit();
    }

    let use_incremental = (cli.incremental || config.cache.enabled) && !cli.no_cache;
    let (raw_diagnostics, duplicates_ignored) = if use_incremental {
        match collect_diagnostics_incremental(
            &project_root,
            &ctx,
            &config,
            &all_cli_ignores,
            cli.offline,
            cli.refresh_vuln_cache,
            cli.verbose,
            cli.cache_dir.as_deref(),
        ) {
            Ok((diags, dupes, _)) => (diags, dupes),
            Err(e) => {
                if cli.verbose {
                    eprintln!("{}", format!("Incremental scan fallback: {e}").yellow());
                }
                match collect_all_diagnostics(&ctx, &config, cli.offline, cli.refresh_vuln_cache) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!(
                            "{}",
                            format!("Failed to collect diagnostics: {e}").red().bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
                }
            }
        }
    } else {
        match collect_all_diagnostics(&ctx, &config, cli.offline, cli.refresh_vuln_cache) {
            Ok(d) => d,
            Err(e) => {
                eprintln!(
                    "{}",
                    format!("Failed to collect diagnostics: {e}").red().bold()
                );
                ExitCode::ExecutionError.exit();
            }
        }
    };
    let diagnostics_after_rules = match config.apply_rule_overrides(raw_diagnostics) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}", format!("Invalid configuration: {e}").red().bold());
            ExitCode::ConfigurationError.exit();
        }
    };

    // Apply configured suppressions and inline comment suppressions
    let suppression_outcome = apply_suppressions(
        &project_root,
        &config.suppressions,
        diagnostics_after_rules,
        None,
    );
    let mut diagnostics = suppression_outcome.active_diagnostics;
    let suppressed_diagnostics = suppression_outcome.suppressed;

    // Preserve file-scoped behavior: filter diagnostics to the target file if invoked on a file
    if root.is_file() {
        let canonical_target = root.canonicalize().unwrap_or_else(|_| root.clone());
        diagnostics.retain(|d| {
            let p = &d.location.file_path;
            let full_p = if p.is_relative() {
                project_root.join(p)
            } else {
                p.clone()
            };
            full_p == root
                || full_p == canonical_target
                || full_p
                    .canonicalize()
                    .map(|cp| cp == canonical_target)
                    .unwrap_or(false)
        });
    }

    let mut explanation = calculate_health_score_explained(&diagnostics);
    explanation.duplicate_findings_ignored = duplicates_ignored;
    explanation.suppressed_findings_count = suppressed_diagnostics.len();
    let score = explanation.final_score;
    let grade = score_grade(score);

    let correlator = CorrelationEngine::new();
    let root_causes = correlator.correlate(diagnostics.clone());
    let reporter = Reporter::with_root(&project_root);

    match &cli.command {
        None | Some(Commands::Doctor { .. }) => {
            let (cli_min_score, explain_mode, cli_baseline) = match &cli.command {
                Some(Commands::Doctor {
                    min_score,
                    explain,
                    baseline,
                    ..
                }) => (*min_score, *explain, baseline.clone()),
                _ => (None, false, None),
            };

            let min_score = config.resolve_min_score(cli_min_score);
            let baseline_path = config.resolve_baseline(cli_baseline);

            let mut baseline_diff = None;
            if let Some(b_path) = &baseline_path {
                let baseline_file = if b_path.is_absolute() {
                    b_path.clone()
                } else {
                    root.join(b_path)
                };
                if !baseline_file.is_file() {
                    eprintln!(
                        "{}",
                        format!("Baseline file not found: '{}'", baseline_file.display())
                            .red()
                            .bold()
                    );
                    ExitCode::ExecutionError.exit();
                }
                let baseline_data = match BaselineData::load_from_file(&baseline_file) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!(
                            "{}",
                            format!(
                                "Failed to load baseline file '{}': {e}",
                                baseline_file.display()
                            )
                            .red()
                            .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
                };
                baseline_diff = Some(baseline_data.diff_diagnostics(&diagnostics));
            }

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
                OutputFormat::Sarif => {
                    reporter.render_sarif_with_root(&diagnostics, Some(&ctx.root))?
                }
                OutputFormat::Html => reporter.render_html(
                    score,
                    grade,
                    &root_causes,
                    &diagnostics,
                    Some(&explanation),
                ),
            };
            println!("{}", output);

            if explain_mode && cli.format == OutputFormat::Text {
                print_score_explanation_details(&explanation);
            }

            if !suppressed_diagnostics.is_empty() && cli.format == OutputFormat::Text {
                println!(
                    "{}",
                    format!(
                        "  ({} finding(s) suppressed by configuration or inline comments)",
                        suppressed_diagnostics.len()
                    )
                    .yellow()
                );
            }

            if !suppression_outcome.expired_configured.is_empty()
                && cli.format == OutputFormat::Text
            {
                eprintln!(
                    "{}",
                    format!(
                        "⚠️ Warning: {} suppression entry(ies) in configuration have expired and are no longer active.",
                        suppression_outcome.expired_configured.len()
                    )
                    .yellow()
                    .bold()
                );
            }

            if let Some(diff) = &baseline_diff {
                if cli.format == OutputFormat::Text {
                    println!(
                        "{}",
                        format!(
                            "Baseline Status: {} legacy issue(s) tolerated, {} new issue(s), {} resolved.",
                            diff.tolerated_issues.len(),
                            diff.new_issues.len(),
                            diff.resolved_fingerprints.len()
                        ).cyan()
                    );
                }
            }

            if let Some(threshold) = min_score {
                if score < threshold {
                    eprintln!(
                        "{}",
                        format!(
                            "FAILED: Health score {} is below minimum threshold {}.",
                            score, threshold
                        )
                        .red()
                        .bold()
                    );
                    ExitCode::FindingsViolation.exit();
                }
            }

            if (cli.fail_on_incomplete || config.scanner.fail_on_incomplete)
                && diagnostics.iter().any(|d| d.rule_id.0.starts_with("DISC-"))
            {
                ExitCode::IncompleteAnalysis.exit();
            }

            ExitCode::Success.exit();
        }
        Some(Commands::Check {
            strict,
            fail_on,
            baseline,
            update_baseline,
            fail_on_incomplete,
            ..
        }) => {
            let should_fail_incomplete =
                *fail_on_incomplete || cli.fail_on_incomplete || config.scanner.fail_on_incomplete;
            let has_incomplete = diagnostics.iter().any(|d| d.rule_id.0.starts_with("DISC-"));
            let cli_fail_on = match fail_on.as_deref().map(parse_severity).transpose() {
                Ok(sev) => sev,
                Err(e) => {
                    eprintln!("{}", format!("Invalid --fail-on: {e}").red().bold());
                    ExitCode::ConfigurationError.exit();
                }
            };
            let threshold = config.resolve_fail_on(cli_fail_on, *strict, Severity::Error);
            let baseline_path = config.resolve_baseline(baseline.clone());

            if let Some(b_path) = &baseline_path {
                let baseline_file = if b_path.is_absolute() {
                    b_path.clone()
                } else {
                    root.join(b_path)
                };
                if !baseline_file.is_file() {
                    eprintln!(
                        "{}",
                        format!(
                            "Baseline file not found: '{}'. Run 'stackvet baseline record' first.",
                            baseline_file.display()
                        )
                        .red()
                        .bold()
                    );
                    ExitCode::ExecutionError.exit();
                }

                let mut baseline_data = match BaselineData::load_from_file(&baseline_file) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!(
                            "{}",
                            format!(
                                "Failed to load baseline file '{}': {e}",
                                baseline_file.display()
                            )
                            .red()
                            .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
                };
                let diff = baseline_data.diff_diagnostics(&diagnostics);
                let violating_issues = diff.new_issues_above_threshold(threshold);

                if *update_baseline {
                    baseline_data.update_from_diagnostics(&diagnostics);
                    if let Err(e) = baseline_data.save_to_file(&baseline_file) {
                        eprintln!(
                            "{}",
                            format!(
                                "Failed to save baseline file '{}': {e}",
                                baseline_file.display()
                            )
                            .red()
                            .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
                    eprintln!(
                        "{}",
                        format!(
                            "Baseline updated with current findings at '{}'.",
                            baseline_file.display()
                        )
                        .green()
                        .bold()
                    );
                }

                if cli.format == OutputFormat::Text {
                    if violating_issues.is_empty() {
                        if should_fail_incomplete && has_incomplete {
                            eprintln!(
                                "{}",
                                "✗ Baseline CI Check Incomplete: Malformed files, syntax errors, or scanner limit exhaustion detected."
                                    .red()
                                    .bold()
                            );
                            ExitCode::IncompleteAnalysis.exit();
                        }
                        println!(
                            "{}",
                            format!("✓ Baseline CI Check Passed: No new regressions at or above {} threshold.", threshold)
                                .green()
                                .bold()
                        );
                        if !diff.tolerated_issues.is_empty() {
                            println!(
                                "  ({} legacy finding(s) tolerated by baseline)",
                                diff.tolerated_issues.len()
                            );
                        }
                        if !diff.resolved_fingerprints.is_empty() {
                            println!(
                                "{}",
                                format!(
                                    "✓ Resolved: {} legacy baseline finding(s) fixed!",
                                    diff.resolved_fingerprints.len()
                                )
                                .green()
                            );
                        }
                        if !suppressed_diagnostics.is_empty() {
                            println!(
                                "  ({} finding(s) suppressed by configuration or inline comments)",
                                suppressed_diagnostics.len()
                            );
                        }
                        if !suppression_outcome.expired_configured.is_empty() {
                            eprintln!(
                                "{}",
                                format!(
                                    "⚠️ Warning: {} suppression entry(ies) in configuration have expired and are no longer active.",
                                    suppression_outcome.expired_configured.len()
                                )
                                .yellow()
                                .bold()
                            );
                        }
                        ExitCode::Success.exit();
                    } else {
                        eprintln!(
                            "{}",
                            format!("✗ Baseline CI Check Failed: {} new regression finding(s) at or above {} threshold.", violating_issues.len(), threshold)
                                .red()
                                .bold()
                        );
                        for d in &violating_issues {
                            eprintln!(
                                "  [{}] {}: {}",
                                d.severity.as_str().red(),
                                d.rule_id.as_str().cyan(),
                                d.message
                            );
                            if let Some(span) = &d.location.span {
                                eprintln!(
                                    "     at {}:{}",
                                    d.location.file_path.display(),
                                    span.start_line
                                );
                            }
                        }
                        if !diff.tolerated_issues.is_empty() {
                            eprintln!(
                                "  ({} legacy finding(s) tolerated by baseline)",
                                diff.tolerated_issues.len()
                            );
                        }
                        if !diff.resolved_fingerprints.is_empty() {
                            eprintln!(
                                "{}",
                                format!(
                                    "✓ Resolved: {} legacy baseline finding(s) fixed!",
                                    diff.resolved_fingerprints.len()
                                )
                                .green()
                            );
                        }
                        if !suppressed_diagnostics.is_empty() {
                            eprintln!(
                                "  ({} finding(s) suppressed by configuration or inline comments)",
                                suppressed_diagnostics.len()
                            );
                        }
                        if !suppression_outcome.expired_configured.is_empty() {
                            eprintln!(
                                "{}",
                                format!(
                                    "⚠️ Warning: {} suppression entry(ies) in configuration have expired and are no longer active.",
                                    suppression_outcome.expired_configured.len()
                                )
                                .yellow()
                                .bold()
                            );
                        }
                        ExitCode::FindingsViolation.exit();
                    }
                } else {
                    let output = match cli.format {
                        OutputFormat::Json => reporter.render_json(
                            score,
                            grade,
                            &root_causes,
                            &diagnostics,
                            Some(&explanation),
                        )?,
                        OutputFormat::Sarif => {
                            reporter.render_sarif_with_root(&diagnostics, Some(&ctx.root))?
                        }
                        OutputFormat::Html => reporter.render_html(
                            score,
                            grade,
                            &root_causes,
                            &diagnostics,
                            Some(&explanation),
                        ),
                        OutputFormat::Text => unreachable!(),
                    };
                    println!("{}", output);

                    if !violating_issues.is_empty() {
                        ExitCode::FindingsViolation.exit();
                    } else if should_fail_incomplete && has_incomplete {
                        ExitCode::IncompleteAnalysis.exit();
                    } else {
                        ExitCode::Success.exit();
                    }
                }
            } else {
                let violating_issues: Vec<&stackvet_core::Diagnostic> = diagnostics
                    .iter()
                    .filter(|d| d.severity >= threshold)
                    .collect();

                if cli.format == OutputFormat::Text {
                    if violating_issues.is_empty() {
                        if should_fail_incomplete && has_incomplete {
                            eprintln!(
                                "{}",
                                "✗ CI Check Incomplete: Malformed files, syntax errors, or scanner limit exhaustion detected."
                                    .red()
                                    .bold()
                            );
                            ExitCode::IncompleteAnalysis.exit();
                        }
                        println!(
                            "{}",
                            format!(
                                "✓ CI Check Passed: No findings at or above {} threshold.",
                                threshold
                            )
                            .green()
                            .bold()
                        );
                        if !suppressed_diagnostics.is_empty() {
                            println!(
                                "  ({} finding(s) suppressed by configuration or inline comments)",
                                suppressed_diagnostics.len()
                            );
                        }
                        if !suppression_outcome.expired_configured.is_empty() {
                            eprintln!(
                                "{}",
                                format!(
                                    "⚠️ Warning: {} suppression entry(ies) in configuration have expired and are no longer active.",
                                    suppression_outcome.expired_configured.len()
                                )
                                .yellow()
                                .bold()
                            );
                        }
                        ExitCode::Success.exit();
                    } else {
                        eprintln!(
                            "{}",
                            format!(
                                "✗ CI Check Failed: {} finding(s) at or above {} threshold.",
                                violating_issues.len(),
                                threshold
                            )
                            .red()
                            .bold()
                        );
                        for d in &violating_issues {
                            eprintln!(
                                "  [{}] {}: {}",
                                d.severity.as_str().red(),
                                d.rule_id.as_str().cyan(),
                                d.message
                            );
                            if let Some(span) = &d.location.span {
                                eprintln!(
                                    "     at {}:{}",
                                    d.location.file_path.display(),
                                    span.start_line
                                );
                            }
                        }
                        if !suppressed_diagnostics.is_empty() {
                            eprintln!(
                                "  ({} finding(s) suppressed by configuration or inline comments)",
                                suppressed_diagnostics.len()
                            );
                        }
                        if !suppression_outcome.expired_configured.is_empty() {
                            eprintln!(
                                "{}",
                                format!(
                                    "⚠️ Warning: {} suppression entry(ies) in configuration have expired and are no longer active.",
                                    suppression_outcome.expired_configured.len()
                                )
                                .yellow()
                                .bold()
                            );
                        }
                        ExitCode::FindingsViolation.exit();
                    }
                } else {
                    let output = match cli.format {
                        OutputFormat::Json => reporter.render_json(
                            score,
                            grade,
                            &root_causes,
                            &diagnostics,
                            Some(&explanation),
                        )?,
                        OutputFormat::Sarif => {
                            reporter.render_sarif_with_root(&diagnostics, Some(&ctx.root))?
                        }
                        OutputFormat::Html => reporter.render_html(
                            score,
                            grade,
                            &root_causes,
                            &diagnostics,
                            Some(&explanation),
                        ),
                        OutputFormat::Text => unreachable!(),
                    };
                    println!("{}", output);

                    if !violating_issues.is_empty() {
                        ExitCode::FindingsViolation.exit();
                    } else if should_fail_incomplete && has_incomplete {
                        ExitCode::IncompleteAnalysis.exit();
                    } else {
                        ExitCode::Success.exit();
                    }
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

            println!(
                "\n{}",
                "─── Detailed Deep Inspection ──────────────────────────────────────────"
                    .cyan()
                    .bold()
            );
            for d in &filtered {
                let sev_badge = match d.severity {
                    Severity::Critical => "[CRITICAL]".red().bold(),
                    Severity::Error => "[ERROR]".red(),
                    Severity::Warning => "[WARN]".yellow(),
                    Severity::Info => "[INFO]".cyan(),
                    Severity::Hint => "[HINT]".dimmed(),
                };
                println!(
                    "\n{} {} ({})",
                    sev_badge,
                    d.rule_id.0.cyan().bold(),
                    d.language.as_str().dimmed()
                );
                println!("  Location:    {}", d.location.display_location().bold());
                println!("  Message:     {}", d.message);
                if !d.explanation.is_empty() {
                    println!("  Explanation: {}", d.explanation);
                }
                println!("  Remediation: {}", d.suggested_remediation.green());
                if let Some(snippet) = &d.location.snippet {
                    println!("  Snippet:\n    {}", snippet.dimmed());
                }
                if let Some(fix) = &d.fix {
                    println!("  Safe Fix:    {}", fix.description.yellow());
                }
            }
            println!();
        }
        Some(Commands::Fix {
            yes,
            dry_run,
            rollback,
            verify,
            ..
        }) => {
            let engine = RemediationEngine::new();

            if *rollback {
                if !engine.has_rollback_session(&root) {
                    println!(
                        "{}",
                        "No active rollback session found in target directory.".yellow()
                    );
                    return Ok(());
                }
                let restored = engine.rollback_session(&root)?;
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

            let report = engine.apply_fixes_transactional_with_diagnostics(
                &fixable,
                Some(&root),
                Some(&diagnostics),
            )?;
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

            if *verify {
                println!("\n{}", "--- Running Fix Verification ---".cyan().bold());
                let post_diagnostics =
                    scan_and_collect_diagnostics(&root, &cli.ignore, cli.offline, &config)?;
                let result = stackvet_remediation::FixVerificationResult::verify(
                    &diagnostics,
                    &post_diagnostics,
                    &fixable,
                );
                print_verification_summary(&result);
                if !result.is_clean {
                    println!(
                        "{}",
                        "To revert these changes, run 'stackvet fix --rollback'.".cyan()
                    );
                    ExitCode::FindingsViolation.exit();
                }
            } else {
                println!(
                    "{}",
                    "To revert these changes, run 'stackvet fix --rollback'.".cyan()
                );
            }
        }
        Some(Commands::Verify { .. }) => {
            let engine = RemediationEngine::new();
            if !engine.has_rollback_session(&root) {
                println!(
                    "{}",
                    "No active rollback session found. Running full project diagnostic verification..."
                        .yellow()
                );
                let current_diagnostics =
                    scan_and_collect_diagnostics(&root, &cli.ignore, cli.offline, &config)?;
                if current_diagnostics.is_empty() {
                    println!(
                        "{}",
                        "Verification passed: 0 diagnostic findings.".green().bold()
                    );
                    return Ok(());
                } else {
                    println!(
                        "{}",
                        format!(
                            "Verification complete: {} diagnostic finding(s) present.",
                            current_diagnostics.len()
                        )
                        .yellow()
                        .bold()
                    );
                    ExitCode::FindingsViolation.exit();
                }
            }

            let snapshot = engine.read_rollback_snapshot(&root)?;
            println!(
                "{}",
                format!(
                    "Verifying {} fix(es) from session recorded at timestamp {}...",
                    snapshot.descriptions.len(),
                    snapshot.timestamp
                )
                .cyan()
                .bold()
            );

            let current_diagnostics =
                scan_and_collect_diagnostics(&root, &cli.ignore, cli.offline, &config)?;

            if snapshot.pre_diagnostics.is_empty() {
                println!(
                    "{}",
                    "Rollback snapshot does not contain a pre-fix diagnostic baseline. Verifying current project status across modified files..."
                        .yellow()
                );
                let modified_paths: std::collections::HashSet<PathBuf> =
                    snapshot.backups.iter().map(|b| b.path.clone()).collect();
                let remaining_in_modified: Vec<_> = current_diagnostics
                    .iter()
                    .filter(|d| modified_paths.contains(&d.location.file_path))
                    .collect();

                if remaining_in_modified.is_empty() {
                    println!(
                        "{}",
                        format!(
                            "Verification SUCCESS: 0 unresolved issues across {} modified file(s).",
                            modified_paths.len()
                        )
                        .green()
                        .bold()
                    );
                    return Ok(());
                } else {
                    eprintln!(
                        "{}",
                        format!(
                            "Verification WARNING: {} unresolved issue(s) remaining in modified files.",
                            remaining_in_modified.len()
                        )
                        .red()
                        .bold()
                    );
                    ExitCode::FindingsViolation.exit();
                }
            }

            let fixes_to_verify: Vec<stackvet_core::FixAction> = if !snapshot.fixes.is_empty() {
                snapshot.fixes.clone()
            } else {
                snapshot
                    .backups
                    .iter()
                    .zip(snapshot.descriptions.iter())
                    .map(|(b, desc)| {
                        stackvet_core::FixAction::new(desc.clone(), b.path.clone(), String::new())
                    })
                    .collect()
            };

            let result = stackvet_remediation::FixVerificationResult::verify(
                &snapshot.pre_diagnostics,
                &current_diagnostics,
                &fixes_to_verify,
            );
            print_verification_summary(&result);
            if !result.is_clean {
                ExitCode::FindingsViolation.exit();
            }
            return Ok(());
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
                    if let Err(e) = baseline_data.save_to_file(&baseline_file) {
                        eprintln!(
                            "{}",
                            format!(
                                "Failed to save baseline file '{}': {e}",
                                baseline_file.display()
                            )
                            .red()
                            .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
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
                    ExitCode::Success.exit();
                }
                BaselineAction::Verify {
                    baseline,
                    strict,
                    fail_on,
                    update,
                    ..
                } => {
                    let resolved_baseline = config
                        .resolve_baseline(baseline.clone())
                        .unwrap_or_else(|| PathBuf::from(".stackvet-baseline.json"));
                    let baseline_file =
                        if resolved_baseline.is_absolute() || resolved_baseline.is_file() {
                            resolved_baseline
                        } else {
                            root.join(resolved_baseline)
                        };
                    if !baseline_file.is_file() {
                        eprintln!(
                            "{}",
                            format!("Baseline file not found: '{}'. Run 'stackvet baseline record' first.", baseline_file.display())
                                .red()
                                .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }

                    let cli_fail_on = match fail_on.as_deref().map(parse_severity).transpose() {
                        Ok(sev) => sev,
                        Err(e) => {
                            eprintln!("{}", format!("Invalid --fail-on: {e}").red().bold());
                            ExitCode::ConfigurationError.exit();
                        }
                    };
                    let threshold = config.resolve_fail_on(cli_fail_on, *strict, Severity::Error);

                    let mut baseline_data = match BaselineData::load_from_file(&baseline_file) {
                        Ok(b) => b,
                        Err(e) => {
                            eprintln!(
                                "{}",
                                format!(
                                    "Failed to load baseline file '{}': {e}",
                                    baseline_file.display()
                                )
                                .red()
                                .bold()
                            );
                            ExitCode::ExecutionError.exit();
                        }
                    };
                    let diff = baseline_data.diff_diagnostics(&diagnostics);
                    let violating_issues = diff.new_issues_above_threshold(threshold);

                    if *update {
                        baseline_data.update_from_diagnostics(&diagnostics);
                        if let Err(e) = baseline_data.save_to_file(&baseline_file) {
                            eprintln!(
                                "{}",
                                format!(
                                    "Failed to save baseline file '{}': {e}",
                                    baseline_file.display()
                                )
                                .red()
                                .bold()
                            );
                            ExitCode::ExecutionError.exit();
                        }
                        println!(
                            "{}",
                            format!(
                                "Baseline updated with current findings at '{}'.",
                                baseline_file.display()
                            )
                            .green()
                            .bold()
                        );
                    }

                    if violating_issues.is_empty() {
                        println!(
                            "{}",
                            format!("✓ Baseline Verified: No new regressions detected above {} threshold.", threshold)
                                .green()
                                .bold()
                        );
                        if !diff.tolerated_issues.is_empty() {
                            println!(
                                "  ({} legacy finding(s) tolerated)",
                                diff.tolerated_issues.len()
                            );
                        }
                        if !diff.resolved_fingerprints.is_empty() {
                            println!(
                                "{}",
                                format!(
                                    "✓ Resolved: {} legacy baseline finding(s) fixed!",
                                    diff.resolved_fingerprints.len()
                                )
                                .green()
                            );
                        }
                        ExitCode::Success.exit();
                    } else {
                        eprintln!(
                            "{}",
                            format!("✗ Baseline Regression Detected: {} new finding(s) at or above {} threshold.", violating_issues.len(), threshold)
                                .red()
                                .bold()
                        );
                        for d in &violating_issues {
                            eprintln!(
                                "  [{}] {}: {}",
                                d.severity.as_str().red(),
                                d.rule_id.as_str().cyan(),
                                d.message
                            );
                            if let Some(span) = &d.location.span {
                                eprintln!(
                                    "     at {}:{}",
                                    d.location.file_path.display(),
                                    span.start_line
                                );
                            }
                        }
                        if !diff.tolerated_issues.is_empty() {
                            eprintln!(
                                "  ({} legacy finding(s) tolerated)",
                                diff.tolerated_issues.len()
                            );
                        }
                        if !diff.resolved_fingerprints.is_empty() {
                            eprintln!(
                                "{}",
                                format!(
                                    "✓ Resolved: {} legacy baseline finding(s) fixed!",
                                    diff.resolved_fingerprints.len()
                                )
                                .green()
                            );
                        }
                        ExitCode::FindingsViolation.exit();
                    }
                }
            }
        }
        Some(Commands::Pr {
            base,
            head,
            files,
            diff,
            strict,
            ci,
            github_annotations,
            comment,
            json,
            sarif,
            baseline,
            output,
            ..
        }) => {
            let changed_files = if let Some(diff_path) = diff {
                let p = if diff_path.is_absolute() {
                    diff_path.clone()
                } else {
                    root.join(diff_path)
                };
                match parse_patch_file(&p, &root) {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!(
                            "{}",
                            format!("Failed to parse diff file '{}': {e}", p.display())
                                .red()
                                .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
                }
            } else if let Some(files_str) = files {
                parse_explicit_files(files_str, &root)
            } else {
                match collect_git_diff(&root, base.as_deref(), Some(head)) {
                    Ok(summary) => summary.changed_files,
                    Err(e) => {
                        eprintln!("{}", format!("Git diff detection failed: {e}").red().bold());
                        ExitCode::ExecutionError.exit();
                    }
                }
            };

            let base_diagnostics: Vec<stackvet_core::Diagnostic> = if let Some(b_file) = baseline {
                let baseline_path = if b_file.is_absolute() {
                    b_file.clone()
                } else {
                    root.join(b_file)
                };
                if !baseline_path.is_file() {
                    eprintln!(
                        "{}",
                        format!("Baseline file not found: '{}'", baseline_path.display())
                            .red()
                            .bold()
                    );
                    ExitCode::ExecutionError.exit();
                }
                match BaselineData::load_from_file(&baseline_path) {
                    Ok(bdata) => bdata
                        .issues
                        .iter()
                        .map(|entry| {
                            stackvet_core::Diagnostic::builder(
                                &entry.rule_id,
                                stackvet_core::config::parse_severity(&entry.severity)
                                    .unwrap_or(Severity::Warning),
                                stackvet_core::LanguageTarget::General,
                            )
                            .message(&entry.message)
                            .location(
                                stackvet_core::DiagnosticLocation::new(&entry.file_path).with_span(
                                    stackvet_core::TextSpan::single_line(entry.line, 1, 1),
                                ),
                            )
                            .build()
                        })
                        .collect(),
                    Err(e) => {
                        eprintln!(
                            "{}",
                            format!(
                                "Failed to load baseline file '{}': {e}",
                                baseline_path.display()
                            )
                            .red()
                            .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
                }
            } else if root.join(".stackvet-baseline.json").is_file() {
                if let Ok(bdata) =
                    BaselineData::load_from_file(&root.join(".stackvet-baseline.json"))
                {
                    bdata
                        .issues
                        .iter()
                        .map(|entry| {
                            stackvet_core::Diagnostic::builder(
                                &entry.rule_id,
                                stackvet_core::config::parse_severity(&entry.severity)
                                    .unwrap_or(Severity::Warning),
                                stackvet_core::LanguageTarget::General,
                            )
                            .message(&entry.message)
                            .location(
                                stackvet_core::DiagnosticLocation::new(&entry.file_path).with_span(
                                    stackvet_core::TextSpan::single_line(entry.line, 1, 1),
                                ),
                            )
                            .build()
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            let pr_result =
                PrAnalysisResult::analyze(changed_files, &diagnostics, &base_diagnostics);

            if *github_annotations || *ci {
                for ann in pr_result.render_github_annotations() {
                    println!("{ann}");
                }
            }

            if *comment {
                let md = pr_result.render_markdown_comment();
                if let Some(out_file) = output {
                    if let Err(e) = fs::write(out_file, &md) {
                        eprintln!(
                            "{}",
                            format!("Failed to write report to '{}': {e}", out_file.display())
                                .red()
                                .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
                } else {
                    println!("{md}");
                }
            } else if *json {
                let json_str =
                    serde_json::to_string_pretty(&pr_result).unwrap_or_else(|_| "{}".to_string());
                if let Some(out_file) = output {
                    if let Err(e) = fs::write(out_file, &json_str) {
                        eprintln!(
                            "{}",
                            format!("Failed to write report to '{}': {e}", out_file.display())
                                .red()
                                .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
                } else {
                    println!("{json_str}");
                }
            } else if *sarif {
                let sarif_str = reporter
                    .render_sarif_with_root(&pr_result.new_diagnostics, Some(&root))
                    .unwrap_or_else(|_| "{}".to_string());
                if let Some(out_file) = output {
                    if let Err(e) = fs::write(out_file, &sarif_str) {
                        eprintln!(
                            "{}",
                            format!("Failed to write report to '{}': {e}", out_file.display())
                                .red()
                                .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
                } else {
                    println!("{sarif_str}");
                }
            } else {
                let text = pr_result.render_text();
                if let Some(out_file) = output {
                    if let Err(e) = fs::write(out_file, &text) {
                        eprintln!(
                            "{}",
                            format!("Failed to write report to '{}': {e}", out_file.display())
                                .red()
                                .bold()
                        );
                        ExitCode::ExecutionError.exit();
                    }
                } else {
                    println!("{text}");
                }
            }

            if pr_result.is_clean(*strict) {
                ExitCode::Success.exit();
            } else {
                ExitCode::FindingsViolation.exit();
            }
        }
        Some(Commands::BugReport { output, json, .. }) => {
            let report = bug_report::generate_bug_report(
                &project_root,
                &ctx,
                &config,
                &diagnostics,
                cli.config.as_deref(),
            )?;
            if *json {
                let json_str = serde_json::to_string_pretty(&report)?;
                if let Some(out_path) = output {
                    fs::write(out_path, &json_str)?;
                    println!("Bug report written to {}", out_path.display());
                } else {
                    println!("{}", json_str);
                }
            } else {
                bug_report::print_bug_report_summary(&report);
                if let Some(out_path) = output {
                    let json_str = serde_json::to_string_pretty(&report)?;
                    fs::write(out_path, &json_str)?;
                    println!("Detailed bug report JSON written to {}", out_path.display());
                }
            }
            ExitCode::Success.exit();
        }
        Some(Commands::Explain { .. })
        | Some(Commands::Rules { .. })
        | Some(Commands::Init { .. }) => {
            // Handled above
        }
    }

    Ok(())
}
