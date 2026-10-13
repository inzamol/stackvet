use stackvet_analyzers::run_all_analyzers;
use stackvet_core::Diagnostic;
use stackvet_python::detect_python_project;
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
        let path = std::env::temp_dir().join(format!("stackvet-py-test-{}-{}", name, ts));
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
    ctx.python = detect_python_project(dir);
    run_all_analyzers(&ctx, true).expect("Failed to run core analyzers")
}

#[test]
fn test_relative_imports_and_parent_traversal() {
    let test_dir = TestDir::new("rel-imports");
    let root = test_dir.path();

    // Setup project structure:
    // root/
    //   pyproject.toml
    //   parent_mod.py
    //   pkg/
    //     __init__.py
    //     core.py
    //     models.py
    //     helpers/
    //       __init__.py
    //       utils.py
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"rel-app\"\nversion = \"0.1.0\"\ndependencies = []\n",
    )
    .unwrap();

    fs::write(
        root.join("parent_mod.py"),
        "def parent_func():\n    return 42\n",
    )
    .unwrap();

    let pkg = root.join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join("__init__.py"), "# package init\n").unwrap();
    fs::write(pkg.join("models.py"), "class User:\n    pass\n").unwrap();

    let helpers = pkg.join("helpers");
    fs::create_dir_all(&helpers).unwrap();
    fs::write(helpers.join("__init__.py"), "").unwrap();
    fs::write(helpers.join("utils.py"), "def format_name():\n    pass\n").unwrap();

    // core.py uses level 1 (.models and .helpers.utils) and level 2 (..parent_mod)
    fs::write(
        pkg.join("core.py"),
        r#"
from .models import User
from .helpers.utils import format_name
from ..parent_mod import parent_func
from . import models

def run():
    u = User()
    format_name()
    parent_func()
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let imp002_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "IMP-002").collect();
    let dep002_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "DEP-002").collect();

    assert!(
        imp002_diags.is_empty(),
        "Expected 0 unresolved relative import diagnostics, found: {:?}",
        imp002_diags
    );
    assert!(
        dep002_diags.is_empty(),
        "Expected 0 missing dependency diagnostics for local relative imports, found: {:?}",
        dep002_diags
    );
}

#[test]
fn test_namespace_packages_pep420() {
    let test_dir = TestDir::new("pep420");
    let root = test_dir.path();

    // PEP 420 namespace package: directory structure WITHOUT __init__.py
    // root/
    //   pyproject.toml
    //   main.py
    //   ns/
    //     sub_ns/
    //       worker.py
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"ns-app\"\nversion = \"0.1.0\"\ndependencies = []\n",
    )
    .unwrap();

    let sub_ns = root.join("ns").join("sub_ns");
    fs::create_dir_all(&sub_ns).unwrap();
    fs::write(sub_ns.join("worker.py"), "def work():\n    return True\n").unwrap();

    fs::write(
        root.join("main.py"),
        "from ns.sub_ns.worker import work\n\ndef main():\n    work()\n",
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let dep002_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "DEP-002").collect();

    assert!(
        dep002_diags.is_empty(),
        "PEP 420 namespace package 'ns' should not be flagged as missing dependency: {:?}",
        dep002_diags
    );
}

#[test]
fn test_package_versus_module_names_resolution() {
    let test_dir = TestDir::new("pkg-vs-module");
    let root = test_dir.path();

    fs::write(
        root.join("pyproject.toml"),
        r#"
[project]
name = "mapping-app"
version = "0.1.0"
dependencies = [
    "pyyaml>=6.0",
    "pillow>=10.0.0",
    "psycopg2-binary>=2.9.0",
    "opencv-python-headless>=4.8.0",
    "djangorestframework>=3.14.0",
]
"#,
    )
    .unwrap();

    fs::write(
        root.join("main.py"),
        r#"
import yaml
from PIL import Image
import psycopg2
import cv2
import rest_framework
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let dep002_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "DEP-002").collect();
    let dep003_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "DEP-003").collect();

    assert!(
        dep002_diags.is_empty(),
        "Expected 0 missing dependency diagnostics for mapped imports: {:?}",
        dep002_diags
    );
    assert!(
        dep003_diags.is_empty(),
        "Expected 0 unused dependency diagnostics for mapped imports: {:?}",
        dep003_diags
    );
}

#[test]
fn test_dynamic_imports_detected() {
    let test_dir = TestDir::new("dyn-imports");
    let root = test_dir.path();

    fs::write(
        root.join("pyproject.toml"),
        r#"
[project]
name = "dynamic-app"
version = "0.1.0"
dependencies = [
    "redis>=5.0.0",
    "celery>=5.3.0",
]
"#,
    )
    .unwrap();

    fs::write(
        root.join("main.py"),
        r#"
import os
import importlib

client = __import__("redis")
app = importlib.import_module("celery")
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let dep003_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "DEP-003").collect();

    assert!(
        dep003_diags.is_empty(),
        "Dynamic imports should prevent DEP-003 unused dependency warnings: {:?}",
        dep003_diags
    );
}

#[test]
fn test_standard_library_and_future_imports() {
    let test_dir = TestDir::new("stdlib-test");
    let root = test_dir.path();

    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"stdlib-app\"\nversion = \"0.1.0\"\ndependencies = []\n",
    )
    .unwrap();

    fs::write(
        root.join("main.py"),
        r#"
from __future__ import annotations
import _thread
import zoneinfo
import tomllib
import sys
import os
import math
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let dep002_diags: Vec<&Diagnostic> =
        diags.iter().filter(|d| d.rule_id.0 == "DEP-002").collect();

    assert!(
        dep002_diags.is_empty(),
        "Standard library and __future__ imports must not trigger DEP-002: {:?}",
        dep002_diags
    );
}

#[test]
fn test_distinguish_missing_packages_from_valid_imports() {
    let test_dir = TestDir::new("distinguish-test");
    let root = test_dir.path();

    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"distinguish-app\"\nversion = \"0.1.0\"\ndependencies = []\n",
    )
    .unwrap();

    // Local module 'models.py'
    fs::write(root.join("models.py"), "class Product:\n    pass\n").unwrap();

    // main.py imports local module 'models' AND genuinely missing package 'missing_foobar_pkg'
    fs::write(
        root.join("main.py"),
        r#"
import models
import missing_foobar_pkg
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let dep002_messages: Vec<String> = diags
        .iter()
        .filter(|d| d.rule_id.0 == "DEP-002")
        .map(|d| d.message.clone())
        .collect();

    assert_eq!(
        dep002_messages.len(),
        1,
        "Expected exactly 1 DEP-002 for genuine missing package, found: {:?}",
        dep002_messages
    );
    assert!(
        dep002_messages[0].contains("missing-foobar-pkg")
            || dep002_messages[0].contains("missing_foobar_pkg"),
        "Diagnostic message should identify missing package: {}",
        dep002_messages[0]
    );
    assert!(
        !dep002_messages[0].contains("models"),
        "Local module 'models' must not be flagged as missing dependency!"
    );
}
