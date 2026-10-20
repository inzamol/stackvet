use crate::strip_json_comments;
use anyhow::Result;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use stackvet_core::WorkspaceMemberInfo;
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

            let clean_pkg_path = pkg_path.replace('\\', "/");
            let clean_pkg_path = clean_pkg_path.strip_prefix("./").unwrap_or(&clean_pkg_path);
            if clean_pkg_path.contains("node_modules/") {
                if let Some(name) = extract_pkg_name_from_node_modules_path(clean_pkg_path) {
                    if let Some(ver) = pkg_info.get("version").and_then(|v| v.as_str()) {
                        let is_top_level = clean_pkg_path.starts_with("node_modules/")
                            && !clean_pkg_path["node_modules/".len()..].contains("/node_modules/");
                        data.locked_versions
                            .entry(name.clone())
                            .or_default()
                            .push(ver.to_string());
                        if is_top_level {
                            data.locked_packages.insert(name.clone(), ver.to_string());
                        } else {
                            data.locked_packages
                                .entry(name.clone())
                                .or_insert_with(|| ver.to_string());
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

/// Recursively collects legacy npm dependency versions without replacing primary versions.
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

/// Parses bun.lock (Bun 1.2+ JSONC format directly).
pub fn parse_bun_lock(content: &str) -> LockfileData {
    let mut data = LockfileData::default();
    let cleaned = strip_json_comments(content);
    let val: serde_json::Value = match serde_json::from_str(&cleaned) {
        Ok(v) => v,
        Err(_) => return data,
    };

    if let Some(packages) = val.get("packages").and_then(|p| p.as_object()) {
        for (key, val) in packages {
            if key.is_empty() {
                // Root package entry
                continue;
            }

            // In Bun, packages map entry is typically an array where element 0 is "name@version"
            // e.g. "react": ["react@18.2.0", ...]
            // or "packages/ui": ["@myorg/ui@workspace:packages/ui", ...]
            let specifier_opt = if let Some(arr) = val.as_array() {
                arr.first().and_then(|v| v.as_str())
            } else if let Some(s) = val.as_str() {
                Some(s)
            } else if let Some(obj) = val.as_object() {
                obj.get("version").and_then(|v| v.as_str())
            } else {
                None
            };

            let Some(specifier) = specifier_opt else {
                continue;
            };

            // Parse specifier: "name@version" or "@scope/pkg@version"
            let (name, ver) = if let Some(stripped) = specifier.strip_prefix('@') {
                if let Some(at_idx) = stripped.find('@') {
                    (format!("@{}", &stripped[..at_idx]), &stripped[at_idx + 1..])
                } else {
                    continue;
                }
            } else if let Some(at_idx) = specifier.find('@') {
                (specifier[..at_idx].to_string(), &specifier[at_idx + 1..])
            } else {
                (key.clone(), specifier)
            };

            if ver.starts_with("workspace:") || ver.starts_with("link:") {
                let ws_clean = ver.strip_prefix("workspace:").unwrap_or(ver).trim();
                let recorded_ver = if ws_clean.is_empty() || ws_clean == "*" || ws_clean == "." {
                    "0.0.0".to_string()
                } else {
                    ws_clean.to_string()
                };
                data.workspace_packages.insert(name.clone(), recorded_ver);
            } else {
                let clean_ver = clean_pnpm_version(ver);
                data.locked_versions
                    .entry(name.clone())
                    .or_default()
                    .push(clean_ver.clone());
                data.locked_packages.insert(name, clean_ver);
            }
        }
    }

    data
}

fn extract_deps_map(val: Option<&serde_json::Value>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(obj) = val.and_then(|v| v.as_object()) {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                map.insert(k.clone(), s.to_string());
            }
        }
    }
    map
}

/// Discovers workspace members across package.json, pnpm-workspace.yaml, lerna.json, bun, and nested directories.
pub fn discover_workspace_members(
    root: &Path,
    pkg_json: Option<&serde_json::Value>,
) -> Vec<WorkspaceMemberInfo> {
    let mut members = Vec::new();
    let mut discovered_manifests = std::collections::HashSet::new();

    // 1. Collect glob patterns
    let mut globs: Vec<String> = Vec::new();

    // Check package.json workspaces
    if let Some(pkg) = pkg_json {
        if let Some(ws) = pkg.get("workspaces") {
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
    }

    // Check pnpm-workspace.yaml
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

    // Check lerna.json
    let lerna_json = root.join("lerna.json");
    if lerna_json.is_file() {
        if let Ok(content) = fs::read_to_string(&lerna_json) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(arr) = val.get("packages").and_then(|p| p.as_array()) {
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

    // Fallback if no explicit workspaces configured: check conventional monorepo folders
    if globs.is_empty() {
        let conventions = ["packages/*", "apps/*", "services/*", "libs/*", "modules/*"];
        for conv in conventions {
            let parent = conv.trim_end_matches("/*");
            if root.join(parent).is_dir() {
                globs.push(conv.to_string());
            }
        }
    }

    let root_pkg_path = root.join("package.json");

    // Helper closure to parse a member package.json
    let mut add_member_from_dir = |dir: &Path| {
        let manifest_path = dir.join("package.json");
        if manifest_path.is_file() && manifest_path != root_pkg_path {
            let canonical = manifest_path
                .canonicalize()
                .unwrap_or_else(|_| manifest_path.clone());
            if !discovered_manifests.insert(canonical) {
                return;
            }
            if let Ok(content) = fs::read_to_string(&manifest_path) {
                let stripped = strip_json_comments(&content);
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&stripped) {
                    let fallback_name = dir
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let name = val
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or(&fallback_name)
                        .to_string();
                    let version = val
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let dependencies = extract_deps_map(val.get("dependencies"));
                    let dev_dependencies = extract_deps_map(val.get("devDependencies"));
                    let peer_dependencies = extract_deps_map(val.get("peerDependencies"));
                    let optional_dependencies = extract_deps_map(val.get("optionalDependencies"));

                    members.push(WorkspaceMemberInfo {
                        name,
                        version,
                        manifest_path,
                        root_dir: dir.to_path_buf(),
                        dependencies,
                        dev_dependencies,
                        peer_dependencies,
                        optional_dependencies,
                    });
                }
            }
        }
    };

    // Scan matching directories for each pattern
    for pattern in globs {
        let pattern_clean = pattern.trim_end_matches('/').replace('\\', "/");
        if pattern_clean.ends_with("/**") {
            let parent_dir = &pattern_clean[..pattern_clean.len() - 3];
            let target_parent = root.join(parent_dir);
            if target_parent.is_dir() {
                let mut stack = vec![(target_parent, 0)];
                while let Some((curr, depth)) = stack.pop() {
                    if depth > 4 {
                        continue;
                    }
                    if let Ok(entries) = fs::read_dir(&curr) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                let name = path.file_name().unwrap_or_default().to_string_lossy();
                                if matches!(
                                    name.as_ref(),
                                    "node_modules"
                                        | ".git"
                                        | "target"
                                        | "dist"
                                        | "build"
                                        | ".venv"
                                        | ".turbo"
                                        | ".next"
                                ) {
                                    continue;
                                }
                                if path.join("package.json").is_file() {
                                    add_member_from_dir(&path);
                                } else {
                                    stack.push((path, depth + 1));
                                }
                            }
                        }
                    }
                }
            }
        } else if pattern_clean.ends_with("/*") {
            let parent_dir = &pattern_clean[..pattern_clean.len() - 2];
            let target_parent = root.join(parent_dir);
            if let Ok(entries) = fs::read_dir(target_parent) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        if !matches!(
                            name.as_ref(),
                            "node_modules" | ".git" | "target" | "dist" | "build"
                        ) {
                            add_member_from_dir(&path);
                        }
                    }
                }
            }
        } else {
            let target_dir = root.join(&pattern_clean);
            if target_dir.is_dir() {
                add_member_from_dir(&target_dir);
            }
        }
    }

    // Sort members deterministically by name and manifest_path
    members.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.manifest_path.cmp(&b.manifest_path))
    });
    members
}

/// Scans workspaces defined in package.json or pnpm-workspace.yaml to discover local packages.
pub fn discover_workspace_packages(
    root: &Path,
    pkg_json: &serde_json::Value,
) -> HashMap<String, String> {
    let members = discover_workspace_members(root, Some(pkg_json));
    let mut packages = HashMap::new();
    for m in members {
        let v = m.version.unwrap_or_else(|| "*".to_string());
        packages.insert(m.name, v);
    }
    packages
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintCompatibility {
    Compatible {
        suggested_resolution: Option<String>,
    },
    Incompatible {
        conflicting_members: Vec<(String, String)>,
        reason: String,
    },
}

/// Extracts candidate versions from a version constraint string to test intersection.
pub fn extract_candidate_versions(constraint: &str) -> Vec<Version> {
    let mut candidates = Vec::new();
    let c = constraint.trim();
    if c.is_empty() || c == "*" || c == "latest" {
        return candidates;
    }

    let clean = c.strip_prefix("workspace:").unwrap_or(c).trim();

    let parts: Vec<&str> = clean
        .split(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .filter(|s| !s.is_empty())
        .collect();

    for p in parts {
        let dots = p.chars().filter(|&c| c == '.').count();
        let normalized = if dots == 2 {
            p.to_string()
        } else if dots == 1 {
            format!("{}.0", p)
        } else if dots == 0 {
            format!("{}.0.0", p)
        } else {
            continue;
        };

        if let Ok(v) = Version::parse(&normalized) {
            if !candidates.contains(&v) {
                candidates.push(v.clone());
            }
            let patch_bump = Version::new(v.major, v.minor, v.patch + 1);
            if !candidates.contains(&patch_bump) {
                candidates.push(patch_bump);
            }
            let minor_bump = Version::new(v.major, v.minor + 1, 0);
            if !candidates.contains(&minor_bump) {
                candidates.push(minor_bump);
            }
        }
    }

    candidates
}

/// Checks whether declared npm version constraints from multiple workspace members are mutually compatible.
pub fn check_npm_constraints_compatibility(
    member_declarations: &[(&str, &str)],
) -> ConstraintCompatibility {
    if member_declarations.len() <= 1 {
        let res = member_declarations.first().map(|(_, c)| c.to_string());
        return ConstraintCompatibility::Compatible {
            suggested_resolution: res,
        };
    }

    // Filter wildcards and normalize workspace prefixes
    let mut non_wildcards: Vec<(&str, String)> = Vec::new();
    for &(member, constraint) in member_declarations {
        let clean = constraint.trim();
        let unworkspaced = clean.strip_prefix("workspace:").unwrap_or(clean).trim();
        if unworkspaced.is_empty()
            || unworkspaced == "*"
            || unworkspaced == "latest"
            || unworkspaced == "x"
            || unworkspaced == "X"
        {
            continue;
        }
        non_wildcards.push((member, unworkspaced.to_string()));
    }

    // If all are wildcards, compatible!
    if non_wildcards.is_empty() {
        return ConstraintCompatibility::Compatible {
            suggested_resolution: Some("*".to_string()),
        };
    }

    if non_wildcards.iter().any(|(_, constraint)| {
        constraint == "^"
            || constraint == "~"
            || constraint.contains("||")
            || [
                "file:", "link:", "portal:", "git+", "git://", "github:", "http://", "https://",
            ]
            .iter()
            .any(|prefix| constraint.starts_with(prefix))
            || extract_candidate_versions(constraint).is_empty()
    }) {
        return ConstraintCompatibility::Compatible {
            suggested_resolution: None,
        };
    }

    // If all non-wildcards are identical
    let first_c = &non_wildcards[0].1;
    if non_wildcards.iter().all(|(_, c)| c == first_c) {
        return ConstraintCompatibility::Compatible {
            suggested_resolution: Some(first_c.clone()),
        };
    }

    // Collect candidate versions from all constraints
    let mut all_candidates: Vec<Version> = Vec::new();
    for (_, c) in &non_wildcards {
        for v in extract_candidate_versions(c) {
            if !all_candidates.contains(&v) {
                all_candidates.push(v);
            }
        }
    }

    // Test each candidate version against all non-wildcard constraints
    let mut satisfying_version: Option<Version> = None;
    for v in &all_candidates {
        let v_str = v.to_string();
        if non_wildcards
            .iter()
            .all(|(_, c)| npm_version_satisfies(c, &v_str))
        {
            satisfying_version = Some(v.clone());
            break;
        }
    }

    if let Some(v) = satisfying_version {
        // Find which member's constraint is tightest while satisfying all other constraints
        let mut best_constraint: Option<String> = None;
        for (_, c) in &non_wildcards {
            let candidates = extract_candidate_versions(c);
            if let Some(base_v) = candidates.first() {
                let is_subset = all_candidates
                    .iter()
                    .filter(|cand| npm_version_satisfies(c, &cand.to_string()))
                    .all(|cand| {
                        non_wildcards
                            .iter()
                            .all(|(_, other)| npm_version_satisfies(other, &cand.to_string()))
                    });
                if is_subset {
                    if let Some(prev) = &best_constraint {
                        let prev_candidates = extract_candidate_versions(prev);
                        if let Some(pb) = prev_candidates.first() {
                            if base_v > pb {
                                best_constraint = Some(c.clone());
                            }
                        }
                    } else {
                        best_constraint = Some(c.clone());
                    }
                }
            }
        }
        let best_constraint = best_constraint.unwrap_or_else(|| format!("^{}", v));

        ConstraintCompatibility::Compatible {
            suggested_resolution: Some(best_constraint),
        }
    } else {
        let conflicting_members = member_declarations
            .iter()
            .map(|&(m, c)| (m.to_string(), c.to_string()))
            .collect();
        let summary = member_declarations
            .iter()
            .map(|(m, c)| format!("'{}' declares '{}'", m, c))
            .collect::<Vec<_>>()
            .join(", ");
        ConstraintCompatibility::Incompatible {
            conflicting_members,
            reason: format!(
                "No compatible version satisfies all declared constraints ({})",
                summary
            ),
        }
    }
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

    /// Verifies package name extraction for scoped, nested, and workspace paths.
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

    /// Verifies npm package entries retain root versions and linked workspace metadata.
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
                },
                "packages/shared-ui/node_modules/react": {
                    "version": "17.0.2"
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
        // Verify locked_versions contains both versions
        let react_versions = data.locked_versions.get("react").unwrap();
        assert!(react_versions.contains(&"18.2.0".to_string()));
        assert!(react_versions.contains(&"17.0.2".to_string()));
    }

    /// Verifies pnpm importer versions, peer suffix removal, and workspace links.
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

    /// Verifies Yarn classic headers resolve regular and scoped package versions.
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

    /// Verifies Bun JSONC parsing accepts comments, trailing commas, and workspace entries.
    #[test]
    fn test_parse_bun_lock() {
        let content = r#"{
            "lockfileVersion": 1,
            // JSONC comments in bun.lock
            "packages": {
                "": ["my-app@workspace:."],
                "react": ["react@18.2.0"],
                "@types/react": ["@types/react@18.2.14"],
                "packages/ui": ["@myorg/ui@workspace:packages/ui"],
            }
        }"#;

        let data = parse_bun_lock(content);
        assert_eq!(
            data.locked_packages.get("react").map(|s| s.as_str()),
            Some("18.2.0")
        );
        assert_eq!(
            data.locked_packages.get("@types/react").map(|s| s.as_str()),
            Some("18.2.14")
        );
        assert!(data.workspace_packages.contains_key("@myorg/ui"));
    }

    /// Verifies npm ranges, exact pins, and workspace constraints against locked versions.
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

    #[test]
    fn test_check_npm_constraints_compatibility() {
        // Compatible caret ranges
        let decls = [("app", "^18.0.0"), ("admin", "^18.2.0")];
        let res = check_npm_constraints_compatibility(&decls);
        assert!(matches!(res, ConstraintCompatibility::Compatible { .. }));
        if let ConstraintCompatibility::Compatible {
            suggested_resolution,
        } = res
        {
            assert_eq!(suggested_resolution, Some("^18.2.0".to_string()));
        }

        // Incompatible caret ranges
        let conflict = [("app", "^18.2.0"), ("admin", "^17.0.2")];
        let res_conflict = check_npm_constraints_compatibility(&conflict);
        assert!(matches!(
            res_conflict,
            ConstraintCompatibility::Incompatible { .. }
        ));

        // Compatible with wildcard
        let with_wildcard = [("app", "*"), ("utils", "^4.17.21")];
        let res_wc = check_npm_constraints_compatibility(&with_wildcard);
        assert!(matches!(res_wc, ConstraintCompatibility::Compatible { .. }));

        // Incompatible exact pins
        let exact_conflict = [("app", "1.0.0"), ("admin", "2.0.0")];
        let res_exact = check_npm_constraints_compatibility(&exact_conflict);
        assert!(matches!(
            res_exact,
            ConstraintCompatibility::Incompatible { .. }
        ));

        // Compatible with git/http URL, protocol prefix, or union
        let with_git = [
            ("app", "git+https://github.com/org/repo.git#v1.0.0"),
            ("admin", "^1.0.0"),
        ];
        let res_git = check_npm_constraints_compatibility(&with_git);
        assert!(matches!(
            res_git,
            ConstraintCompatibility::Compatible {
                suggested_resolution: None
            }
        ));

        let with_union = [("app", "^16.0.0 || ^17.0.0"), ("admin", "^17.0.0")];
        let res_union = check_npm_constraints_compatibility(&with_union);
        assert!(matches!(
            res_union,
            ConstraintCompatibility::Compatible {
                suggested_resolution: None
            }
        ));
    }
}
