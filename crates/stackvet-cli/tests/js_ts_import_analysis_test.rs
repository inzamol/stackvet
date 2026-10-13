use stackvet_analyzers::run_all_analyzers;
use stackvet_core::Diagnostic;
use stackvet_node::detect_node_project;
use stackvet_scanner::ProjectScanner;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("stackvet-node-test-{}-{}", name, ts));
        fs::create_dir_all(&path).expect("create test temp dir");
        TestDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn analyze_test_dir(dir: &Path) -> Vec<Diagnostic> {
    let scanner = ProjectScanner::new(dir);
    let mut ctx = scanner.build_context().expect("Failed to build context");
    ctx.node = detect_node_project(dir);
    run_all_analyzers(&ctx, true).expect("Failed to run core analyzers")
}

#[test]
fn test_dynamic_imports_and_commonjs_recognized() {
    let test_dir = TestDir::new("dyn-cjs");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "dyn-cjs-app",
            "dependencies": {
                "lodash": "^4.17.21",
                "express": "^4.19.2",
                "path-browserify": "^1.0.1"
            }
        }"#,
    )
    .unwrap();

    // File using dynamic import, CommonJS require, and require.resolve
    fs::write(
        root.join("index.ts"),
        r#"
const express = require('express');

async function bootstrap() {
    const _ = await import('lodash');
    const p = require.resolve('path-browserify');
}
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep002_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "NDEP-002").collect();

    assert!(
        ndep002_diags.is_empty(),
        "Dynamic import and CommonJS require dependencies should not be flagged as unused (NDEP-002): {:?}",
        ndep002_diags
    );
}

#[test]
fn test_reexports_recognized_as_active_usage() {
    let test_dir = TestDir::new("reexports");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "reexport-app",
            "dependencies": {
                "ramda": "^0.30.0",
                "axios": "^1.6.0"
            }
        }"#,
    )
    .unwrap();

    fs::write(
        root.join("index.ts"),
        r#"
export * from 'ramda';
export { get } from 'axios';
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep002_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "NDEP-002").collect();

    assert!(
        ndep002_diags.is_empty(),
        "Re-exported packages must not be flagged as unused (NDEP-002): {:?}",
        ndep002_diags
    );
}

#[test]
fn test_typescript_path_aliases_resolution() {
    let test_dir = TestDir::new("ts-aliases");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "ts-aliases-app",
            "dependencies": {
                "react": "^18.2.0"
            }
        }"#,
    )
    .unwrap();

    fs::write(
        root.join("tsconfig.json"),
        r#"{
            // TSConfig with comments & paths
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@/*": ["src/*"],
                    "~components/*": ["src/components/*"],
                    "@config": ["src/config.ts"]
                }
            }
        }"#,
    )
    .unwrap();

    let src = root.join("src");
    let components = src.join("components");
    fs::create_dir_all(&components).unwrap();

    fs::write(
        components.join("Button.tsx"),
        "export const Button = () => null;\n",
    )
    .unwrap();
    fs::write(
        src.join("config.ts"),
        "export const API_URL = 'http://localhost';\n",
    )
    .unwrap();
    fs::write(
        src.join("utils.ts"),
        "export const formatDate = () => '';\n",
    )
    .unwrap();

    fs::write(
        src.join("App.tsx"),
        r#"
import { Button } from '@/components/Button';
import { Button as Btn2 } from '~components/Button';
import { API_URL } from '@config';
import { formatDate } from '@/utils';

export function App() {
    return null;
}
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep001_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "NDEP-001").collect();
    let imp002_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "IMP-002").collect();

    assert!(
        ndep001_diags.is_empty(),
        "Valid TypeScript path aliases must never be flagged as missing npm packages (NDEP-001): {:?}",
        ndep001_diags
    );
    assert!(
        imp002_diags.is_empty(),
        "Resolved TypeScript path aliases must not trigger IMP-002: {:?}",
        imp002_diags
    );
}

#[test]
fn test_unresolved_path_alias_triggers_diagnostic() {
    let test_dir = TestDir::new("unresolved-alias");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "unresolved-alias-app",
            "dependencies": {}
        }"#,
    )
    .unwrap();

    fs::write(
        root.join("tsconfig.json"),
        r#"{
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "@/*": ["src/*"]
                }
            }
        }"#,
    )
    .unwrap();

    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();

    // App imports @/non_existent_file which does not exist on disk
    fs::write(
        src.join("main.ts"),
        "import { Missing } from '@/non_existent_file';\n",
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let imp002_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "IMP-002").collect();

    assert!(
        !imp002_diags.is_empty(),
        "Unresolved TypeScript path alias must trigger IMP-002"
    );
    assert!(
        imp002_diags[0].message.contains("@/non_existent_file"),
        "IMP-002 message should identify the unresolved alias specifier: {}",
        imp002_diags[0].message
    );
}

#[test]
fn test_local_module_resolution_extensions_and_directory_index() {
    let test_dir = TestDir::new("local-resolution");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "local-res-app",
            "dependencies": {}
        }"#,
    )
    .unwrap();

    // Project structure:
    // root/
    //   src/
    //     index.ts
    //     helpers.js
    //     widget/
    //       index.tsx
    //     format.mjs
    let src = root.join("src");
    let widget = src.join("widget");
    fs::create_dir_all(&widget).unwrap();

    fs::write(src.join("helpers.js"), "export const help = () => 1;\n").unwrap();
    fs::write(
        widget.join("index.tsx"),
        "export const Widget = () => null;\n",
    )
    .unwrap();
    fs::write(src.join("format.mjs"), "export const format = () => '';\n").unwrap();

    fs::write(
        src.join("index.ts"),
        r#"
import { help } from './helpers';
import { Widget } from './widget';
import { format } from './format';
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let imp002_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "IMP-002").collect();

    assert!(
        imp002_diags.is_empty(),
        "Extensionless, directory index, and .mjs imports must resolve without IMP-002: {:?}",
        imp002_diags
    );
}

#[test]
fn test_distinguishes_genuine_missing_npm_package_from_valid_import() {
    let test_dir = TestDir::new("missing-vs-valid");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "missing-vs-valid-app",
            "dependencies": {
                "react": "^18.2.0"
            }
        }"#,
    )
    .unwrap();

    fs::write(root.join("localMod.ts"), "export const local = 42;\n").unwrap();

    // imports declared 'react', local './localMod', and undeclared 'framer-motion'
    fs::write(
        root.join("index.ts"),
        r#"
import React from 'react';
import { local } from './localMod';
import { motion } from 'framer-motion';
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep001_messages: Vec<String> = diags
        .iter()
        .filter(|d| d.rule_id.0 == "NDEP-001")
        .map(|d| d.message.clone())
        .collect();

    assert_eq!(
        ndep001_messages.len(),
        1,
        "Expected exactly 1 NDEP-001 diagnostic for undeclared 'framer-motion', got: {:?}",
        ndep001_messages
    );
    assert!(
        ndep001_messages[0].contains("framer-motion"),
        "NDEP-001 message must identify undeclared package: {}",
        ndep001_messages[0]
    );
}
