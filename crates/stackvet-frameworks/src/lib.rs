pub mod celery;
pub mod django;
pub mod fastapi;
pub mod react;
pub mod sqlalchemy;

pub use celery::CeleryAnalyzer;
pub use django::DjangoAnalyzer;
pub use fastapi::FastApiAnalyzer;
pub use react::ReactAnalyzer;
pub use sqlalchemy::SqlAlchemyAnalyzer;

use anyhow::Result;
use stackvet_core::{Analyzer, Diagnostic, ProjectContext};

pub fn run_all_framework_analyzers(ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();

    let analyzers: Vec<Box<dyn Analyzer>> = vec![
        Box::new(FastApiAnalyzer::new()),
        Box::new(SqlAlchemyAnalyzer::new()),
        Box::new(DjangoAnalyzer::new()),
        Box::new(CeleryAnalyzer::new()),
        Box::new(ReactAnalyzer::new()),
    ];

    for analyzer in analyzers {
        let mut results = analyzer.analyze(ctx)?;
        diagnostics.append(&mut results);
    }

    Ok(diagnostics)
}
