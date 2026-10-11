use anyhow::{Context, Result};
use stackvet_core::FixAction;
use std::fs;
use toml_edit::{Array, DocumentMut, Item, Value};

/// Injects dependency into pyproject.toml without altering existing formatting or comments
pub fn add_dependency_to_pyproject(
    content: &str,
    package: &str,
    version: &str,
) -> Result<String> {
    let mut doc: DocumentMut = content
        .parse()
        .context("Failed to parse pyproject.toml for editing")?;

    let spec = if version.is_empty() {
        package.to_string()
    } else if version.starts_with('>') || version.starts_with('=') || version.starts_with('~') {
        format!("{}{}", package, version)
    } else {
        format!("{}>={}", package, version)
    };

    if let Some(project) = doc.get_mut("project").and_then(Item::as_table_mut) {
        if let Some(deps) = project.get_mut("dependencies").and_then(Item::as_array_mut) {
            // Check if already present
            let already_has = deps.iter().any(|v| {
                v.as_str()
                    .map_or(false, |s| s.starts_with(package) || s == package)
            });
            if !already_has {
                deps.push(spec);
            }
        } else {
            let mut arr = Array::new();
            arr.push(spec);
            project.insert("dependencies", Item::Value(Value::Array(arr)));
        }
    } else {
        // Fallback or poetry
        if let Some(tool) = doc.get_mut("tool").and_then(Item::as_table_mut) {
            if let Some(poetry) = tool.get_mut("poetry").and_then(Item::as_table_mut) {
                if let Some(deps) = poetry.get_mut("dependencies").and_then(Item::as_table_mut) {
                    let ver_spec = if version.is_empty() {
                        "*"
                    } else {
                        version
                    };
                    deps.insert(package, Item::Value(Value::from(ver_spec)));
                }
            }
        }
    }

    Ok(doc.to_string())
}

/// Adds dependency to package.json dependencies object
pub fn add_dependency_to_package_json(
    content: &str,
    package: &str,
    version: &str,
) -> Result<String> {
    let mut val: serde_json::Value =
        serde_json::from_str(content).context("Failed to parse package.json")?;

    let ver_spec = if version.is_empty() {
        "latest"
    } else {
        version
    };

    if let Some(obj) = val.as_object_mut() {
        let deps = obj
            .entry("dependencies")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if let Some(deps_map) = deps.as_object_mut() {
            deps_map.insert(
                package.to_string(),
                serde_json::Value::String(ver_spec.to_string()),
            );
        }
    }

    Ok(serde_json::to_string_pretty(&val)?)
}

pub struct RemediationEngine;

impl RemediationEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn apply_fix(&self, fix: &FixAction) -> Result<()> {
        let path = &fix.file_path;
        if !path.is_file() {
            anyhow::bail!("Target file does not exist: {}", path.display());
        }

        let content = fs::read_to_string(path)?;
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();

        let updated_content = if file_name == "pyproject.toml" {
            add_dependency_to_pyproject(&content, &fix.replacement, "")?
        } else if file_name == "package.json" {
            add_dependency_to_package_json(&content, &fix.replacement, "*")?
        } else {
            // Text replacement if span provided
            content
        };

        fs::write(path, updated_content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_dependency_to_pyproject() {
        let original = r#"
[project]
name = "sample"
version = "0.1.0"
dependencies = [
    "requests>=2.28.0",
]
"#;
        let modified = add_dependency_to_pyproject(original, "fastapi", "0.100.0").unwrap();
        assert!(modified.contains("\"fastapi>=0.100.0\""));
        assert!(modified.contains("\"requests>=2.28.0\""));
    }

    #[test]
    fn test_add_dependency_to_package_json() {
        let original = r#"{
  "name": "sample",
  "dependencies": {
    "react": "^18.0.0"
  }
}"#;
        let modified = add_dependency_to_package_json(original, "axios", "^1.6.0").unwrap();
        assert!(modified.contains("\"axios\": \"^1.6.0\""));
    }
}
