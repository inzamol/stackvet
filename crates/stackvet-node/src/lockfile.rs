use anyhow::Result;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LockfileData {
    pub locked_packages: HashMap<String, String>,
    pub locked_versions: HashMap<String, Vec<String>>,
    pub workspace_packages: HashMap<String, String>,
}

/// Extracts package name from a node_modules path.
/// E.g. "node_modules/react" -> "react"
/// "node_modules/@scope/pkg" -> "@scope/pkg"
/// "node_modules/foo/node_modules/bar" -> "bar"
/// "node_modules/foo/node_modules/@scope/bar" -> "@scope/bar"
pub fn extract_pkg_name_from_node_modules_path(path: &str) -> Option<String> {
    let clean = path.replace('\\', "/");
    let marker = "node_modules/";
    let last_nm = clean.rfind(marker)?;
    let sub = &clean[last_nm + marker.len()..];
    let parts: Vec<&str> = sub.split('/').collect();
    if parts.is_empty() || parts[0].is_empty() {
        return None;
    }

    if parts[0].starts_with('@') && parts.len() >= 2 {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        Some(parts[0].to_string())
    }
}

/// Parses npm package-lock.json supporting v1, v2, and v3 formats.
pub fn parse_npm_lock(content: &str) -> Result<LockfileData> {
    let mut data = LockfileData::default();
    let val: serde_json::Value = serde_json::from_str(content)?;

    // 1. Check v2/v3 "packages" object
    if let Some(packages) = val.get("packages").and_then(|p| p.as_object()) {
        for (pkg_path, pkg_info) in packages {
            if pkg_path.is_empty() {
                // Root package entry
                continue;
            }

            let is_link = pkg_info
                .get("link")
                .and_then(|l| l.as_bool())
                .unwrap_or(false);

            if pkg_path.contains("node_modules/") {
                if let Some(name) = extract_pkg_name_from_node_modules_path(pkg_path) {
                    if let Some(ver) = pkg_info.get("version").and_then(|v| v.as_str()) {
                        let is_top_level = !pkg_path[13..].contains("/node_modules/");
                        data.locked_versions
                            .entry(name.clone())
                            .or_default()
                            .push(ver.to_string());
                        if is_top_level || !data.locked_packages.contains_key(&name) {
                            data.locked_packages.insert(name, ver.to_string());
                        }
                    } else if is_link {
                        // Symlinked workspace package
                        if let Some(resolved) = pkg_info.get("resolved").and_then(|r| r.as_str()) {
                            // Target might be packages/ui
                            if let Some(target_info) = packages.get(resolved) {
                                if let Some(ver) =
                                    target_info.get("version").and_then(|v| v.as_str())
                                {
                                    data.locked_packages.insert(name.clone(), ver.to_string());
                                    data.workspace_packages.insert(name, ver.to_string());
                                }
                            }
                        }
                    }
                }
            } else {
                // Workspace member entry (e.g. "packages/ui")
                if let (Some(name), Some(ver)) = (
                    pkg_info.get("name").and_then(|n| n.as_str()),
                    pkg_info.get("version").and_then(|v| v.as_str()),
                ) {
                    data.workspace_packages
                        .insert(name.to_string(), ver.to_string());
                }
            }
        }
    }

    // 2. Check v1 "dependencies" object if locked_packages is still empty or as fallback
    if let Some(deps) = val.get("dependencies").and_then(|d| d.as_object()) {
        parse_npm_v1_dependencies(deps, &mut data);
    }

    Ok(data)
}

fn parse_npm_v1_dependencies(
    deps: &serde_json::Map<String, serde_json::Value>,
    data: &mut LockfileData,
) {
    for (name, dep_info) in deps {
        if let Some(ver) = dep_info.get("version").and_then(|v| v.as_str()) {
            data.locked_versions
                .entry(name.clone())
                .or_default()
                .push(ver.to_string());
            if !data.locked_packages.contains_key(name) {
                data.locked_packages.insert(name.clone(), ver.to_string());
            }
        }
        if let Some(nested) = dep_info.get("dependencies").and_then(|d| d.as_object()) {
            parse_npm_v1_dependencies(nested, data);
        }
    }
}

/// Strips pnpm peer dependency suffix or build metadata.
/// E.g. "18.2.0(react-dom@18.2.0)" -> "18.2.0"
/// "18.2.0_react@18.2.0" -> "18.2.0"
pub fn clean_pnpm_version(raw: &str) -> String {
    let mut ver = raw.trim().trim_matches('"').trim_matches('\'');
    if let Some(paren_idx) = ver.find('(') {
        ver = &ver[..paren_idx];
    }
    if let Some(und_idx) = ver.find('_') {
        let left = &ver[..und_idx];
        if Version::parse(left).is_ok() {
            ver = left;
        }
    }
    ver.to_string()
}

/// Parses pnpm-lock.yaml (v5, v6, v9).
pub fn parse_pnpm_lock(content: &str) -> Result<LockfileData> {
    let mut data = LockfileData::default();
    let val: serde_yaml::Value = serde_yaml::from_str(content)?;

    // 1. Check importers (pnpm v6+)
    if let Some(importers) = val.get("importers").and_then(|i| i.as_mapping()) {
        for (importer_key, importer_val) in importers {
            let importer_str = importer_key.as_str().unwrap_or("");
            let is_root = importer_str == "." || importer_str.is_empty();

            for dep_section in ["dependencies", "devDependencies"] {
                if let Some(deps) = importer_val.get(dep_section).and_then(|d| d.as_mapping()) {
                    for (k, v) in deps {
                        let pkg_name = match k.as_str() {
                            Some(s) => s.to_string(),
                            None => continue,
                        };

                        if let Some(ver_str) = v.as_str() {
                            if ver_str.starts_with("link:") || ver_str.starts_with("workspace:") {
                                // Workspace reference
                                data.workspace_packages
                                    .entry(pkg_name.clone())
                                    .or_insert_with(|| "0.0.0".to_string());
                            } else {
                                let clean = clean_pnpm_version(ver_str);
                                data.locked_versions
                                    .entry(pkg_name.clone())
                                    .or_default()
                                    .push(clean.clone());
                                if is_root || !data.locked_packages.contains_key(&pkg_name) {
                                    data.locked_packages.insert(pkg_name, clean);
                                }
                            }
                        } else if let Some(map) = v.as_mapping() {
                            if let Some(ver_str) = map.get("version").and_then(|ver| ver.as_str()) {
                                if ver_str.starts_with("link:") || ver_str.starts_with("workspace:")
                                {
                                    data.workspace_packages
                                        .entry(pkg_name.clone())
                                        .or_insert_with(|| "0.0.0".to_string());
                                } else {
                                    let clean = clean_pnpm_version(ver_str);
                                    data.locked_versions
                                        .entry(pkg_name.clone())
                                        .or_default()
                                        .push(clean.clone());
                                    if is_root || !data.locked_packages.contains_key(&pkg_name) {
                                        data.locked_packages.insert(pkg_name, clean);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Older pnpm format top-level dependencies & devDependencies
    for dep_section in ["dependencies", "devDependencies"] {
        if let Some(deps) = val.get(dep_section).and_then(|d| d.as_mapping()) {
            for (k, v) in deps {
                if let (Some(pkg_name), Some(ver_str)) = (k.as_str(), v.as_str()) {
                    let clean = clean_pnpm_version(ver_str);
                    data.locked_versions
                        .entry(pkg_name.to_string())
                        .or_default()
                        .push(clean.clone());
                    data.locked_packages
                        .entry(pkg_name.to_string())
                        .or_insert(clean);
                }
            }
        }
    }

    // 3. packages mapping (top-level catalog of all locked packages)
    if let Some(packages) = val.get("packages").and_then(|p| p.as_mapping()) {
        for (pkg_key, _pkg_val) in packages {
            if let Some(key_str) = pkg_key.as_str() {
                // Formats: "/react@18.2.0", "react@18.2.0", "/@scope/pkg@1.0.0", "@scope/pkg@1.0.0"
                let trimmed = key_str.trim_start_matches('/');
                let clean_key = clean_pnpm_version(trimmed);
                if let Some(last_at) = clean_key.rfind('@') {
                    if last_at > 0 {
                        let name = &clean_key[..last_at];
                        let ver = &clean_key[last_at + 1..];
                        if !ver.is_empty() {
                            data.locked_versions
                                .entry(name.to_string())
                                .or_default()
                                .push(ver.to_string());
                            data.locked_packages
                                .entry(name.to_string())
                                .or_insert_with(|| ver.to_string());
                        }
                    }
                }
            }
        }
    }

    Ok(data)
}

/// Parses yarn.lock (classic v1 and berry v2+).
pub fn parse_yarn_lock(content: &str) -> LockfileData {
    let mut data = LockfileData::default();
    let mut current_packages: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Header line: ends with ':' and is not indented (or starts at col 0)
        if !line.starts_with(' ') && !line.starts_with('\t') && trimmed.ends_with(':') {
            current_packages.clear();
            let header = trimmed.trim_end_matches(':');
            for item in header.split(',') {
                let spec = item.trim().trim_matches('"').trim_matches('\'');
                // Extract package name:
                // e.g. "react@^18.2.0" -> "react"
                // "@types/react@^18.2.0" -> "@types/react"
                // "react@npm:^18.2.0" -> "react"
                // "@scope/pkg@npm:^1.0.0" -> "@scope/pkg"
                let name = if let Some(stripped) = spec.strip_prefix('@') {
                    if let Some(at_idx) = stripped.find('@') {
                        format!("@{}", &stripped[..at_idx])
                    } else {
                        format!("@{}", stripped)
                    }
                } else if let Some(at_idx) = spec.find('@') {
                    spec[..at_idx].to_string()
                } else {
                    spec.to_string()
                };

                if !name.is_empty() && !current_packages.contains(&name) {
                    current_packages.push(name);
                }
            }
            continue;
        }

        // Inside block: look for version
        if (line.starts_with(' ') || line.starts_with('\t'))
            && (trimmed.starts_with("version:") || trimmed.starts_with("version "))
        {
            let raw_ver = if let Some(rest) = trimmed.strip_prefix("version:") {
                rest
            } else if let Some(rest) = trimmed.strip_prefix("version ") {
                rest
            } else {
                ""
            };
            let ver = raw_ver
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();

            if !ver.is_empty() && !current_packages.is_empty() {
                for pkg_name in &current_packages {
                    data.locked_versions
                        .entry(pkg_name.clone())
                        .or_default()
                        .push(ver.clone());
                    data.locked_packages
                        .entry(pkg_name.clone())
                        .or_insert_with(|| ver.clone());
                }
            }
        }
    }

    data
}

/// Parses bun.lock (text) or JSON-based bun lockfile.
pub fn parse_bun_lock(content: &str) -> LockfileData {
    if let Ok(data) = parse_npm_lock(content) {
        if !data.locked_packages.is_empty() {
            return data;
        }
    }
    parse_yarn_lock(content)
}

/// Scans workspaces defined in package.json or pnpm-workspace.yaml to discover local packages.
pub fn discover_workspace_packages(
    root: &Path,
    pkg_json: &serde_json::Value,
) -> HashMap<String, String> {
    let mut packages = HashMap::new();

    // 1. Check "workspaces" in package.json
    let mut globs: Vec<String> = Vec::new();
    if let Some(ws) = pkg_json.get("workspaces") {
        if let Some(arr) = ws.as_array() {
            for item in arr {
                if let Some(s) = item.as_str() {
                    globs.push(s.to_string());
                }
            }
        } else if let Some(obj) = ws.as_object() {
            if let Some(arr) = obj.get("packages").and_then(|p| p.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        globs.push(s.to_string());
                    }
                }
            }
        }
    }

    // 2. Check pnpm-workspace.yaml
    let pnpm_ws = root.join("pnpm-workspace.yaml");
    if pnpm_ws.is_file() {
        if let Ok(content) = fs::read_to_string(&pnpm_ws) {
            if let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                if let Some(arr) = yaml.get("packages").and_then(|p| p.as_sequence()) {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            if !globs.contains(&s.to_string()) {
                                globs.push(s.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Scan matching directories
    for pattern in globs {
        let pattern_clean = pattern.trim_end_matches('/').replace('\\', "/");
        if pattern_clean.ends_with("/*") {
            let parent_dir = &pattern_clean[..pattern_clean.len() - 2];
            let target_parent = root.join(parent_dir);
            if let Ok(entries) = fs::read_dir(target_parent) {
                for entry in entries.flatten() {
                    let sub_pkg = entry.path().join("package.json");
                    if sub_pkg.is_file() {
                        if let Ok(c) = fs::read_to_string(&sub_pkg) {
                            if let Ok(sub_json) = serde_json::from_str::<serde_json::Value>(&c) {
                                if let (Some(n), Some(v)) = (
                                    sub_json.get("name").and_then(|x| x.as_str()),
                                    sub_json.get("version").and_then(|x| x.as_str()),
                                ) {
                                    packages.insert(n.to_string(), v.to_string());
                                }
                            }
                        }
                    }
                }
            }
        } else {
            let target_pkg = root.join(&pattern_clean).join("package.json");
            if target_pkg.is_file() {
                if let Ok(c) = fs::read_to_string(&target_pkg) {
                    if let Ok(sub_json) = serde_json::from_str::<serde_json::Value>(&c) {
                        if let (Some(n), Some(v)) = (
                            sub_json.get("name").and_then(|x| x.as_str()),
                            sub_json.get("version").and_then(|x| x.as_str()),
                        ) {
                            packages.insert(n.to_string(), v.to_string());
                        }
                    }
                }
            }
        }
    }

    packages
}

/// Checks whether a locked version satisfies a manifest version constraint in npm/Node ecosystems.
pub fn npm_version_satisfies(constraint: &str, locked_version: &str) -> bool {
    let c = constraint.trim();
    let l = locked_version.trim();

    if c.is_empty() || c == "*" || c == "latest" || c == "next" || c == "beta" {
        return true;
    }

    // Workspace protocols
    if let Some(rest) = c.strip_prefix("workspace:") {
        let rest_trim = rest.trim();
        if rest_trim == "*" || rest_trim == "^" || rest_trim == "~" || rest_trim.is_empty() {
            return true;
        }
        return npm_version_satisfies(rest_trim, locked_version);
    }

    // Local file / link / portal / git protocols
    if c.starts_with("file:")
        || c.starts_with("link:")
        || c.starts_with("portal:")
        || c.starts_with("git+")
        || c.starts_with("git://")
        || c.starts_with("github:")
        || c.starts_with("http://")
        || c.starts_with("https://")
    {
        return true;
    }

    // npm alias: "npm:foo@^1.0.0"
    if let Some(alias_spec) = c.strip_prefix("npm:") {
        if let Some(at_idx) = alias_spec.rfind('@') {
            let actual_constraint = &alias_spec[at_idx + 1..];
            return npm_version_satisfies(actual_constraint, locked_version);
        }
    }

    // Exact string match (e.g. "1.2.3")
    if c == l {
        return true;
    }

    let parsed_locked = match Version::parse(l) {
        Ok(v) => v,
        Err(_) => {
            // Strip any build metadata or extra parts
            let clean_ver = l.split('-').next().unwrap_or(l);
            match Version::parse(clean_ver) {
                Ok(v) => v,
                Err(_) => return c == l,
            }
        }
    };

    // Parse exact pin like "18.2.0"
    if let Ok(exact) = Version::parse(c) {
        return parsed_locked == exact;
    }

    // Normalize hyphen range: "1.0.0 - 2.0.0" -> ">=1.0.0, <=2.0.0"
    let req_str = if c.contains(" - ") {
        let parts: Vec<&str> = c.split(" - ").collect();
        if parts.len() == 2 {
            format!(">={}, <={}", parts[0].trim(), parts[1].trim())
        } else {
            c.to_string()
        }
    } else {
        // Normalize space-separated ranges: ">=1.0.0 <2.0.0" -> ">=1.0.0, <2.0.0"
        c.split_whitespace().collect::<Vec<_>>().join(", ")
    };

    if let Ok(req) = VersionReq::parse(&req_str) {
        return req.matches(&parsed_locked);
    }

    // Fallback: check starts_with or simple prefix
    let clean_c = c
        .trim_start_matches('^')
        .trim_start_matches('~')
        .trim_start_matches('=')
        .trim();
    if let (Some(c_major), Some(l_major)) = (clean_c.split('.').next(), l.split('.').next()) {
        if c.starts_with('^') {
            return c_major == l_major;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pkg_name_from_node_modules_path() {
        assert_eq!(
            extract_pkg_name_from_node_modules_path("node_modules/react"),
            Some("react".to_string())
        );
        assert_eq!(
            extract_pkg_name_from_node_modules_path("node_modules/@types/react"),
            Some("@types/react".to_string())
        );
        assert_eq!(
            extract_pkg_name_from_node_modules_path("packages/app/node_modules/@scope/pkg/dist"),
            Some("@scope/pkg".to_string())
        );
        assert_eq!(
            extract_pkg_name_from_node_modules_path("node_modules/foo/node_modules/bar"),
            Some("bar".to_string())
        );
    }

    #[test]
    fn test_parse_npm_lock_v2_v3() {
        let json = r#"{
            "name": "test-app",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "packages": {
                "": {
                    "name": "test-app",
                    "dependencies": {
                        "react": "^18.2.0"
                    }
                },
                "node_modules/react": {
                    "version": "18.2.0"
                },
                "node_modules/@types/react": {
                    "version": "18.2.14"
                },
                "packages/shared-ui": {
                    "name": "@test/shared-ui",
                    "version": "2.0.0"
                },
                "node_modules/@test/shared-ui": {
                    "resolved": "packages/shared-ui",
                    "link": true
                }
            }
        }"#;

        let data = parse_npm_lock(json).unwrap();
        assert_eq!(
            data.locked_packages.get("react").map(|s| s.as_str()),
            Some("18.2.0")
        );
        assert_eq!(
            data.locked_packages.get("@types/react").map(|s| s.as_str()),
            Some("18.2.14")
        );
        assert_eq!(
            data.workspace_packages
                .get("@test/shared-ui")
                .map(|s| s.as_str()),
            Some("2.0.0")
        );
    }

    #[test]
    fn test_parse_pnpm_lock_v6() {
        let yaml = r#"
lockfileVersion: '6.0'

importers:
  .:
    dependencies:
      react:
        specifier: ^18.2.0
        version: 18.2.0
      axios:
        specifier: ^1.6.0
        version: 1.6.0(react@18.2.0)
      "@test/shared-ui":
        specifier: "workspace:*"
        version: link:packages/shared-ui

packages:
  /react@18.2.0:
    resolution: {}
  /axios@1.6.0(react@18.2.0):
    resolution: {}
"#;

        let data = parse_pnpm_lock(yaml).unwrap();
        assert_eq!(
            data.locked_packages.get("react").map(|s| s.as_str()),
            Some("18.2.0")
        );
        assert_eq!(
            data.locked_packages.get("axios").map(|s| s.as_str()),
            Some("1.6.0")
        );
        assert!(data.workspace_packages.contains_key("@test/shared-ui"));
    }

    #[test]
    fn test_parse_yarn_lock() {
        let content = r#"
# THIS IS AN AUTOGENERATED FILE. DO NOT EDIT MANUALLY.
# yarn lockfile v1

"react@^18.2.0", "react@>=18.0.0":
  version "18.2.0"
  resolved "https://registry.yarnpkg.com/react/-/react-18.2.0.tgz"

"@types/react@^18.2.0":
  version "18.2.14"
  resolved "https://registry.yarnpkg.com/@types/react/-/react-18.2.14.tgz"
"#;

        let data = parse_yarn_lock(content);
        assert_eq!(
            data.locked_packages.get("react").map(|s| s.as_str()),
            Some("18.2.0")
        );
        assert_eq!(
            data.locked_packages.get("@types/react").map(|s| s.as_str()),
            Some("18.2.14")
        );
    }

    #[test]
    fn test_npm_version_satisfies() {
        assert!(npm_version_satisfies("^18.2.0", "18.2.0"));
        assert!(npm_version_satisfies("^18.2.0", "18.3.1"));
        assert!(!npm_version_satisfies("^18.2.0", "17.0.2"));
        assert!(!npm_version_satisfies("^18.2.0", "19.0.0"));

        assert!(npm_version_satisfies("~1.2.3", "1.2.4"));
        assert!(!npm_version_satisfies("~1.2.3", "1.3.0"));

        assert!(npm_version_satisfies(">=2.0.0", "2.1.0"));
        assert!(!npm_version_satisfies(">=2.0.0", "1.9.9"));

        assert!(npm_version_satisfies("1.0.0", "1.0.0"));
        assert!(!npm_version_satisfies("1.0.0", "1.0.1"));

        assert!(npm_version_satisfies("workspace:*", "2.5.0"));
        assert!(npm_version_satisfies("workspace:^2.0.0", "2.1.0"));
        assert!(!npm_version_satisfies("workspace:^2.0.0", "1.9.0"));
    }
}
