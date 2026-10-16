use stackvet_analyzers::run_all_analyzers;
use stackvet_core::Diagnostic;
use stackvet_node::detect_node_project;
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
        let path = std::env::temp_dir().join(format!("stackvet-lock-test-{}-{}", name, ts));
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
    ctx.python = detect_python_project(dir);
    run_all_analyzers(&ctx, true).expect("Failed to run core analyzers")
}

#[test]
fn test_npm_missing_lockfile_triggers_ndep004() {
    let test_dir = TestDir::new("npm-missing");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "npm-missing-app",
            "dependencies": {
                "react": "^18.2.0"
            }
        }"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep004 = diags.iter().find(|d| d.rule_id.0 == "NDEP-004");
    assert!(ndep004.is_some(), "Expected NDEP-004 for missing lockfile");
    assert!(
        ndep004.unwrap().message.contains("No lockfile detected"),
        "Message should indicate no lockfile detected"
    );
}

#[test]
fn test_npm_synced_lockfile_produces_zero_ndep004() {
    let test_dir = TestDir::new("npm-synced");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "npm-synced-app",
            "dependencies": {
                "react": "^18.2.0",
                "axios": "^1.6.0"
            }
        }"#,
    )
    .unwrap();

    fs::write(
        root.join("package-lock.json"),
        r#"{
            "name": "npm-synced-app",
            "lockfileVersion": 3,
            "packages": {
                "": {
                    "dependencies": {
                        "react": "^18.2.0",
                        "axios": "^1.6.0"
                    }
                },
                "node_modules/react": {
                    "version": "18.2.0"
                },
                "node_modules/axios": {
                    "version": "1.6.2"
                }
            }
        }"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep004 = diags.iter().find(|d| d.rule_id.0 == "NDEP-004");
    assert!(
        ndep004.is_none(),
        "Expected no NDEP-004 for synced lockfile, found: {:?}",
        ndep004
    );
}

#[test]
fn test_npm_stale_lockfile_entry_triggers_ndep004() {
    let test_dir = TestDir::new("npm-stale");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "npm-stale-app",
            "dependencies": {
                "react": "^18.2.0",
                "axios": "^1.6.0"
            }
        }"#,
    )
    .unwrap();

    // Lockfile is missing axios
    fs::write(
        root.join("package-lock.json"),
        r#"{
            "name": "npm-stale-app",
            "lockfileVersion": 3,
            "packages": {
                "node_modules/react": {
                    "version": "18.2.0"
                }
            }
        }"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let stale_diag = diags
        .iter()
        .find(|d| d.rule_id.0 == "NDEP-004" && d.message.contains("missing from"));
    assert!(
        stale_diag.is_some(),
        "Expected NDEP-004 stale entry for axios missing from lockfile"
    );
    assert!(stale_diag.unwrap().message.contains("axios"));
}

#[test]
fn test_npm_constraint_drift_triggers_ndep004() {
    let test_dir = TestDir::new("npm-drift");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "npm-drift-app",
            "dependencies": {
                "react": "^18.2.0"
            }
        }"#,
    )
    .unwrap();

    // Lockfile still locks older react 17.0.2
    fs::write(
        root.join("package-lock.json"),
        r#"{
            "name": "npm-drift-app",
            "lockfileVersion": 3,
            "packages": {
                "node_modules/react": {
                    "version": "17.0.2"
                }
            }
        }"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let drift_diag = diags
        .iter()
        .find(|d| d.rule_id.0 == "NDEP-004" && d.message.contains("violates manifest constraint"));
    assert!(
        drift_diag.is_some(),
        "Expected NDEP-004 for constraint drift"
    );
    let msg = &drift_diag.unwrap().message;
    assert!(msg.contains("17.0.2"));
    assert!(msg.contains("^18.2.0"));
}

#[test]
fn test_pnpm_lockfile_sync_and_drift() {
    let test_dir = TestDir::new("pnpm-drift");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "pnpm-app",
            "dependencies": {
                "react": "^18.2.0"
            }
        }"#,
    )
    .unwrap();

    // Lockfile has react locked at 18.2.0 -> clean
    fs::write(
        root.join("pnpm-lock.yaml"),
        r#"
lockfileVersion: '6.0'
importers:
  .:
    dependencies:
      react:
        specifier: ^18.2.0
        version: 18.2.0
packages:
  /react@18.2.0:
    resolution: {}
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    assert!(!diags.iter().any(|d| d.rule_id.0 == "NDEP-004"));

    // Now update package.json to ^19.0.0 without updating lockfile
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "pnpm-app",
            "dependencies": {
                "react": "^19.0.0"
            }
        }"#,
    )
    .unwrap();

    let diags_drift = analyze_test_dir(root);
    let pnpm_drift = diags_drift
        .iter()
        .find(|d| d.rule_id.0 == "NDEP-004" && d.message.contains("violates manifest constraint"));
    assert!(pnpm_drift.is_some(), "Expected NDEP-004 pnpm drift");
}

#[test]
fn test_yarn_lockfile_sync_and_stale() {
    let test_dir = TestDir::new("yarn-test");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "yarn-app",
            "dependencies": {
                "react": "^18.2.0",
                "lodash": "^4.17.21"
            }
        }"#,
    )
    .unwrap();

    // Yarn lock only contains react, missing lodash
    fs::write(
        root.join("yarn.lock"),
        r#"
# yarn lockfile v1
"react@^18.2.0":
  version "18.2.0"
  resolved "https://registry.yarnpkg.com/react/-/react-18.2.0.tgz"
"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let stale_diag = diags
        .iter()
        .find(|d| d.rule_id.0 == "NDEP-004" && d.message.contains("lodash"));
    assert!(
        stale_diag.is_some(),
        "Expected NDEP-004 for lodash missing in yarn.lock"
    );
}

#[test]
fn test_workspace_dependencies_resolved_and_drift() {
    let test_dir = TestDir::new("workspace-test");
    let root = test_dir.path();

    // Monorepo root package.json
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "monorepo-root",
            "workspaces": ["packages/*"],
            "dependencies": {
                "@myorg/ui": "workspace:*",
                "@myorg/utils": "workspace:^1.0.0"
            }
        }"#,
    )
    .unwrap();

    // Package @myorg/ui version 2.0.0
    let ui_pkg_dir = root.join("packages").join("ui");
    fs::create_dir_all(&ui_pkg_dir).unwrap();
    fs::write(
        ui_pkg_dir.join("package.json"),
        r#"{
            "name": "@myorg/ui",
            "version": "2.0.0"
        }"#,
    )
    .unwrap();

    // Package @myorg/utils version 1.2.0
    let utils_pkg_dir = root.join("packages").join("utils");
    fs::create_dir_all(&utils_pkg_dir).unwrap();
    fs::write(
        utils_pkg_dir.join("package.json"),
        r#"{
            "name": "@myorg/utils",
            "version": "1.2.0"
        }"#,
    )
    .unwrap();

    // Valid npm lockfile with workspace references
    fs::write(
        root.join("package-lock.json"),
        r#"{
            "name": "monorepo-root",
            "lockfileVersion": 3,
            "packages": {
                "": {
                    "dependencies": {
                        "@myorg/ui": "workspace:*",
                        "@myorg/utils": "workspace:^1.0.0"
                    }
                },
                "packages/ui": {
                    "name": "@myorg/ui",
                    "version": "2.0.0"
                },
                "packages/utils": {
                    "name": "@myorg/utils",
                    "version": "1.2.0"
                },
                "node_modules/@myorg/ui": {
                    "resolved": "packages/ui",
                    "link": true
                },
                "node_modules/@myorg/utils": {
                    "resolved": "packages/utils",
                    "link": true
                }
            }
        }"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    assert!(
        !diags.iter().any(|d| d.rule_id.0 == "NDEP-004"),
        "Expected zero NDEP-004 for valid workspace dependencies, got: {:?}",
        diags
    );

    // Now introduce workspace version mismatch: constraint is workspace:^2.0.0 for @myorg/utils (which is 1.2.0)
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "monorepo-root",
            "workspaces": ["packages/*"],
            "dependencies": {
                "@myorg/ui": "workspace:*",
                "@myorg/utils": "workspace:^2.0.0"
            }
        }"#,
    )
    .unwrap();

    let diags_ws_drift = analyze_test_dir(root);
    let ws_drift = diags_ws_drift
        .iter()
        .find(|d| d.rule_id.0 == "NDEP-004" && d.message.contains("@myorg/utils"));
    assert!(
        ws_drift.is_some(),
        "Expected NDEP-004 for workspace version mismatch"
    );
}

#[test]
fn test_inconsistent_dependency_versions_ndep006() {
    let test_dir = TestDir::new("inconsistent-vers");
    let root = test_dir.path();

    fs::write(
        root.join("package.json"),
        r#"{
            "name": "multi-ver-app",
            "dependencies": {
                "react": "^18.2.0"
            }
        }"#,
    )
    .unwrap();

    // Lockfile with multiple conflicting major versions of react
    fs::write(
        root.join("package-lock.json"),
        r#"{
            "name": "multi-ver-app",
            "lockfileVersion": 3,
            "packages": {
                "node_modules/react": {
                    "version": "18.2.0"
                },
                "node_modules/legacy-lib/node_modules/react": {
                    "version": "17.0.2"
                }
            }
        }"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep006 = diags.iter().find(|d| d.rule_id.0 == "NDEP-006");
    assert!(
        ndep006.is_some(),
        "Expected NDEP-006 for conflicting react versions"
    );
    assert!(ndep006.unwrap().message.contains("17.0.2"));
    assert!(ndep006.unwrap().message.contains("18.2.0"));
}

#[test]
fn test_python_uv_lock_missing_stale_and_drift() {
    let test_dir = TestDir::new("py-uv-drift");
    let root = test_dir.path();

    // 1. Missing uv.lock
    fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "py-app"
version = "0.1.0"
dependencies = [
    "requests>=2.28.0",
    "pydantic>=2.0.0",
]
"#,
    )
    .unwrap();

    let diags_missing = analyze_test_dir(root);
    let missing_lock = diags_missing
        .iter()
        .find(|d| d.rule_id.0 == "DEP-005" && d.message.contains("Missing lockfile"));
    assert!(
        missing_lock.is_some(),
        "Expected DEP-005 for missing lockfile"
    );

    // 2. Synced uv.lock -> 0 DEP-005
    fs::write(
        root.join("uv.lock"),
        r#"version = 1
revision = 1

[[package]]
name = "requests"
version = "2.28.2"

[[package]]
name = "pydantic"
version = "2.6.4"
"#,
    )
    .unwrap();

    let diags_synced = analyze_test_dir(root);
    assert!(
        !diags_synced.iter().any(|d| d.rule_id.0 == "DEP-005"),
        "Expected 0 DEP-005 for synced uv.lock"
    );

    // 3. Stale uv.lock: manifest adds fastapi but uv.lock doesn't have it
    fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "py-app"
version = "0.1.0"
dependencies = [
    "requests>=2.28.0",
    "pydantic>=2.0.0",
    "fastapi>=0.110.0",
]
"#,
    )
    .unwrap();

    let diags_stale = analyze_test_dir(root);
    let stale_diag = diags_stale
        .iter()
        .find(|d| d.rule_id.0 == "DEP-005" && d.message.contains("fastapi"));
    assert!(
        stale_diag.is_some(),
        "Expected DEP-005 for stale entry (fastapi missing from uv.lock)"
    );

    // 4. Constraint drift: requirement bumped to requests>=2.31.0 but locked at 2.28.2
    fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "py-app"
version = "0.1.0"
dependencies = [
    "requests>=2.31.0",
    "pydantic>=2.0.0",
]
"#,
    )
    .unwrap();

    let diags_drift = analyze_test_dir(root);
    let drift_diag = diags_drift
        .iter()
        .find(|d| d.rule_id.0 == "DEP-005" && d.message.contains("violates manifest requirement"));
    assert!(
        drift_diag.is_some(),
        "Expected DEP-005 for requirement drift"
    );
    let msg = &drift_diag.unwrap().message;
    assert!(msg.contains("2.28.2"));
    assert!(msg.contains(">=2.31.0"));
}

#[test]
fn test_python_poetry_lock_sync_and_drift() {
    let test_dir = TestDir::new("py-poetry-drift");
    let root = test_dir.path();

    fs::write(
        root.join("pyproject.toml"),
        r#"[tool.poetry]
name = "poetry-app"
version = "0.1.0"

[tool.poetry.dependencies]
python = "^3.11"
fastapi = "^0.110.0"
"#,
    )
    .unwrap();

    // Valid poetry.lock
    fs::write(
        root.join("poetry.lock"),
        r#"
[[package]]
name = "fastapi"
version = "0.110.2"
"#,
    )
    .unwrap();

    let diags_synced = analyze_test_dir(root);
    assert!(!diags_synced.iter().any(|d| d.rule_id.0 == "DEP-005"));

    // Drift: poetry constraint updated to ^0.120.0
    fs::write(
        root.join("pyproject.toml"),
        r#"[tool.poetry]
name = "poetry-app"
version = "0.1.0"

[tool.poetry.dependencies]
python = "^3.11"
fastapi = "^0.120.0"
"#,
    )
    .unwrap();

    let diags_drift = analyze_test_dir(root);
    let drift_diag = diags_drift
        .iter()
        .find(|d| d.rule_id.0 == "DEP-005" && d.message.contains("fastapi"));
    assert!(
        drift_diag.is_some(),
        "Expected DEP-005 for poetry lock drift"
    );
}
