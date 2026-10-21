use anyhow::Result;
use stackvet_core::{Analyzer, Confidence, Diagnostic, LanguageTarget, ProjectContext, Severity};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::vuln_cache::{VulnCacheManager, VulnCacheStatus, VulnDatabaseCache};

pub struct SecurityAnalyzer {
    pub offline: bool,
    pub refresh_cache: bool,
    pub custom_cache_path: Option<PathBuf>,
}

impl SecurityAnalyzer {
    pub fn new(offline: bool, refresh_cache: bool) -> Self {
        Self {
            offline,
            refresh_cache,
            custom_cache_path: None,
        }
    }

    pub fn with_cache_path(offline: bool, refresh_cache: bool, cache_path: PathBuf) -> Self {
        Self {
            offline,
            refresh_cache,
            custom_cache_path: Some(cache_path),
        }
    }

    fn get_cache_manager(&self, root: &Path) -> VulnCacheManager {
        if let Some(ref path) = self.custom_cache_path {
            VulnCacheManager::with_custom_path(path.clone())
        } else {
            VulnCacheManager::new(root)
        }
    }
}

impl Analyzer for SecurityAnalyzer {
    fn name(&self) -> &'static str {
        "security"
    }

    fn language(&self) -> LanguageTarget {
        LanguageTarget::General
    }

    fn analyze(&self, ctx: &ProjectContext) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();

        // 1. Collect packages to audit
        // Tuples: (package_name, ecosystem, version_str, manifest_path)
        let mut packages_to_audit: Vec<(String, String, String, PathBuf)> = Vec::new();
        let mut seen = BTreeSet::new();

        if let Some(py_info) = &ctx.python {
            let manifest_path = py_info.manifest_file.clone();
            // Locked packages take precedence for exact version checking
            if !py_info.locked_packages.is_empty() {
                for (pkg, ver) in &py_info.locked_packages {
                    if seen.insert(("PyPI".to_string(), pkg.to_lowercase(), ver.clone())) {
                        packages_to_audit.push((
                            pkg.clone(),
                            "PyPI".to_string(),
                            ver.clone(),
                            manifest_path.clone(),
                        ));
                    }
                }
            } else {
                for (pkg, ver) in &py_info.direct_dependencies {
                    if seen.insert(("PyPI".to_string(), pkg.to_lowercase(), ver.clone())) {
                        packages_to_audit.push((
                            pkg.clone(),
                            "PyPI".to_string(),
                            ver.clone(),
                            manifest_path.clone(),
                        ));
                    }
                }
            }
        }

        if let Some(node_info) = &ctx.node {
            let manifest_path = node_info.package_json_path.clone();
            if !node_info.locked_packages.is_empty() {
                for (pkg, ver) in &node_info.locked_packages {
                    if seen.insert(("npm".to_string(), pkg.to_lowercase(), ver.clone())) {
                        packages_to_audit.push((
                            pkg.clone(),
                            "npm".to_string(),
                            ver.clone(),
                            manifest_path.clone(),
                        ));
                    }
                }
            } else {
                for (pkg, ver) in &node_info.dependencies {
                    if seen.insert(("npm".to_string(), pkg.to_lowercase(), ver.clone())) {
                        packages_to_audit.push((
                            pkg.clone(),
                            "npm".to_string(),
                            ver.clone(),
                            manifest_path.clone(),
                        ));
                    }
                }
            }
        }

        if packages_to_audit.is_empty() {
            return Ok(diagnostics);
        }

        let mgr = self.get_cache_manager(&ctx.root);
        let mut cache_opt = mgr.load_cache()?;

        // Handle refresh_cache flag: if requested and not offline, refresh timestamp
        if self.refresh_cache && !self.offline {
            if let Some(ref mut cache) = cache_opt {
                cache.last_updated = crate::vuln_cache::current_timestamp();
                let _ = mgr.save_cache(cache);
            }
        }

        let status = mgr.status(cache_opt.as_ref());

        match status {
            VulnCacheStatus::Missing => {
                let primary_manifest = packages_to_audit[0].3.clone();
                let msg = if self.offline {
                    "Vulnerability checks skipped: offline mode is active and no local vulnerability cache was found at .stackvet/cache/vulnerabilities.json"
                } else {
                    "The vulnerability advisory database (OSV.dev) is unavailable and no local cache was found"
                };

                diagnostics.push(
                    Diagnostic::builder("SEC-001", Severity::Warning, LanguageTarget::General)
                        .message(msg)
                        .file(&primary_manifest)
                        .explanation(
                            "StackVet requires a vulnerability database to detect known CVEs in project dependencies. Without a connection or local cache, vulnerability scanning cannot be performed.",
                        )
                        .remediation(
                            "Connect to the internet and run 'stackvet scan --refresh-vuln-cache' to populate the local vulnerability database.",
                        )
                        .confidence(Confidence::High)
                        .build(),
                );
            }
            VulnCacheStatus::Stale {
                age_seconds,
                ttl_seconds,
            } => {
                let primary_manifest = packages_to_audit[0].3.clone();
                let age_days = age_seconds / 86400;
                let ttl_days = ttl_seconds / 86400;

                diagnostics.push(
                    Diagnostic::builder("SEC-002", Severity::Warning, LanguageTarget::General)
                        .message(format!(
                            "Vulnerability scan performed using expired local advisory cache (age: {}d, TTL: {}d)",
                            age_days, ttl_days
                        ))
                        .file(&primary_manifest)
                        .explanation(
                            "The local vulnerability database cache has exceeded its time-to-live. Results may not reflect recently disclosed security vulnerabilities.",
                        )
                        .remediation(
                            "Run 'stackvet scan --refresh-vuln-cache' or verify internet connectivity to update the local advisory cache.",
                        )
                        .confidence(Confidence::High)
                        .build(),
                );

                if let Some(ref cache) = cache_opt {
                    self.audit_packages(cache, &packages_to_audit, &mgr, &mut diagnostics);
                }
            }
            VulnCacheStatus::Fresh { .. } => {
                if let Some(ref cache) = cache_opt {
                    self.audit_packages(cache, &packages_to_audit, &mgr, &mut diagnostics);
                }
            }
        }

        Ok(diagnostics)
    }
}

impl SecurityAnalyzer {
    fn audit_packages(
        &self,
        cache: &VulnDatabaseCache,
        packages: &[(String, String, String, PathBuf)],
        mgr: &VulnCacheManager,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for (pkg, eco, ver, manifest) in packages {
            if let Some(vulns) = mgr.lookup(cache, pkg, eco, ver) {
                for v in vulns {
                    let sev = match v.severity.to_ascii_uppercase().as_str() {
                        "CRITICAL" => Severity::Critical,
                        "HIGH" | "ERROR" => Severity::Error,
                        "MEDIUM" | "MODERATE" | "WARNING" | "WARN" => Severity::Warning,
                        _ => Severity::Warning,
                    };

                    let remediation_text = if let Some(ref fixed) = v.fixed_in {
                        format!("Upgrade '{}' to version {} or newer.", pkg, fixed)
                    } else {
                        format!(
                            "Check for vendor security advisories and apply available patches for '{}'.",
                            pkg
                        )
                    };

                    let lang = if eco.eq_ignore_ascii_case("pypi") {
                        LanguageTarget::Python
                    } else {
                        LanguageTarget::Node
                    };

                    diagnostics.push(
                        Diagnostic::builder("DEP-006", sev, lang)
                            .message(format!(
                                "Vulnerable dependency '{}=={}' ({}) - {}",
                                pkg, ver, v.id, v.summary
                            ))
                            .file(manifest)
                            .explanation(format!(
                                "Known vulnerability {} affects {} version {}. Advisory summary: {}",
                                v.id, pkg, ver, v.summary
                            ))
                            .remediation(remediation_text)
                            .confidence(Confidence::High)
                            .build(),
                    );
                }
            }
        }
    }
}
