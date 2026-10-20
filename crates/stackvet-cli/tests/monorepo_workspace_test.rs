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
        let path = std::env::temp_dir().join(format!("stackvet-monorepo-test-{}-{}", name, ts));
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
fn test_npm_workspaces_compatible_dependencies() {
    let fixture = TestDir::new("npm-compat");
    let root = fixture.path();

    fs::write(
        root.join("package.json"),
        r#"{
  "name": "root",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .unwrap();

    let pkg_a = root.join("packages").join("pkg-a");
    let pkg_b = root.join("packages").join("pkg-b");
    fs::create_dir_all(pkg_a.join("src")).unwrap();
    fs::create_dir_all(pkg_b.join("src")).unwrap();

    fs::write(
        pkg_a.join("package.json"),
        r#"{
  "name": "pkg-a",
  "version": "1.0.0",
  "dependencies": {
    "lodash": "^4.17.20"
  }
}"#,
    )
    .unwrap();
    fs::write(
        pkg_a.join("src").join("index.ts"),
        r#"import { debounce } from "lodash"; debounce(() => {}, 100);"#,
    )
    .unwrap();

    fs::write(
        pkg_b.join("package.json"),
        r#"{
  "name": "pkg-b",
  "version": "1.0.0",
  "dependencies": {
    "lodash": "^4.17.0"
  }
}"#,
    )
    .unwrap();
    fs::write(
        pkg_b.join("src").join("index.ts"),
        r#"import { throttle } from "lodash"; throttle(() => {}, 100);"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let conflict_diags: Vec<_> = diags.iter().filter(|d| d.rule_id.0 == "NDEP-006").collect();
    assert!(
        conflict_diags.is_empty(),
        "Expected 0 NDEP-006 conflict diagnostics for compatible ranges, got: {:?}",
        conflict_diags
    );
}

#[test]
fn test_npm_workspaces_conflicting_dependencies() {
    let fixture = TestDir::new("npm-conflict");
    let root = fixture.path();

    fs::write(
        root.join("package.json"),
        r#"{
  "name": "root",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .unwrap();

    let pkg_a = root.join("packages").join("app");
    let pkg_b = root.join("packages").join("admin");
    let pkg_c = root.join("packages").join("docs");
    fs::create_dir_all(pkg_a.join("src")).unwrap();
    fs::create_dir_all(pkg_b.join("src")).unwrap();
    fs::create_dir_all(pkg_c.join("src")).unwrap();

    fs::write(
        pkg_a.join("package.json"),
        r#"{
  "name": "app",
  "version": "1.0.0",
  "dependencies": {
    "react": "^18.2.0"
  }
}"#,
    )
    .unwrap();
    fs::write(
        pkg_a.join("src").join("index.tsx"),
        r#"import React from "react"; export const App = () => <div/>;"#,
    )
    .unwrap();

    fs::write(
        pkg_b.join("package.json"),
        r#"{
  "name": "admin",
  "version": "1.0.0",
  "dependencies": {
    "react": "^17.0.2"
  }
}"#,
    )
    .unwrap();
    fs::write(
        pkg_b.join("src").join("index.tsx"),
        r#"import React from "react"; export const Admin = () => <span/>;"#,
    )
    .unwrap();

    fs::write(
        pkg_c.join("package.json"),
        r#"{
  "name": "docs",
  "version": "1.0.0",
  "dependencies": {
    "react": "^16.8.0"
  }
}"#,
    )
    .unwrap();
    fs::write(
        pkg_c.join("src").join("index.tsx"),
        r#"import React from "react"; export const Docs = () => <section/>;"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let conflict = diags.iter().find(|d| d.rule_id.0 == "NDEP-006");
    assert!(
        conflict.is_some(),
        "Expected NDEP-006 for react version conflicts"
    );
    let msg = &conflict.unwrap().message;
    assert!(
        msg.contains("react"),
        "Message should mention 'react': {}",
        msg
    );
    assert!(
        msg.contains("app") && msg.contains("^18.2.0"),
        "Message should identify 'app': {}",
        msg
    );
    assert!(
        msg.contains("admin") && msg.contains("^17.0.2"),
        "Message should identify 'admin': {}",
        msg
    );
    assert!(
        msg.contains("docs") && msg.contains("^16.8.0"),
        "Message should identify 'docs': {}",
        msg
    );
}

#[test]
fn test_pnpm_workspace_yaml_discovery_and_cross_reference() {
    let fixture = TestDir::new("pnpm-workspace");
    let root = fixture.path();

    fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n",
    )
    .unwrap();

    let ui_pkg = root.join("packages").join("ui");
    let web_pkg = root.join("packages").join("web");
    fs::create_dir_all(ui_pkg.join("src")).unwrap();
    fs::create_dir_all(web_pkg.join("src")).unwrap();

    fs::write(
        ui_pkg.join("package.json"),
        r#"{
  "name": "@myorg/ui",
  "version": "1.0.0"
}"#,
    )
    .unwrap();
    fs::write(
        ui_pkg.join("src").join("index.ts"),
        "export const Button = 'btn';",
    )
    .unwrap();

    fs::write(
        web_pkg.join("package.json"),
        r#"{
  "name": "web",
  "version": "1.0.0",
  "dependencies": {
    "@myorg/ui": "workspace:*"
  }
}"#,
    )
    .unwrap();
    fs::write(
        web_pkg.join("src").join("main.ts"),
        r#"import { Button } from "@myorg/ui"; console.log(Button);"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep001 = diags.iter().find(|d| d.rule_id.0 == "NDEP-001");
    let ndep004 = diags.iter().find(|d| d.rule_id.0 == "NDEP-004");
    assert!(
        ndep001.is_none(),
        "Expected 0 NDEP-001 for resolved workspace package, found: {:?}",
        ndep001
    );
    assert!(
        ndep004.is_none(),
        "Expected 0 NDEP-004 for valid workspace reference, found: {:?}",
        ndep004
    );
}

#[test]
fn test_lerna_json_discovery() {
    let fixture = TestDir::new("lerna-workspace");
    let root = fixture.path();

    fs::write(
        root.join("lerna.json"),
        r#"{
  "version": "1.0.0",
  "packages": ["modules/*"]
}"#,
    )
    .unwrap();

    let mod_core = root.join("modules").join("core");
    let mod_app = root.join("modules").join("app");
    fs::create_dir_all(mod_core.join("src")).unwrap();
    fs::create_dir_all(mod_app.join("src")).unwrap();

    fs::write(
        mod_core.join("package.json"),
        r#"{
  "name": "@mono/core",
  "version": "1.0.0"
}"#,
    )
    .unwrap();
    fs::write(
        mod_core.join("src").join("core.ts"),
        "export const Core = 42;",
    )
    .unwrap();

    fs::write(
        mod_app.join("package.json"),
        r#"{
  "name": "app",
  "version": "1.0.0",
  "dependencies": {
    "@mono/core": "^1.0.0"
  }
}"#,
    )
    .unwrap();
    fs::write(
        mod_app.join("src").join("main.ts"),
        r#"import { Core } from "@mono/core"; console.log(Core);"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep001 = diags.iter().find(|d| d.rule_id.0 == "NDEP-001");
    assert!(
        ndep001.is_none(),
        "Expected 0 NDEP-001 for lerna workspace member, found: {:?}",
        ndep001
    );
}

#[test]
fn test_unresolved_workspace_protocol_reference() {
    let fixture = TestDir::new("unresolved-workspace-protocol");
    let root = fixture.path();

    fs::write(
        root.join("package.json"),
        r#"{
  "name": "root",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .unwrap();

    let pkg_a = root.join("packages").join("pkg-a");
    fs::create_dir_all(pkg_a.join("src")).unwrap();

    fs::write(
        pkg_a.join("package.json"),
        r#"{
  "name": "pkg-a",
  "version": "1.0.0",
  "dependencies": {
    "@myorg/nonexistent": "workspace:*"
  }
}"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep004 = diags
        .iter()
        .find(|d| d.rule_id.0 == "NDEP-004" && d.message.contains("@myorg/nonexistent"));
    assert!(
        ndep004.is_some(),
        "Expected NDEP-004 for nonexistent workspace package, got: {:?}",
        diags
    );
}

#[test]
fn test_unresolved_broken_file_path_reference() {
    let fixture = TestDir::new("broken-file-path");
    let root = fixture.path();

    fs::write(
        root.join("package.json"),
        r#"{
  "name": "root",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .unwrap();

    let pkg_a = root.join("packages").join("pkg-a");
    fs::create_dir_all(pkg_a.join("src")).unwrap();

    fs::write(
        pkg_a.join("package.json"),
        r#"{
  "name": "pkg-a",
  "version": "1.0.0",
  "dependencies": {
    "local-util": "file:../nonexistent-util"
  }
}"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep004 = diags
        .iter()
        .find(|d| d.rule_id.0 == "NDEP-004" && d.message.contains("local-util"));
    assert!(
        ndep004.is_some(),
        "Expected NDEP-004 for broken file: path, got: {:?}",
        diags
    );
}

#[test]
fn test_python_uv_workspace_compatible_dependencies() {
    let fixture = TestDir::new("py-uv-compat");
    let root = fixture.path();

    fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "uv-root"
version = "0.1.0"

[tool.uv.workspace]
members = ["packages/*"]
"#,
    )
    .unwrap();

    let pkg_a = root.join("packages").join("pkg-a");
    let pkg_b = root.join("packages").join("pkg-b");
    fs::create_dir_all(pkg_a.join("src")).unwrap();
    fs::create_dir_all(pkg_b.join("src")).unwrap();

    fs::write(
        pkg_a.join("pyproject.toml"),
        r#"[project]
name = "pkg-a"
version = "0.1.0"
dependencies = [
    "requests>=2.25.0",
]
"#,
    )
    .unwrap();
    fs::write(
        pkg_a.join("src").join("main.py"),
        "import requests\nprint(requests.__name__)\n",
    )
    .unwrap();

    fs::write(
        pkg_b.join("pyproject.toml"),
        r#"[project]
name = "pkg-b"
version = "0.1.0"
dependencies = [
    "requests>=2.20.0",
]
"#,
    )
    .unwrap();
    fs::write(
        pkg_b.join("src").join("main.py"),
        "import requests\nprint(requests.__file__)\n",
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let conflict = diags.iter().find(|d| d.rule_id.0 == "DEP-001");
    assert!(
        conflict.is_none(),
        "Expected no DEP-001 for compatible PEP 440 constraints, got: {:?}",
        conflict
    );
}

#[test]
fn test_python_uv_workspace_conflicting_dependencies() {
    let fixture = TestDir::new("py-uv-conflict");
    let root = fixture.path();

    fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "uv-root"
version = "0.1.0"

[tool.uv.workspace]
members = ["packages/*"]
"#,
    )
    .unwrap();

    let pkg_a = root.join("packages").join("service-a");
    let pkg_b = root.join("packages").join("service-b");
    fs::create_dir_all(pkg_a.join("src")).unwrap();
    fs::create_dir_all(pkg_b.join("src")).unwrap();

    fs::write(
        pkg_a.join("pyproject.toml"),
        r#"[project]
name = "service-a"
version = "0.1.0"
dependencies = [
    "django==4.2.0",
]
"#,
    )
    .unwrap();
    fs::write(
        pkg_a.join("src").join("main.py"),
        "import django\nprint(django.__name__)\n",
    )
    .unwrap();

    fs::write(
        pkg_b.join("pyproject.toml"),
        r#"[project]
name = "service-b"
version = "0.1.0"
dependencies = [
    "django==3.2.0",
]
"#,
    )
    .unwrap();
    fs::write(
        pkg_b.join("src").join("main.py"),
        "import django\nprint(django.__name__)\n",
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let conflict = diags.iter().find(|d| d.rule_id.0 == "DEP-001");
    assert!(
        conflict.is_some(),
        "Expected DEP-001 for conflicting django versions in Python workspace"
    );
    let msg = &conflict.unwrap().message;
    assert!(
        msg.contains("django"),
        "Message should mention django: {}",
        msg
    );
    assert!(
        msg.contains("service-a") && msg.contains("4.2.0"),
        "Message should identify service-a: {}",
        msg
    );
    assert!(
        msg.contains("service-b") && msg.contains("3.2.0"),
        "Message should identify service-b: {}",
        msg
    );
}

#[test]
fn test_internal_workspace_package_missing_remediation_hint() {
    let fixture = TestDir::new("ws-missing-remediation");
    let root = fixture.path();

    fs::write(
        root.join("package.json"),
        r#"{
  "name": "root",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .unwrap();

    let pkg_a = root.join("packages").join("pkg-a");
    let pkg_b = root.join("packages").join("pkg-b");
    fs::create_dir_all(pkg_a.join("src")).unwrap();
    fs::create_dir_all(pkg_b.join("src")).unwrap();

    fs::write(
        pkg_b.join("package.json"),
        r#"{
  "name": "@myorg/ui",
  "version": "1.0.0"
}"#,
    )
    .unwrap();
    fs::write(
        pkg_b.join("src").join("index.ts"),
        "export const Button = 1;",
    )
    .unwrap();

    // pkg-a does NOT declare @myorg/ui in package.json
    fs::write(
        pkg_a.join("package.json"),
        r#"{
  "name": "pkg-a",
  "version": "1.0.0"
}"#,
    )
    .unwrap();
    fs::write(
        pkg_a.join("src").join("main.ts"),
        r#"import { Button } from "@myorg/ui"; console.log(Button);"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep001 = diags.iter().find(|d| d.rule_id.0 == "NDEP-001");
    assert!(
        ndep001.is_some(),
        "Expected NDEP-001 for undeclared internal workspace package"
    );
    let remediation = &ndep001.unwrap().suggested_remediation;
    assert!(
        remediation.contains("workspace:*"),
        "Remediation should suggest workspace:* protocol: {}",
        remediation
    );
}

#[test]
fn test_workspace_protocol_version_mismatch() {
    let fixture = TestDir::new("ws-ver-mismatch");
    let root = fixture.path();

    fs::write(
        root.join("package.json"),
        r#"{
  "name": "root",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .unwrap();

    let pkg_a = root.join("packages").join("pkg-a");
    let pkg_b = root.join("packages").join("pkg-b");
    fs::create_dir_all(pkg_a.join("src")).unwrap();
    fs::create_dir_all(pkg_b.join("src")).unwrap();

    fs::write(
        pkg_b.join("package.json"),
        r#"{
  "name": "@myorg/utils",
  "version": "1.0.0"
}"#,
    )
    .unwrap();
    fs::write(
        pkg_b.join("src").join("index.ts"),
        "export const util = true;",
    )
    .unwrap();

    // pkg-a requires workspace:^2.0.0 but actual is 1.0.0
    fs::write(
        pkg_a.join("package.json"),
        r#"{
  "name": "pkg-a",
  "version": "1.0.0",
  "dependencies": {
    "@myorg/utils": "workspace:^2.0.0"
  }
}"#,
    )
    .unwrap();

    let diags = analyze_test_dir(root);
    let ndep004 = diags.iter().find(|d| d.rule_id.0 == "NDEP-004");
    assert!(
        ndep004.is_some(),
        "Expected NDEP-004 for workspace version mismatch"
    );
    let msg = &ndep004.unwrap().message;
    assert!(
        msg.contains("@myorg/utils") && msg.contains("^2.0.0") && msg.contains("1.0.0"),
        "Message should report constraint mismatch details: {}",
        msg
    );
}
