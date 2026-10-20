pub mod docker;
pub mod env_drift;
pub mod imports_arch;
pub mod node_deps;
pub mod python_deps;

pub use docker::DockerAnalyzer;
pub use env_drift::EnvDriftAnalyzer;
pub use imports_arch::ImportsArchAnalyzer;
pub use node_deps::NodeDependencyAnalyzer;
pub use python_deps::PythonDependencyAnalyzer;

use anyhow::Result;
use stackvet_core::{Analyzer, Diagnostic, ProjectContext};

pub fn run_all_analyzers(ctx: &ProjectContext, offline: bool) -> Result<Vec<Diagnostic>> {
    let mut diagnostics = ctx.discovery_diagnostics.clone();

    let analyzers: Vec<Box<dyn Analyzer>> = vec![
        Box::new(PythonDependencyAnalyzer::new(offline)),
        Box::new(NodeDependencyAnalyzer::new()),
        Box::new(ImportsArchAnalyzer::new()),
        Box::new(EnvDriftAnalyzer::new()),
        Box::new(DockerAnalyzer::new()),
    ];

    for analyzer in analyzers {
        let mut results = analyzer.analyze(ctx)?;
        diagnostics.append(&mut results);
    }

    Ok(diagnostics)
}
