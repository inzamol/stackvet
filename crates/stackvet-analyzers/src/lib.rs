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
use stackvet_core::{Analyzer, Diagnostic, ProjectContext};

pub fn run_all_analyzers(ctx: &ProjectContext, offline: bool) -> Result<Vec<Diagnostic>> {
    run_all_analyzers_with_options(ctx, offline, false)
}

pub fn run_all_analyzers_with_options(
    ctx: &ProjectContext,
    offline: bool,
    refresh_vuln_cache: bool,
) -> Result<Vec<Diagnostic>> {
    let mut diagnostics = ctx.discovery_diagnostics.clone();

    let analyzers: Vec<Box<dyn Analyzer>> = vec![
        Box::new(PythonDependencyAnalyzer::new(offline)),
        Box::new(NodeDependencyAnalyzer::new()),
        Box::new(ImportsArchAnalyzer::new()),
        Box::new(EnvDriftAnalyzer::new()),
        Box::new(DockerAnalyzer::new()),
        Box::new(SecurityAnalyzer::new(offline, refresh_vuln_cache)),
    ];

    for analyzer in analyzers {
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

    Ok(diagnostics)
}
