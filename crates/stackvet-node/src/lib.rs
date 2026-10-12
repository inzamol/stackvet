pub mod builtins;

use anyhow::Result;
use oxc_allocator::Allocator;
use oxc_ast::ast::ModuleDeclaration;
use oxc_parser::Parser;
use oxc_span::SourceType;
use stackvet_core::NodeProjectInfo;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeImport {
    pub specifier: String,
    pub is_dynamic: bool,
    pub is_type_only: bool,
    pub line: usize,
}

pub fn parse_node_imports(content: &str, file: &Path) -> Vec<NodeImport> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(file).unwrap_or_default();
    let ret = Parser::new(&allocator, content, source_type).parse();

    let mut imports = Vec::new();
    for stmt in &ret.program.body {
        if let Some(mod_decl) = stmt.as_module_declaration() {
            if let ModuleDeclaration::ImportDeclaration(import_decl) = mod_decl {
                let specifier = import_decl.source.value.to_string();
                let offset = import_decl.span.start as usize;
                let line = content[..offset.min(content.len())].matches('\n').count() + 1;
                imports.push(NodeImport {
                    specifier,
                    is_dynamic: false,
                    is_type_only: import_decl.import_kind.is_type(),
                    line,
                });
            }
        }
    }
    imports
}

/// Normalizes package name from import specifier:
/// "@scope/pkg/foo" -> "@scope/pkg"
/// "axios/lib/adapters" -> "axios"
/// "./utils" -> None (local import)
pub fn extract_package_name(specifier: &str) -> Option<String> {
    if specifier.starts_with('.') || specifier.starts_with('/') {
        return None;
    }
    let parts: Vec<&str> = specifier.split('/').collect();
    if specifier.starts_with('@') {
        if parts.len() >= 2 {
            Some(format!("{}/{}", parts[0], parts[1]))
        } else {
            Some(specifier.to_string())
        }
    } else {
        parts.first().map(|s| s.to_string())
    }
}

pub fn parse_package_json(content: &str, path: &Path) -> Result<NodeProjectInfo> {
    let val: serde_json::Value = serde_json::from_str(content)?;
    let mut info = NodeProjectInfo {
        package_json_path: path.to_path_buf(),
        package_manager: "npm".to_string(),
        ..Default::default()
    };

    if let Some(pm) = val.get("packageManager").and_then(|v| v.as_str()) {
        let name = pm.split('@').next().unwrap_or(pm);
        info.package_manager = name.to_string();
    }

    if let Some(engines) = val.get("engines").and_then(|v| v.as_object()) {
        if let Some(node_ver) = engines.get("node").and_then(|v| v.as_str()) {
            info.node_version_constraint = Some(node_ver.to_string());
        }
    }

    if let Some(deps) = val.get("dependencies").and_then(|v| v.as_object()) {
        for (k, v) in deps {
            info.dependencies
                .insert(k.clone(), v.as_str().unwrap_or("").to_string());
        }
    }

    if let Some(dev_deps) = val.get("devDependencies").and_then(|v| v.as_object()) {
        for (k, v) in dev_deps {
            info.dev_dependencies
                .insert(k.clone(), v.as_str().unwrap_or("").to_string());
        }
    }

    if let Some(peer_deps) = val.get("peerDependencies").and_then(|v| v.as_object()) {
        for (k, v) in peer_deps {
            info.peer_dependencies
                .insert(k.clone(), v.as_str().unwrap_or("").to_string());
        }
    }

    Ok(info)
}

pub fn detect_node_project(root: &Path) -> Option<NodeProjectInfo> {
    let pkg_json = root.join("package.json");
    if pkg_json.is_file() {
        if let Ok(content) = fs::read_to_string(&pkg_json) {
            if let Ok(mut info) = parse_package_json(&content, &pkg_json) {
                // Detect lockfiles
                let pnpm_lock = root.join("pnpm-lock.yaml");
                let yarn_lock = root.join("yarn.lock");
                let npm_lock = root.join("package-lock.json");
                let bun_lock = root.join("bun.lockb");

                if pnpm_lock.is_file() {
                    info.lock_file = Some(pnpm_lock);
                    info.package_manager = "pnpm".to_string();
                } else if yarn_lock.is_file() {
                    info.lock_file = Some(yarn_lock);
                    info.package_manager = "yarn".to_string();
                } else if npm_lock.is_file() {
                    info.lock_file = Some(npm_lock);
                    info.package_manager = "npm".to_string();
                } else if bun_lock.is_file() {
                    info.lock_file = Some(bun_lock);
                    info.package_manager = "bun".to_string();
                }

                return Some(info);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_name() {
        assert_eq!(extract_package_name("react"), Some("react".to_string()));
        assert_eq!(
            extract_package_name("lodash/fp/map"),
            Some("lodash".to_string())
        );
        assert_eq!(
            extract_package_name("@tanstack/react-query"),
            Some("@tanstack/react-query".to_string())
        );
        assert_eq!(
            extract_package_name("@tanstack/react-query/devtools"),
            Some("@tanstack/react-query".to_string())
        );
        assert_eq!(extract_package_name("./components/Button"), None);
        assert_eq!(extract_package_name("../utils"), None);
    }

    #[test]
    fn test_parse_package_json() {
        let content = r#"{
            "name": "web-app",
            "packageManager": "pnpm@9.1.0",
            "engines": {
                "node": ">=20.0.0"
            },
            "dependencies": {
                "react": "^18.2.0",
                "next": "^14.2.0"
            },
            "devDependencies": {
                "typescript": "^5.4.0",
                "vitest": "^1.4.0"
            }
        }"#;

        let info = parse_package_json(content, Path::new("package.json")).unwrap();
        assert_eq!(info.package_manager, "pnpm");
        assert_eq!(info.node_version_constraint.as_deref(), Some(">=20.0.0"));
        assert_eq!(
            info.dependencies.get("react").map(|s| s.as_str()),
            Some("^18.2.0")
        );
        assert_eq!(
            info.dev_dependencies.get("vitest").map(|s| s.as_str()),
            Some("^1.4.0")
        );
    }

    #[test]
    fn test_parse_node_imports() {
        let code = r#"
import React, { useState } from 'react';
import type { User } from './types';
import { Button } from '@mui/material';
const x = 1;
"#;
        let imports = parse_node_imports(code, Path::new("test.tsx"));
        let specs: Vec<&str> = imports.iter().map(|i| i.specifier.as_str()).collect();
        assert!(specs.contains(&"react"));
        assert!(specs.contains(&"./types"));
        assert!(specs.contains(&"@mui/material"));

        let type_import = imports.iter().find(|i| i.specifier == "./types").unwrap();
        assert!(type_import.is_type_only);
    }
}
