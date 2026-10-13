pub mod stdlib;

use anyhow::Result;
use ruff_python_ast::visitor::{walk_expr, walk_stmt, Visitor};
use ruff_python_ast::{
    Expr, ExprAttribute, ExprCall, ExprName, ExprStringLiteral, Stmt, StmtImport, StmtImportFrom,
};
use ruff_python_parser::parse_module;
use stackvet_core::PythonProjectInfo;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonImport {
    pub module: String,
    pub symbol: Option<String>,
    pub line: usize,
    pub is_relative: bool,
    pub level: usize,
    pub is_dynamic: bool,
}

struct ImportVisitor<'b> {
    content: &'b str,
    imports: Vec<PythonImport>,
}

impl<'a, 'b> Visitor<'a> for ImportVisitor<'b> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::Import(StmtImport { names, range, .. }) => {
                let offset = usize::from(range.start());
                let line = self.content[..offset.min(self.content.len())]
                    .matches('\n')
                    .count()
                    + 1;
                for alias in names {
                    self.imports.push(PythonImport {
                        module: alias.name.to_string(),
                        symbol: None,
                        line,
                        is_relative: false,
                        level: 0,
                        is_dynamic: false,
                    });
                }
            }
            Stmt::ImportFrom(StmtImportFrom {
                module,
                names,
                level,
                range,
                ..
            }) => {
                let offset = usize::from(range.start());
                let line = self.content[..offset.min(self.content.len())]
                    .matches('\n')
                    .count()
                    + 1;
                let mod_name = module.as_ref().map(|m| m.to_string()).unwrap_or_default();
                let lvl = *level as usize;
                for alias in names {
                    self.imports.push(PythonImport {
                        module: mod_name.clone(),
                        symbol: Some(alias.name.to_string()),
                        line,
                        is_relative: lvl > 0,
                        level: lvl,
                        is_dynamic: false,
                    });
                }
            }
            _ => {
                walk_stmt(self, stmt);
            }
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(ExprCall {
            func,
            arguments,
            range,
            ..
        }) = expr
        {
            let is_dynamic_import = match func.as_ref() {
                Expr::Name(ExprName { id, .. }) if id.as_str() == "__import__" => true,
                Expr::Attribute(ExprAttribute { value, attr, .. })
                    if attr.as_str() == "import_module" =>
                {
                    matches!(value.as_ref(), Expr::Name(ExprName { id, .. }) if id.as_str() == "importlib")
                }
                _ => false,
            };

            if is_dynamic_import {
                if let Some(first_arg) = arguments.args.first() {
                    if let Expr::StringLiteral(ExprStringLiteral { value, .. }) = first_arg {
                        let raw_mod = value.to_str();
                        let offset = usize::from(range.start());
                        let line = self.content[..offset.min(self.content.len())]
                            .matches('\n')
                            .count()
                            + 1;
                        let leading_dots = raw_mod.chars().take_while(|&c| c == '.').count();
                        let is_relative = leading_dots > 0;
                        let clean_mod = if is_relative {
                            raw_mod.trim_start_matches('.').to_string()
                        } else {
                            raw_mod.to_string()
                        };
                        self.imports.push(PythonImport {
                            module: clean_mod,
                            symbol: None,
                            line,
                            is_relative,
                            level: leading_dots,
                            is_dynamic: true,
                        });
                    }
                }
            }
        }
        walk_expr(self, expr);
    }
}

pub fn parse_python_imports(content: &str, _file: &Path) -> Vec<PythonImport> {
    let mut visitor = ImportVisitor {
        content,
        imports: Vec::new(),
    };
    if let Ok(parsed) = parse_module(content) {
        for stmt in parsed.suite() {
            visitor.visit_stmt(stmt);
        }
    }
    visitor.imports
}

/// Normalizes Python package names (PEP 503: lowercase, hyphens/underscores normalized)
pub fn normalize_package_name(name: &str) -> String {
    name.to_lowercase().replace('_', "-")
}

/// Parses requirement spec like "requests>=2.28.0", "fastapi[all]~=0.100.0", "pytest"
pub fn parse_requirement_spec(spec: &str) -> Option<(String, String)> {
    let trimmed = spec.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
        return None;
    }

    // Strip environment markers: "requests; python_version >= '3.8'"
    let clean_spec = trimmed.split(';').next().unwrap_or(trimmed).trim();

    // Strip extras: "fastapi[all]" -> "fastapi"
    let (name_part, rest) = if let Some(bracket_idx) = clean_spec.find('[') {
        let name = &clean_spec[..bracket_idx];
        let after_bracket = &clean_spec[bracket_idx..];
        let after_extra = if let Some(close_bracket) = after_bracket.find(']') {
            &after_bracket[close_bracket + 1..]
        } else {
            ""
        };
        (name.trim(), after_extra.trim())
    } else {
        // Find comparison operators: ==, >=, <=, ~=, !=, >, <
        let op_indices: Vec<usize> = clean_spec
            .match_indices(|c| c == '=' || c == '>' || c == '<' || c == '~' || c == '!')
            .map(|(idx, _)| idx)
            .collect();
        if let Some(&first_op) = op_indices.first() {
            (clean_spec[..first_op].trim(), clean_spec[first_op..].trim())
        } else {
            (clean_spec.trim(), "")
        }
    };

    if name_part.is_empty() {
        return None;
    }

    Some((normalize_package_name(name_part), rest.to_string()))
}

pub fn parse_pyproject_toml(content: &str, path: &Path) -> Result<PythonProjectInfo> {
    let parsed: toml::Value = toml::from_str(content)?;
    let mut info = PythonProjectInfo {
        manifest_file: path.to_path_buf(),
        package_manager: "pip".to_string(),
        ..Default::default()
    };

    // Check [project] (PEP 621)
    if let Some(project) = parsed.get("project") {
        if let Some(req_py) = project.get("requires-python").and_then(|v| v.as_str()) {
            info.requires_python = Some(req_py.to_string());
        }

        if let Some(deps) = project.get("dependencies").and_then(|v| v.as_array()) {
            for dep in deps {
                if let Some(dep_str) = dep.as_str() {
                    if let Some((name, ver)) = parse_requirement_spec(dep_str) {
                        info.direct_dependencies.insert(name, ver);
                    }
                }
            }
        }

        if let Some(opt_deps) = project
            .get("optional-dependencies")
            .and_then(|v| v.as_table())
        {
            for (_group, deps_val) in opt_deps {
                if let Some(deps_arr) = deps_val.as_array() {
                    for dep in deps_arr {
                        if let Some(dep_str) = dep.as_str() {
                            if let Some((name, ver)) = parse_requirement_spec(dep_str) {
                                info.dev_dependencies.insert(name, ver);
                            }
                        }
                    }
                }
            }
        }
    }

    // Check [tool.poetry]
    if let Some(poetry) = parsed
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.as_table())
    {
        info.package_manager = "poetry".to_string();
        if let Some(deps) = poetry.get("dependencies").and_then(|d| d.as_table()) {
            for (pkg, val) in deps {
                if pkg == "python" {
                    if let Some(ver_str) = val.as_str() {
                        info.requires_python = Some(ver_str.to_string());
                    }
                } else {
                    let ver = match val {
                        toml::Value::String(s) => s.clone(),
                        toml::Value::Table(t) => t
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        _ => String::new(),
                    };
                    info.direct_dependencies
                        .insert(normalize_package_name(pkg), ver);
                }
            }
        }

        if let Some(group) = poetry
            .get("group")
            .and_then(|g| g.get("dev"))
            .and_then(|d| d.get("dependencies"))
            .and_then(|deps| deps.as_table())
        {
            for (pkg, val) in group {
                let ver = val.as_str().unwrap_or("").to_string();
                info.dev_dependencies
                    .insert(normalize_package_name(pkg), ver);
            }
        }
    }

    // Check [tool.uv]
    if parsed.get("tool").and_then(|t| t.get("uv")).is_some() {
        info.package_manager = "uv".to_string();
    }

    Ok(info)
}

pub fn parse_requirements_txt(content: &str, path: &Path) -> PythonProjectInfo {
    let mut info = PythonProjectInfo {
        manifest_file: path.to_path_buf(),
        package_manager: "pip".to_string(),
        ..Default::default()
    };

    for line in content.lines() {
        if let Some((name, ver)) = parse_requirement_spec(line) {
            info.direct_dependencies.insert(name, ver);
        }
    }

    info
}

pub fn parse_uv_lock(content: &str) -> HashMap<String, String> {
    let mut packages = HashMap::new();
    if let Ok(val) = toml::from_str::<toml::Value>(content) {
        if let Some(pkgs) = val.get("package").and_then(|p| p.as_array()) {
            for pkg in pkgs {
                if let (Some(name), Some(version)) = (
                    pkg.get("name").and_then(|n| n.as_str()),
                    pkg.get("version").and_then(|v| v.as_str()),
                ) {
                    packages.insert(normalize_package_name(name), version.to_string());
                }
            }
        }
    }
    packages
}

pub fn detect_python_project(root: &Path) -> Option<PythonProjectInfo> {
    let pyproject = root.join("pyproject.toml");
    if pyproject.is_file() {
        if let Ok(content) = fs::read_to_string(&pyproject) {
            if let Ok(mut info) = parse_pyproject_toml(&content, &pyproject) {
                let uv_lock = root.join("uv.lock");
                if uv_lock.is_file() {
                    info.lock_file = Some(uv_lock.clone());
                    info.package_manager = "uv".to_string();
                    if let Ok(lock_str) = fs::read_to_string(&uv_lock) {
                        info.installed_packages = parse_uv_lock(&lock_str);
                    }
                } else {
                    let poetry_lock = root.join("poetry.lock");
                    if poetry_lock.is_file() {
                        info.lock_file = Some(poetry_lock);
                        info.package_manager = "poetry".to_string();
                    }
                }
                return Some(info);
            }
        }
    }

    let req_txt = root.join("requirements.txt");
    if req_txt.is_file() {
        if let Ok(content) = fs::read_to_string(&req_txt) {
            return Some(parse_requirements_txt(&content, &req_txt));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_python_imports() {
        let code = r#"
import os
import sys, math
from pathlib import Path
from .local_mod import helper
import requests
"#;
        let imports = parse_python_imports(code, Path::new("test.py"));
        let modules: Vec<&str> = imports.iter().map(|i| i.module.as_str()).collect();
        assert!(modules.contains(&"os"));
        assert!(modules.contains(&"sys"));
        assert!(modules.contains(&"math"));
        assert!(modules.contains(&"pathlib"));
        assert!(modules.contains(&"local_mod"));
        assert!(modules.contains(&"requests"));

        let rel = imports.iter().find(|i| i.module == "local_mod").unwrap();
        assert!(rel.is_relative);
        assert_eq!(rel.level, 1);
        assert_eq!(rel.symbol.as_deref(), Some("helper"));
    }

    #[test]
    fn test_parse_relative_and_nested_imports() {
        let code = r#"
from . import sibling
from ..parent_pkg import util
from ...ancestor.core import engine

def load_plugins():
    import plugin_a
    try:
        from optional_pkg import feature
    except ImportError:
        import fallback_pkg

class Service:
    def execute(self):
        import worker
"#;
        let imports = parse_python_imports(code, Path::new("service.py"));
        let modules: Vec<&str> = imports.iter().map(|i| i.module.as_str()).collect();

        // Check relative imports with varying levels
        let rel_sibling = imports
            .iter()
            .find(|i| i.symbol.as_deref() == Some("sibling"))
            .unwrap();
        assert!(rel_sibling.is_relative);
        assert_eq!(rel_sibling.level, 1);
        assert_eq!(rel_sibling.module, "");

        let rel_parent = imports.iter().find(|i| i.module == "parent_pkg").unwrap();
        assert!(rel_parent.is_relative);
        assert_eq!(rel_parent.level, 2);
        assert_eq!(rel_parent.symbol.as_deref(), Some("util"));

        let rel_ancestor = imports
            .iter()
            .find(|i| i.module == "ancestor.core")
            .unwrap();
        assert!(rel_ancestor.is_relative);
        assert_eq!(rel_ancestor.level, 3);
        assert_eq!(rel_ancestor.symbol.as_deref(), Some("engine"));

        // Check nested imports inside function, try/except, and class method
        assert!(modules.contains(&"plugin_a"));
        assert!(modules.contains(&"optional_pkg"));
        assert!(modules.contains(&"fallback_pkg"));
        assert!(modules.contains(&"worker"));
    }

    #[test]
    fn test_parse_dynamic_imports() {
        let code = r#"
import os
backend = __import__("custom_backend")
import importlib
driver = importlib.import_module("database_driver")
"#;
        let imports = parse_python_imports(code, Path::new("app.py"));
        let dynamic_imports: Vec<&PythonImport> = imports.iter().filter(|i| i.is_dynamic).collect();
        assert_eq!(dynamic_imports.len(), 2);

        let dyn_backend = dynamic_imports
            .iter()
            .find(|i| i.module == "custom_backend")
            .unwrap();
        assert!(dyn_backend.is_dynamic);
        assert!(!dyn_backend.is_relative);

        let dyn_driver = dynamic_imports
            .iter()
            .find(|i| i.module == "database_driver")
            .unwrap();
        assert!(dyn_driver.is_dynamic);
        assert!(!dyn_driver.is_relative);
    }

    #[test]
    fn test_parse_requirement_spec() {
        assert_eq!(
            parse_requirement_spec("requests>=2.28.0"),
            Some(("requests".to_string(), ">=2.28.0".to_string()))
        );
        assert_eq!(
            parse_requirement_spec("FastAPI[all]~=0.100.0"),
            Some(("fastapi".to_string(), "~=0.100.0".to_string()))
        );
        assert_eq!(
            parse_requirement_spec("uvicorn"),
            Some(("uvicorn".to_string(), "".to_string()))
        );
        assert_eq!(
            parse_requirement_spec("pydantic_core >= 2.0"),
            Some(("pydantic-core".to_string(), ">= 2.0".to_string()))
        );
    }

    #[test]
    fn test_parse_pyproject() {
        let toml_str = r#"
[project]
name = "my-app"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = [
    "fastapi>=0.100.0",
    "uvicorn[standard]>=0.20.0"
]

[project.optional-dependencies]
dev = [
    "pytest>=7.0.0"
]
"#;
        let info = parse_pyproject_toml(toml_str, Path::new("pyproject.toml")).unwrap();
        assert_eq!(info.requires_python.as_deref(), Some(">=3.11"));
        assert!(info.direct_dependencies.contains_key("fastapi"));
        assert!(info.direct_dependencies.contains_key("uvicorn"));
        assert!(info.dev_dependencies.contains_key("pytest"));
    }
}
