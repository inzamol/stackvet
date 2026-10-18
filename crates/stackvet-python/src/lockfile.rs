use crate::normalize_package_name;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

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
}
