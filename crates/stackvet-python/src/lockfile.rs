use crate::normalize_package_name;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use stackvet_core::WorkspaceMemberInfo;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LockfileData {
    pub locked_packages: HashMap<String, String>,
    pub locked_versions: HashMap<String, Vec<String>>,
    pub workspace_packages: HashMap<String, String>,
}

/// Parses uv.lock (TOML format)
pub fn parse_uv_lock(content: &str) -> LockfileData {
    let mut data = LockfileData::default();
    if let Ok(val) = toml::from_str::<toml::Value>(content) {
        if let Some(pkgs) = val.get("package").and_then(|p| p.as_array()) {
            for pkg in pkgs {
                if let (Some(name), Some(version)) = (
                    pkg.get("name").and_then(|n| n.as_str()),
                    pkg.get("version").and_then(|v| v.as_str()),
                ) {
                    let norm_name = normalize_package_name(name);
                    let is_workspace = pkg
                        .get("source")
                        .and_then(|s| s.get("workspace"))
                        .and_then(|w| w.as_bool())
                        .unwrap_or(false)
                        || pkg
                            .get("source")
                            .and_then(|s| s.get("editable"))
                            .and_then(|e| e.as_str())
                            .is_some();

                    if is_workspace {
                        data.workspace_packages
                            .insert(norm_name.clone(), version.to_string());
                    }

                    data.locked_versions
                        .entry(norm_name.clone())
                        .or_default()
                        .push(version.to_string());
                    data.locked_packages.insert(norm_name, version.to_string());
                }
            }
        }
    }
    data
}

/// Parses poetry.lock (TOML format)
pub fn parse_poetry_lock(content: &str) -> LockfileData {
    let mut data = LockfileData::default();
    if let Ok(val) = toml::from_str::<toml::Value>(content) {
        if let Some(pkgs) = val.get("package").and_then(|p| p.as_array()) {
            for pkg in pkgs {
                if let (Some(name), Some(version)) = (
                    pkg.get("name").and_then(|n| n.as_str()),
                    pkg.get("version").and_then(|v| v.as_str()),
                ) {
                    let norm_name = normalize_package_name(name);
                    let is_develop = pkg
                        .get("develop")
                        .and_then(|d| d.as_bool())
                        .unwrap_or(false);

                    if is_develop {
                        data.workspace_packages
                            .insert(norm_name.clone(), version.to_string());
                    }

                    data.locked_versions
                        .entry(norm_name.clone())
                        .or_default()
                        .push(version.to_string());
                    data.locked_packages.insert(norm_name, version.to_string());
                }
            }
        }
    }
    data
}

/// Parses Pipfile.lock (JSON format)
pub fn parse_pipfile_lock(content: &str) -> LockfileData {
    let mut data = LockfileData::default();
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
        for section in ["default", "develop"] {
            if let Some(pkgs) = val.get(section).and_then(|s| s.as_object()) {
                for (name, info) in pkgs {
                    if let Some(ver_str) = info.get("version").and_then(|v| v.as_str()) {
                        let clean_ver = ver_str.trim_start_matches('=').trim();
                        let norm_name = normalize_package_name(name);
                        data.locked_versions
                            .entry(norm_name.clone())
                            .or_default()
                            .push(clean_ver.to_string());
                        data.locked_packages
                            .insert(norm_name, clean_ver.to_string());
                    }
                }
            }
        }
    }
    data
}

/// Compares two version strings numerically dot-by-dot.
pub fn compare_numeric_versions(a: &str, b: &str) -> Ordering {
    let a_clean = a.split(['-', '+']).next().unwrap_or(a);
    let b_clean = b.split(['-', '+']).next().unwrap_or(b);

    let a_nums: Vec<u64> = a_clean
        .split('.')
        .filter_map(|s| s.parse::<u64>().ok())
        .collect();
    let b_nums: Vec<u64> = b_clean
        .split('.')
        .filter_map(|s| s.parse::<u64>().ok())
        .collect();

    let max_len = a_nums.len().max(b_nums.len());
    for i in 0..max_len {
        let ai = a_nums.get(i).copied().unwrap_or(0);
        let bi = b_nums.get(i).copied().unwrap_or(0);
        match ai.cmp(&bi) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// Checks whether a locked version satisfies a Python PEP 440 version requirement.
pub fn pep_version_satisfies(req_str: &str, locked_version: &str) -> bool {
    let r = req_str.trim();
    let l = locked_version.trim();

    if r.is_empty() || r == "*" {
        return true;
    }

    // Comma-separated specifiers (e.g. ">=2.28.0,<3.0.0")
    for part in r.split(',') {
        let spec = part.trim();
        if spec.is_empty() {
            continue;
        }

        if let Some(target) = spec.strip_prefix("==") {
            let target_trim = target.trim();
            if target_trim.ends_with(".*") {
                let prefix = target_trim.trim_end_matches(".*");
                if !l.starts_with(prefix) {
                    return false;
                }
            } else if target_trim != l {
                return false;
            }
        } else if let Some(target) = spec.strip_prefix("!=") {
            let target_trim = target.trim();
            if target_trim == l {
                return false;
            }
        } else if let Some(target) = spec.strip_prefix("~=") {
            let target_trim = target.trim();
            if compare_numeric_versions(l, target_trim) == Ordering::Less {
                return false;
            }
            let t_parts: Vec<&str> = target_trim.split('.').collect();
            let l_parts: Vec<&str> = l.split('.').collect();
            if t_parts.len() >= 3 && l_parts.len() >= 2 {
                if t_parts[0] != l_parts[0] || t_parts[1] != l_parts[1] {
                    return false;
                }
            } else if t_parts.len() == 2 && !l_parts.is_empty() && t_parts[0] != l_parts[0] {
                return false;
            }
        } else if let Some(target) = spec.strip_prefix(">=") {
            let target_trim = target.trim();
            let ord = compare_numeric_versions(l, target_trim);
            if ord == Ordering::Less {
                return false;
            }
        } else if let Some(target) = spec.strip_prefix("<=") {
            let target_trim = target.trim();
            let ord = compare_numeric_versions(l, target_trim);
            if ord == Ordering::Greater {
                return false;
            }
        } else if let Some(target) = spec.strip_prefix('>') {
            let target_trim = target.trim();
            let ord = compare_numeric_versions(l, target_trim);
            if ord != Ordering::Greater {
                return false;
            }
        } else if let Some(target) = spec.strip_prefix('<') {
            let target_trim = target.trim();
            let ord = compare_numeric_versions(l, target_trim);
            if ord != Ordering::Less {
                return false;
            }
        } else if spec.starts_with('^') || spec.starts_with('~') {
            // Poetry style constraint
            if let (Ok(req), Ok(vl)) = (VersionReq::parse(spec), Version::parse(l)) {
                if !req.matches(&vl) {
                    return false;
                }
            } else {
                let clean = spec[1..].trim();
                let ord = compare_numeric_versions(l, clean);
                if ord == Ordering::Less {
                    return false;
                }
            }
        }
    }

    true
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

/// Checks whether declared PEP 440 version constraints across workspace members are compatible.
pub fn check_pep_constraints_compatibility(
    member_declarations: &[(&str, &str)],
) -> ConstraintCompatibility {
    if member_declarations.len() <= 1 {
        let res = member_declarations.first().map(|(_, c)| c.to_string());
        return ConstraintCompatibility::Compatible {
            suggested_resolution: res,
        };
    }

    let mut non_wildcards: Vec<(&str, String)> = Vec::new();
    for &(member, constraint) in member_declarations {
        let clean = constraint.trim();
        if clean.is_empty() || clean == "*" {
            continue;
        }
        non_wildcards.push((member, clean.to_string()));
    }

    if non_wildcards.is_empty() {
        return ConstraintCompatibility::Compatible {
            suggested_resolution: Some("*".to_string()),
        };
    }

    let first_c = &non_wildcards[0].1;
    if non_wildcards.iter().all(|(_, c)| c == first_c) {
        return ConstraintCompatibility::Compatible {
            suggested_resolution: Some(first_c.clone()),
        };
    }

    // Extract candidate versions
    let mut all_candidates: Vec<String> = Vec::new();
    for (_, c) in &non_wildcards {
        for part in c.split(',') {
            let clean_part = part
                .trim_start_matches(|ch: char| !ch.is_ascii_digit())
                .trim();
            if !clean_part.is_empty() && !all_candidates.contains(&clean_part.to_string()) {
                all_candidates.push(clean_part.to_string());
                let nums: Vec<u64> = clean_part
                    .split('.')
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if nums.len() >= 3 {
                    all_candidates.push(format!("{}.{}.{}", nums[0], nums[1], nums[2] + 1));
                    all_candidates.push(format!("{}.{}.0", nums[0], nums[1] + 1));
                } else if nums.len() == 2 {
                    all_candidates.push(format!("{}.{}.0", nums[0], nums[1]));
                    all_candidates.push(format!("{}.{}", nums[0], nums[1] + 1));
                    all_candidates.push(format!("{}.{}.0", nums[0], nums[1] + 1));
                }
            }
        }
    }

    let mut satisfying_version: Option<String> = None;
    for v in &all_candidates {
        if non_wildcards
            .iter()
            .all(|(_, c)| pep_version_satisfies(c, v))
        {
            satisfying_version = Some(v.clone());
            break;
        }
    }

    if let Some(v) = satisfying_version {
        let best_constraint = non_wildcards
            .iter()
            .map(|(_, c)| c.clone())
            .find(|c| {
                let nums: Vec<&str> = c.split(',').collect();
                let base = nums
                    .first()
                    .map(|s| s.trim_start_matches(|ch: char| !ch.is_ascii_digit()).trim())
                    .unwrap_or("");
                !base.is_empty()
                    && non_wildcards
                        .iter()
                        .all(|(_, other)| pep_version_satisfies(other, base))
            })
            .unwrap_or_else(|| format!(">={}", v));

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

/// Discovers Python workspace members defined in pyproject.toml ([tool.uv.workspace] or poetry) or subdirectories.
pub fn discover_python_workspace_members(
    root: &Path,
    pyproject: Option<&toml::Value>,
) -> Vec<WorkspaceMemberInfo> {
    let mut members = Vec::new();
    let mut discovered_manifests = std::collections::HashSet::new();

    let mut globs: Vec<String> = Vec::new();

    if let Some(val) = pyproject {
        // 1. [tool.uv.workspace] members = [...]
        if let Some(uv_members) = val
            .get("tool")
            .and_then(|t| t.get("uv"))
            .and_then(|u| u.get("workspace"))
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
        {
            for m in uv_members {
                if let Some(s) = m.as_str() {
                    globs.push(s.to_string());
                }
            }
        }

        // 2. [tool.poetry.dependencies] local paths
        if let Some(poetry_deps) = val
            .get("tool")
            .and_then(|t| t.get("poetry"))
            .and_then(|p| p.get("dependencies"))
            .and_then(|d| d.as_table())
        {
            for (_, dep_val) in poetry_deps {
                if let Some(path_str) = dep_val.get("path").and_then(|p| p.as_str()) {
                    globs.push(path_str.to_string());
                }
            }
        }
    }

    // Fallback conventions if no explicit workspace config
    if globs.is_empty() {
        let conventions = ["packages/*", "services/*", "apps/*", "libs/*", "modules/*"];
        for conv in conventions {
            let parent = conv.trim_end_matches("/*");
            if root.join(parent).is_dir() {
                globs.push(conv.to_string());
            }
        }
    }

    let root_manifest_path = root.join("pyproject.toml");

    let mut add_member_from_dir = |dir: &Path| {
        let manifest_path = dir.join("pyproject.toml");
        if manifest_path.is_file() && manifest_path != root_manifest_path {
            let canonical = manifest_path
                .canonicalize()
                .unwrap_or_else(|_| manifest_path.clone());
            if !discovered_manifests.insert(canonical) {
                return;
            }
            if let Ok(content) = fs::read_to_string(&manifest_path) {
                if let Ok(val) = toml::from_str::<toml::Value>(&content) {
                    let fallback_name = dir
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let name = val
                        .get("project")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .or_else(|| {
                            val.get("tool")
                                .and_then(|t| t.get("poetry"))
                                .and_then(|p| p.get("name"))
                                .and_then(|n| n.as_str())
                        })
                        .unwrap_or(&fallback_name);
                    let norm_name = normalize_package_name(name);

                    let version = val
                        .get("project")
                        .and_then(|p| p.get("version"))
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            val.get("tool")
                                .and_then(|t| t.get("poetry"))
                                .and_then(|p| p.get("version"))
                                .and_then(|v| v.as_str())
                        })
                        .map(|s| s.to_string());

                    let mut dependencies = HashMap::new();
                    if let Some(deps) = val
                        .get("project")
                        .and_then(|p| p.get("dependencies"))
                        .and_then(|d| d.as_array())
                    {
                        for dep in deps {
                            if let Some(s) = dep.as_str() {
                                if let Some((n, v)) = crate::parse_requirement_spec(s) {
                                    dependencies.insert(n, v);
                                }
                            }
                        }
                    } else if let Some(deps) = val
                        .get("tool")
                        .and_then(|t| t.get("poetry"))
                        .and_then(|p| p.get("dependencies"))
                        .and_then(|d| d.as_table())
                    {
                        for (k, v) in deps {
                            if k != "python" {
                                let ver_str = match v {
                                    toml::Value::String(s) => s.clone(),
                                    toml::Value::Table(t) => t
                                        .get("version")
                                        .and_then(|ver| ver.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    _ => String::new(),
                                };
                                dependencies.insert(normalize_package_name(k), ver_str);
                            }
                        }
                    }

                    let mut dev_dependencies = HashMap::new();
                    if let Some(opt) = val
                        .get("project")
                        .and_then(|p| p.get("optional-dependencies"))
                        .and_then(|o| o.as_table())
                    {
                        for (_, arr) in opt {
                            if let Some(arr) = arr.as_array() {
                                for dep in arr {
                                    if let Some(s) = dep.as_str() {
                                        if let Some((n, v)) = crate::parse_requirement_spec(s) {
                                            dev_dependencies.insert(n, v);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    members.push(WorkspaceMemberInfo {
                        name: norm_name,
                        version,
                        manifest_path,
                        root_dir: dir.to_path_buf(),
                        dependencies,
                        dev_dependencies,
                        peer_dependencies: HashMap::new(),
                        optional_dependencies: HashMap::new(),
                    });
                }
            }
        }
    };

    for pattern in globs {
        let pattern_clean = pattern.trim_end_matches('/').replace('\\', "/");
        if pattern_clean.ends_with("/*") {
            let parent_dir = &pattern_clean[..pattern_clean.len() - 2];
            let target_parent = root.join(parent_dir);
            if let Ok(entries) = fs::read_dir(target_parent) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        if !matches!(
                            name.as_ref(),
                            ".venv" | "venv" | "__pycache__" | ".git" | "target" | "dist" | "build"
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

    members.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.manifest_path.cmp(&b.manifest_path))
    });
    members
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies uv package versions and normalized workspace package names are retained.
    #[test]
    fn test_parse_uv_lock() {
        let content = r#"
version = 1
revision = 1

[[package]]
name = "requests"
version = "2.28.2"

[[package]]
name = "pydantic"
version = "2.6.0"

[[package]]
name = "local_pkg"
version = "0.1.0"
source = { workspace = true }
"#;

        let data = parse_uv_lock(content);
        assert_eq!(
            data.locked_packages.get("requests").map(|s| s.as_str()),
            Some("2.28.2")
        );
        assert_eq!(
            data.locked_packages.get("pydantic").map(|s| s.as_str()),
            Some("2.6.0")
        );
        assert_eq!(
            data.workspace_packages.get("local-pkg").map(|s| s.as_str()),
            Some("0.1.0")
        );
    }

    /// Verifies Poetry versions and development workspace packages are retained.
    #[test]
    fn test_parse_poetry_lock() {
        let content = r#"
[[package]]
name = "fastapi"
version = "0.110.0"

[[package]]
name = "shared_lib"
version = "1.0.0"
develop = true
"#;

        let data = parse_poetry_lock(content);
        assert_eq!(
            data.locked_packages.get("fastapi").map(|s| s.as_str()),
            Some("0.110.0")
        );
        assert_eq!(
            data.workspace_packages
                .get("shared-lib")
                .map(|s| s.as_str()),
            Some("1.0.0")
        );
    }

    /// Verifies Python bounds, exact pins, and compatible release constraints.
    #[test]
    fn test_pep_version_satisfies() {
        assert!(pep_version_satisfies(">=2.28.0", "2.28.2"));
        assert!(pep_version_satisfies(">=2.28.0", "2.28.0"));
        assert!(!pep_version_satisfies(">=2.28.0", "2.27.9"));

        assert!(pep_version_satisfies(">=2.28.0, <3.0.0", "2.31.0"));
        assert!(!pep_version_satisfies(">=2.28.0, <3.0.0", "3.0.1"));

        assert!(pep_version_satisfies("==0.100.0", "0.100.0"));
        assert!(!pep_version_satisfies("==0.100.0", "0.100.1"));

        assert!(pep_version_satisfies("~=1.4.5", "1.4.9"));
        assert!(!pep_version_satisfies("~=1.4.5", "1.5.0"));

        assert!(pep_version_satisfies("^1.2.0", "1.3.0"));
        assert!(!pep_version_satisfies("^1.2.0", "2.0.0"));
    }

    #[test]
    fn test_check_pep_constraints_compatibility() {
        // Compatible ranges
        let decls = [("api", ">=2.0.0,<3.0.0"), ("worker", ">=2.5.0,<3.0.0")];
        let res = check_pep_constraints_compatibility(&decls);
        assert!(matches!(res, ConstraintCompatibility::Compatible { .. }));

        // Incompatible ranges
        let conflict = [("api", ">=2.0.0,<3.0.0"), ("worker", ">=1.10.0,<2.0.0")];
        let res_conflict = check_pep_constraints_compatibility(&conflict);
        assert!(matches!(
            res_conflict,
            ConstraintCompatibility::Incompatible { .. }
        ));

        // Compatible with wildcard
        let with_wc = [("api", "*"), ("worker", ">=1.0.0")];
        let res_wc = check_pep_constraints_compatibility(&with_wc);
        assert!(matches!(res_wc, ConstraintCompatibility::Compatible { .. }));

        // Compatible with two-part versions and padded candidates
        let two_part = [("api", ">=2.25"), ("worker", ">=2.25.0,<3.0.0")];
        let res_two_part = check_pep_constraints_compatibility(&two_part);
        assert!(matches!(
            res_two_part,
            ConstraintCompatibility::Compatible { .. }
        ));
    }
}
