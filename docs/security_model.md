# StackVet Security & Trust Model

## 1. Overview & Security Philosophy
StackVet is designed to operate safely on **untrusted repositories**, including open-source pull requests, third-party codebases, and continuous integration pipelines. StackVet adheres to the principle of least privilege, strict defense in depth, and zero arbitrary code execution.

---

## 2. Security Trust Boundaries

```
┌────────────────────────────────────────────────────────┐
│                   Untrusted Input                      │
│  - Repository file trees                               │
│  - Manifests (pyproject.toml, package.json, lockfiles) │
│  - Git diff hunks & commit metadata                    │
│  - Symlinks & file paths                               │
└──────────────────────────┬─────────────────────────────┘
                           │ Strict Boundary Enforcement
┌──────────────────────────▼─────────────────────────────┐
│                 StackVet Core Engine                   │
│  - Safe AST-only parsers (ruff-python, oxc)            │
│  - Canonical root boundary containment                 │
│  - Resource limits (size, depth, file count)           │
│  - Format-preserving atomic remediation                │
└──────────────────────────┬─────────────────────────────┘
                           │ Sanitized Diagnostics & Exit Codes
┌──────────────────────────▼─────────────────────────────┐
│                   Verified Outputs                     │
│  - SARIF v2.1.0 / JSON / Text reports                  │
│  - Standardized exit codes (0, 1, 2, 3, 4)             │
│  - Zero secret or token leakage in error messages      │
└────────────────────────────────────────────────────────┘
```

---

## 3. Core Security Guarantees

### 3.1 Zero Arbitrary Code Execution
- **Static Analysis Only**: StackVet never invokes Python interpreters (`python`, `python3`), Node runtimes (`node`), package managers (`pip`, `npm`, `pnpm`), or shell subprocesses during repository analysis.
- **Pure Rust AST Parsers**: All code parsing is performed using memory-safe Rust implementations:
  - Python AST: `ruff_python_parser` and `ruff_python_ast`
  - JavaScript / TypeScript AST: `oxc_parser` and `oxc_ast`
  - TOML / JSON / YAML: `toml`, `toml_edit`, `serde_json`, and `serde_yaml`
- Execution of arbitrary code or untrusted configuration scripts is impossible by architecture.

### 3.2 Canonical Repository Boundary Enforcement & Symlink Containment
- **Path Traversal Defense**: All file paths derived from git diffs, command-line arguments, or repository file trees are validated against the canonical repository root:
  ```rust
  canonical_candidate.starts_with(&canonical_root)
  ```
  Any path containing path traversal attempts (such as `../../etc/passwd` or outside absolute paths) is rejected and excluded from scanning.
- **Symlink Protection**:
  - By default, symlinks pointing outside the repository root or cyclical symlinks are ignored.
  - Skipped symlinks are recorded in scan results and surfaced as diagnostic `DISC-005` ("Scan resource limit or repository boundary exceeded").

### 3.3 Resource Exhaustion & Denial of Service Protection
To prevent algorithmic complexity attacks or memory exhaustion on malicious or oversized repositories, StackVet enforces configurable resource limits:

| Limit | Default | Configuration (`stackvet.toml`) | Diagnostic Emitted |
| :--- | :--- | :--- | :--- |
| **Max File Size** | 10 MB (`10,485,760` bytes) | `[scanner] max_file_size` | `DISC-005` |
| **Max Traversal Depth** | 40 directory levels | `[scanner] max_depth` | Stopped silently |
| **Max Scanned Files** | 50,000 files | `[scanner] max_files` | `DISC-005` (`is_truncated`) |
| **Symlink Following** | `false` | `[scanner] follow_symlinks` | `DISC-005` |

Files exceeding the file size limit are excluded from in-memory AST generation to prevent OOM panics.

### 3.4 Secret Leakage Prevention in Diagnostic Messages
- Parser errors from malformed manifests or syntax errors (`DISC-001` through `DISC-004`) only disclose file location, line/column coordinates, and concise parser state descriptions.
- File contents, raw token streams, or potential credentials (e.g. `SECRET_KEY = "..."`) are never echoed into diagnostic messages or reports.

---

## 4. Incomplete Analysis vs. Clean Scans

When scanning untrusted repositories, an analysis may be partial if:
1. Manifests are corrupted or unreadable (`DISC-001`, `DISC-004`).
2. Source files contain invalid syntax (`DISC-002`, `DISC-003`).
3. Repository boundaries or resource limits are exceeded (`DISC-005`).

To prevent false confidence where an incomplete analysis reports zero security findings:
- `ReportSummary` tracks `is_complete: bool` and `incomplete_reasons: Vec<String>`.
- In CI pipelines, `--fail-on-incomplete` or `scanner.fail_on_incomplete = true` causes StackVet to exit with **Exit Code 4** (`IncompleteAnalysis`).

---

## 5. Standard Process Exit Codes

| Code | Name | Description |
| :---: | :--- | :--- |
| `0` | `Success` | Command succeeded; no findings at or above configured thresholds. |
| `1` | `FindingsViolation` | Findings detected that violate configured severity thresholds or baseline regressions. |
| `2` | `ConfigurationError` | Invalid configuration syntax, unrecognized rule ID, or unsupported severity override. |
| `3` | `ExecutionError` | Operational I/O error (e.g. missing target path or unreadable baseline file). |
| `4` | `IncompleteAnalysis` | Analysis could not be fully completed due to malformed files, unreadable files, or resource limit exhaustion when `--fail-on-incomplete` is requested. |
