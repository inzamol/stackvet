pub mod docker;
pub mod env_drift;
pub mod imports_arch;
pub mod node_deps;
pub mod python_deps;
pub mod security;
pub mod vuln_cache;

pub use docker::DockerAnalyzer;
pub use env_drift::EnvDriftAnalyzer;
pub use imports_arch::ImportsArchAnalyzer;
pub use node_deps::NodeDependencyAnalyzer;
pub use python_deps::PythonDependencyAnalyzer;
pub use security::SecurityAnalyzer;
pub use vuln_cache::{
    CachedVulnerability, VulnCacheEntry, VulnCacheManager, VulnCacheStatus, VulnDatabaseCache,
};

use anyhow::Result;
use stackvet_core::{Analyzer, Diagnostic, ProjectContext, VulnerabilityCheckSummary};

pub fn run_all_analyzers(ctx: &ProjectContext, offline: bool) -> Result<Vec<Diagnostic>> {
    run_all_analyzers_with_options(ctx, offline, false)
}

pub fn run_all_analyzers_with_options(
    ctx: &ProjectContext,
    offline: bool,
    refresh_vuln_cache: bool,
) -> Result<Vec<Diagnostic>> {
    let (diags, _) = run_all_analyzers_with_options_and_status(ctx, offline, refresh_vuln_cache)?;
    Ok(diags)
}

pub fn run_all_analyzers_with_options_and_status(
    ctx: &ProjectContext,
    offline: bool,
    refresh_vuln_cache: bool,
) -> Result<(Vec<Diagnostic>, Option<VulnerabilityCheckSummary>)> {
    let mut diagnostics = ctx.discovery_diagnostics.clone();

    let non_security_analyzers: Vec<Box<dyn Analyzer>> = vec![
        Box::new(PythonDependencyAnalyzer::new(offline)),
        Box::new(NodeDependencyAnalyzer::new()),
        Box::new(ImportsArchAnalyzer::new()),
        Box::new(EnvDriftAnalyzer::new()),
        Box::new(DockerAnalyzer::new()),
    ];

    for analyzer in non_security_analyzers {
        match analyzer.analyze(ctx) {
            Ok(mut results) => diagnostics.append(&mut results),
            Err(err) => {
                tracing::error!("Analyzer '{}' failed: {err}", analyzer.name());
                eprintln!(
                    "Warning: Analyzer '{}' failed and was skipped: {err}",
                    analyzer.name()
                );
            }
        }
    }

    let sec_analyzer = SecurityAnalyzer::new(offline, refresh_vuln_cache);
    let (mut sec_diags, vuln_status) = match sec_analyzer.scan_with_status(ctx) {
        Ok(res) => res,
        Err(err) => {
            tracing::error!("Analyzer 'security' failed: {err}");
            eprintln!("Warning: Analyzer 'security' failed and was skipped: {err}");
            (Vec::new(), None)
        }
    };
    diagnostics.append(&mut sec_diags);

    Ok((diagnostics, vuln_status))
}
