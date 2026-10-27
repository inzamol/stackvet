use anyhow::Result;
use stackvet_core::{Analyzer, Confidence, Diagnostic, LanguageTarget, ProjectContext, Severity};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::vuln_cache::{VulnCacheEntry, VulnCacheManager, VulnCacheStatus, VulnDatabaseCache};

pub struct SecurityAnalyzer {
    pub offline: bool,
    pub refresh_cache: bool,
    pub custom_cache_path: Option<PathBuf>,
    pub custom_osv_url: Option<String>,
}

impl SecurityAnalyzer {
    pub fn new(offline: bool, refresh_cache: bool) -> Self {
        Self {
            offline,
            refresh_cache,
            custom_cache_path: None,
            custom_osv_url: None,
        }
    }

    pub fn with_cache_path(offline: bool, refresh_cache: bool, cache_path: PathBuf) -> Self {
        Self {
            offline,
            refresh_cache,
            custom_cache_path: Some(cache_path),
            custom_osv_url: None,
        }
    }

    pub fn with_endpoint(
        offline: bool,
        refresh_cache: bool,
        cache_path: PathBuf,
        osv_url: impl Into<String>,
    ) -> Self {
        Self {
            offline,
            refresh_cache,
            custom_cache_path: Some(cache_path),
            custom_osv_url: Some(osv_url.into()),
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
        let mut cache_corrupted = false;
        let mut cache_opt = match mgr.load_cache() {
            Ok(opt) => opt,
            Err(err) => {
                tracing::warn!(
                    "Vulnerability database cache at {:?} is corrupted or unreadable: {err}. Treating cache as missing.",
                    mgr.cache_path()
                );
                cache_corrupted = true;
                None
            }
        };

        let mut refresh_failure_err: Option<String> = None;

        // If refresh requested or if cache is missing and not offline, fetch fresh advisories from OSV
        if (self.refresh_cache || cache_opt.is_none()) && !self.offline {
            match fetch_advisories_from_osv(&packages_to_audit, self.custom_osv_url.as_deref()) {
                Ok(new_entries) => {
                    let mut cache =
                        cache_opt.unwrap_or_else(|| VulnDatabaseCache::new(mgr.ttl_seconds()));
                    for entry in new_entries {
                        let norm_pkg =
                            crate::vuln_cache::normalize_pkg_name(&entry.package, &entry.ecosystem);
                        let norm_eco = entry.ecosystem.to_ascii_lowercase();
                        cache.entries.retain(|e| {
                            !(e.ecosystem.to_ascii_lowercase() == norm_eco
                                && crate::vuln_cache::normalize_pkg_name(&e.package, &e.ecosystem)
                                    == norm_pkg
                                && e.version == entry.version)
                        });
                        cache.entries.push(entry);
                    }
                    cache.last_updated = crate::vuln_cache::current_timestamp();
                    if let Err(e) = mgr.save_cache(&cache) {
                        tracing::error!(
                            "Failed to save vulnerability cache to {:?}: {e}",
                            mgr.cache_path()
                        );
                        eprintln!("Warning: Failed to save vulnerability cache: {e}");
                    }
                    cache_opt = Some(cache);
                }
                Err(err) => {
                    tracing::warn!("Failed to refresh vulnerability cache from OSV.dev: {err}");
                    refresh_failure_err = Some(err.to_string());
                    // Do NOT bump last_updated if network fetch failed
                }
            }
        }

        if let Some(ref err) = refresh_failure_err {
            if cache_opt.is_some() && self.refresh_cache {
                let primary_manifest = packages_to_audit[0].3.clone();
                diagnostics.push(
                    Diagnostic::builder("SEC-003", Severity::Warning, LanguageTarget::General)
                        .message(format!(
                            "Vulnerability advisory database refresh failed: {err}. Falling back to existing local cache."
                        ))
                        .file(&primary_manifest)
                        .explanation(
                            "A refresh of the vulnerability advisory database was requested or attempted, but failed due to a network or service error. StackVet will continue using cached advisory data, which may be outdated.",
                        )
                        .remediation(
                            "Check your internet connection, verify proxy settings, or run without '--refresh-vuln-cache' to use cached advisories.",
                        )
                        .confidence(Confidence::High)
                        .build(),
                );
            }
        }

        let status = mgr.status(cache_opt.as_ref());

        match status {
            VulnCacheStatus::Missing => {
                let primary_manifest = packages_to_audit[0].3.clone();
                let msg = if cache_corrupted {
                    format!(
                        "Vulnerability checks skipped: local vulnerability cache at {:?} is corrupted or invalid",
                        mgr.cache_path()
                    )
                } else if self.offline {
                    "Vulnerability checks skipped: offline mode is active and no local vulnerability cache was found at .stackvet/cache/vulnerabilities.json".to_string()
                } else if let Some(ref err) = refresh_failure_err {
                    format!(
                        "The vulnerability advisory database (OSV.dev) is unavailable and no local cache was found: {err}"
                    )
                } else {
                    "The vulnerability advisory database (OSV.dev) is unavailable and no local cache was found".to_string()
                };

                let explanation = if cache_corrupted {
                    "The vulnerability database cache file contains invalid JSON or an incompatible schema. Vulnerability scanning cannot be performed with corrupted advisory data."
                } else if self.offline {
                    "Offline mode was specified, but no local vulnerability cache is available. Vulnerability checks cannot be performed."
                } else {
                    "StackVet requires a vulnerability database to detect known CVEs in project dependencies. Without a connection or local cache, vulnerability scanning cannot be performed."
                };

                let remediation = if cache_corrupted {
                    "Run 'stackvet scan --refresh-vuln-cache' to overwrite and repair the local advisory database cache, or delete the corrupted cache file."
                } else if self.offline {
                    "Disable offline mode or run 'stackvet scan --refresh-vuln-cache' while connected to the internet to pre-populate the vulnerability database."
                } else {
                    "Check your internet connection, verify firewall or proxy settings, and run 'stackvet scan --refresh-vuln-cache' to populate the local vulnerability database."
                };

                diagnostics.push(
                    Diagnostic::builder("SEC-001", Severity::Warning, LanguageTarget::General)
                        .message(msg)
                        .file(&primary_manifest)
                        .explanation(explanation)
                        .remediation(remediation)
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

#[derive(Debug, serde::Deserialize)]
struct OsvQueryResponse {
    #[serde(default)]
    vulns: Vec<OsvVulnDetail>,
}

#[derive(Debug, serde::Deserialize)]
struct OsvVulnDetail {
    id: String,
    summary: Option<String>,
    details: Option<String>,
    #[serde(default)]
    database_specific: Option<OsvDbSpecific>,
    #[serde(default)]
    affected: Vec<OsvAffectedPkg>,
}

#[derive(Debug, serde::Deserialize)]
struct OsvDbSpecific {
    severity: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct OsvAffectedPkg {
    #[serde(default)]
    ranges: Vec<OsvRangeEvent>,
}

#[derive(Debug, serde::Deserialize)]
struct OsvRangeEvent {
    #[serde(default)]
    events: Vec<OsvEventDetail>,
}

#[derive(Debug, serde::Deserialize)]
struct OsvEventDetail {
    fixed: Option<String>,
}

fn run_async_future<F, T>(future: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(future)),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(future)
        }
    }
}

async fn fetch_advisories_from_osv_async(
    packages: &[(String, String, String, PathBuf)],
    custom_url: Option<&str>,
) -> Result<Vec<VulnCacheEntry>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("StackVet-CLI")
        .build()?;

    let mut entries = Vec::new();
    let mut any_network_error = false;
    let mut any_success = false;
    let mut last_error_msg = None;

    for (pkg, eco, ver, _) in packages {
        let payload = serde_json::json!({
            "package": {
                "name": pkg,
                "ecosystem": eco,
            },
            "version": ver,
        });

        let default_url = "https://api.osv.dev/v1/query";
        let osv_endpoint = custom_url.unwrap_or(default_url);

        let resp = match client.post(osv_endpoint).json(&payload).send().await {
            Ok(r) => {
                any_success = true;
                r
            }
            Err(e) => {
                tracing::warn!("Failed to query OSV for {pkg}@{ver}: {e}");
                any_network_error = true;
                last_error_msg = Some(e.to_string());
                continue;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            tracing::warn!("OSV query for {pkg}@{ver} returned HTTP status {status}");
            any_network_error = true;
            last_error_msg = Some(format!("HTTP status {status}"));
            continue;
        }

        let data: OsvQueryResponse = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to parse OSV response for {pkg}@{ver}: {e}");
                continue;
            }
        };

        let mut vulns = Vec::new();
        for v in data.vulns {
            let summary = v
                .summary
                .or(v.details)
                .unwrap_or_else(|| "Security advisory".to_string());
            let severity = v
                .database_specific
                .and_then(|d| d.severity)
                .unwrap_or_else(|| "HIGH".to_string());
            let fixed_in = v
                .affected
                .iter()
                .flat_map(|a| &a.ranges)
                .flat_map(|r| &r.events)
                .find_map(|e| e.fixed.clone());

            vulns.push(crate::vuln_cache::CachedVulnerability {
                id: v.id,
                summary,
                severity,
                fixed_in,
            });
        }

        entries.push(VulnCacheEntry {
            package: pkg.clone(),
            ecosystem: eco.clone(),
            version: ver.clone(),
            vulnerabilities: vulns,
        });
    }

    if any_network_error && !any_success {
        if let Some(err) = last_error_msg {
            anyhow::bail!("Network error connecting to OSV.dev advisory database: {err}");
        } else {
            anyhow::bail!("Network error: could not connect to OSV.dev advisory database");
        }
    }

    Ok(entries)
}

fn fetch_advisories_from_osv(
    packages: &[(String, String, String, PathBuf)],
    custom_url: Option<&str>,
) -> Result<Vec<VulnCacheEntry>> {
    run_async_future(fetch_advisories_from_osv_async(packages, custom_url))
}
