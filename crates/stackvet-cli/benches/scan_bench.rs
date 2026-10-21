use criterion::{black_box, criterion_group, criterion_main, Criterion};
use stackvet_core::{
    Confidence, Diagnostic, DiagnosticLocation, LanguageTarget, Severity, TextSpan,
};
use stackvet_correlation::{calculate_health_score_explained, CorrelationEngine};
use stackvet_scanner::ProjectScanner;
use std::fs;
use std::path::{Path, PathBuf};

struct UniqueBenchDir {
    path: PathBuf,
}

impl UniqueBenchDir {
    fn new(prefix: &str) -> Self {
        let unique_name = format!(
            "{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = std::env::temp_dir().join(unique_name);
        fs::create_dir_all(&path).expect("Failed to create unique benchmark directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for UniqueBenchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn create_synthetic_project(num_files: usize) -> UniqueBenchDir {
    let bench_dir = UniqueBenchDir::new(&format!("stackvet_bench_{}", num_files));
    let dir = bench_dir.path();
    fs::create_dir_all(dir.join("src").join("components")).unwrap();
    fs::create_dir_all(dir.join("tests")).unwrap();

    fs::write(
        dir.join("pyproject.toml"),
        "[project]\nname = \"bench-project\"\nversion = \"0.1.0\"\ndependencies = [\"requests\", \"fastapi\"]\n",
    )
    .unwrap();

    fs::write(
        dir.join("package.json"),
        "{\"name\": \"bench-web\", \"dependencies\": {\"react\": \"^18.2.0\"}}\n",
    )
    .unwrap();

    for i in 0..num_files {
        let py_code = format!(
            "import os\nimport sys\nimport requests\nfrom fastapi import FastAPI\n\napp = FastAPI()\n\n@app.get('/item/{}')\ndef get_item():\n    return {{'item': {}}}\n",
            i, i
        );
        fs::write(dir.join(format!("src/file_{}.py", i)), py_code).unwrap();

        let ts_code = format!(
            "import React, {{ useState, useEffect }} from 'react';\n\nexport const Component{} = () => {{\n  const [count, setCount] = useState(0);\n  useEffect(() => {{\n    console.log(count);\n  }}, []);\n  return <div>{{count}}</div>;\n}};\n",
            i
        );
        fs::write(dir.join(format!("src/components/Comp_{}.tsx", i)), ts_code).unwrap();
    }

    bench_dir
}

fn bench_project_scanner(c: &mut Criterion) {
    let project_dir = create_synthetic_project(50);

    c.bench_function("scanner_directory_traversal_50_files", |b| {
        b.iter(|| {
            let scanner = ProjectScanner::new(black_box(project_dir.path()));
            let res = scanner.scan();
            assert!(!res.source_files.is_empty());
        })
    });
}

fn bench_python_ast_parsing(c: &mut Criterion) {
    let sample_python = r#"
import os
import sys
from typing import List, Optional, Dict
import requests
from fastapi import FastAPI, Depends, HTTPException

app = FastAPI(title="Benchmark Application")

class ItemManager:
    def __init__(self, db_url: str):
        self.db_url = db_url

    async def fetch_all(self, limit: int = 100) -> List[Dict]:
        return [{"id": i, "name": f"Item {i}"} for i in range(limit)]

@app.get("/items")
async def list_items(limit: int = 50):
    manager = ItemManager("sqlite:///:memory:")
    return await manager.fetch_all(limit=limit)
"#;

    c.bench_function("python_ast_parse_ruff", |b| {
        b.iter(|| {
            let parsed = ruff_python_parser::parse(
                black_box(sample_python),
                ruff_python_parser::Mode::Module.into(),
            );
            assert!(parsed.is_ok());
        })
    });
}

fn bench_oxc_typescript_ast_parsing(c: &mut Criterion) {
    let sample_typescript = r#"
import React, { useState, useEffect, useCallback, useMemo } from 'react';

interface UserProps {
    id: string;
    username: string;
    roles: string[];
}

export const UserProfile: React.FC<UserProps> = ({ id, username, roles }) => {
    const [active, setActive] = useState(false);

    const formattedRoles = useMemo(() => {
        return roles.map(r => r.toUpperCase()).join(', ');
    }, [roles]);

    const handleToggle = useCallback(() => {
        setActive(prev => !prev);
    }, []);

    useEffect(() => {
        document.title = `Profile: ${username}`;
    }, [username]);

    return (
        <div className="profile-card">
            <h2>{username}</h2>
            <p>ID: {id}</p>
            <p>Roles: {formattedRoles}</p>
            <button onClick={handleToggle}>Toggle: {active ? "Active" : "Inactive"}</button>
        </div>
    );
};
"#;

    c.bench_function("typescript_ast_parse_oxc", |b| {
        b.iter(|| {
            let allocator = oxc_allocator::Allocator::default();
            let source_type = oxc_span::SourceType::default()
                .with_typescript(true)
                .with_jsx(true);
            let ret =
                oxc_parser::Parser::new(&allocator, black_box(sample_typescript), source_type)
                    .parse();
            assert!(ret.errors.is_empty());
        })
    });
}

fn bench_health_scoring_and_correlation(c: &mut Criterion) {
    let mut diags = Vec::new();
    for i in 0..100 {
        diags.push(
            Diagnostic::builder(
                if i % 3 == 0 {
                    "DEP-002"
                } else if i % 3 == 1 {
                    "IMP-001"
                } else {
                    "FAP-001"
                },
                if i % 5 == 0 {
                    Severity::Critical
                } else {
                    Severity::Warning
                },
                LanguageTarget::Python,
            )
            .message(format!("Diagnostic finding {}", i))
            .location(
                DiagnosticLocation::new(format!("src/file_{}.py", i % 20))
                    .with_span(TextSpan::single_line(i + 1, 1, 20)),
            )
            .confidence(Confidence::High)
            .build(),
        );
    }

    let correlator = CorrelationEngine::new();

    c.bench_function("health_scoring_and_correlation_100_diags", |b| {
        b.iter(|| {
            let exp = calculate_health_score_explained(black_box(&diags));
            assert!(exp.final_score <= 100);

            let root_causes = correlator.correlate(black_box(diags.clone()));
            assert!(!root_causes.is_empty());
        })
    });
}

criterion_group!(
    benches,
    bench_project_scanner,
    bench_python_ast_parsing,
    bench_oxc_typescript_ast_parsing,
    bench_health_scoring_and_correlation
);
criterion_main!(benches);
