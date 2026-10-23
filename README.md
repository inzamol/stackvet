<p align="center">
  <img src="docs/assets/logo.svg" alt="StackVet Logo" width="96" height="96" />
</p>

# StackVet (`stackvet`)

> **Universal Project Health & Static Diagnostics Engine**  
> *Rigorously examine, inspect, and evaluate modern polyglot application stacks for health, safety, concurrency hazards, and architectural soundness in milliseconds.*

[![Crates.io](https://img.shields.io/badge/crates.io-v0.1.0-orange.svg)](https://crates.io)
[![PyPI](https://img.shields.io/badge/pypi-stackvet-blue.svg)](https://pypi.org)
[![npm](https://img.shields.io/badge/npm-stackvet-red.svg)](https://npmjs.com)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

---

## Table of Contents

- [Overview](#overview)
- [Key Features](#key-features)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Terminal UI & Interactive Scorecard](#terminal-ui--interactive-scorecard)
- [CLI Command Guide](#cli-command-guide)
  - [`stackvet doctor` (aliases: `diagnose`, `health`)](#stackvet-doctor)
  - [`stackvet check` (aliases: `scan`, `audit`)](#stackvet-check)
  - [`stackvet inspect` (aliases: `examine`, `details`)](#stackvet-inspect)
  - [`stackvet explain`](#stackvet-explain)
  - [`stackvet fix` (aliases: `remediate`, `auto-fix`)](#stackvet-fix)
  - [`stackvet graph` (aliases: `architecture`, `dependencies`)](#stackvet-graph)
  - [`stackvet baseline`](#stackvet-baseline)
  - [`stackvet init` (alias: `setup`)](#stackvet-init)
  - [`stackvet rules` (aliases: `catalog`, `list-rules`)](#stackvet-rules)
- [Diagnostic Rules Catalog (132 Rules)](#diagnostic-rules-catalog)
  - [Python Dependency Rules (`DEP-xxx`)](#python-dependency-rules-dep-xxx)
  - [Node Dependency Rules (`NDEP-xxx`)](#node-dependency-rules-ndep-xxx)
  - [Import & Architecture Rules (`IMP-xxx`, `ARC-xxx`)](#import--architecture-rules-imp-xxx-arc-xxx)
  - [Environment & Drift Rules (`ENV-xxx`)](#environment--drift-rules-env-xxx)
  - [Docker Rules (`DOC-xxx`)](#docker-rules-doc-xxx)
  - [FastAPI Framework Rules (`FAP-xxx`)](#fastapi-framework-rules-fap-xxx)
  - [SQLAlchemy Rules (`SQL-xxx`)](#sqlalchemy-rules-sql-xxx)
  - [Django Rules (`DJG-xxx`)](#django-rules-djg-xxx)
  - [Celery Rules (`CEL-xxx`)](#celery-rules-cel-xxx)
  - [React & Next.js Rules (`RCT-xxx`, `NXT-xxx`)](#react--nextjs-rules-rct-xxx-nxt-xxx)
- [Output Formats](#output-formats)
- [Process Exit Codes](#process-exit-codes)
- [Project Configuration (`stackvet.toml`)](#project-configuration)
- [CI/CD Integration](#cicd-integration)
- [Publishing to npm & PyPI](#publishing-to-npm--pypi)
- [Architecture & Workspace Structure](#architecture--workspace-structure)
- [Contributing](#contributing)
- [License](#license)

---

## Overview

Modern software applications are polyglot by nature: Python backends (FastAPI, Django), Node/TypeScript frontends (React, Next.js), Docker containers, and CI/CD pipelines.

When applications break, developers often suffer from **alert fatigue**—facing dozens of disconnected warnings across different linters that fail to identify the single underlying root cause.

**StackVet** bridges this gap:
1. **Parallel Static Analysis**: Uses Rust with `ruff_python_parser` and `oxc_parser` to inspect ASTs without running user code.
2. **Cross-Layer Drift Detection**: Detects mismatches between local environments, package manifests, lockfiles, Docker images, and CI matrix definitions.
3. **Correlation Engine**: Groups individual symptoms into concise **Root Cause Clusters**.
4. **Health Score & Grade**: Produces an actionable 0–100 health score with letter grades (A through F).
5. **Format-Preserving Auto-Fixes**: Injects missing dependencies into `pyproject.toml` or `package.json` preserving user comments and indentation using `toml_edit`.

---

## Key Features

- **Blazing Fast**: Native Rust engine with Rayon parallel processing; analyzes multi-thousand-file repositories in milliseconds.
- **AST-Level Accuracy**: Direct AST traversal eliminating regex false positives.
- **Circular Dependency Detection**: Detects import cycles using Tarjan's Strongly Connected Components (SCC) algorithm.
- **Zero-Config Discovery**: Automatically discovers `pyproject.toml`, `requirements.txt`, `uv.lock`, `package.json`, `pnpm-lock.yaml`, Dockerfiles, and GitHub workflows.
- **Enterprise Reporting**: Outputs rich terminal tables, machine-readable JSON, OASIS SARIF v2.1.0 for GitHub Code Scanning, and standalone responsive dark-mode HTML reports.

---

## Installation

### Via Cargo (Rust)
```bash
cargo install stackvet
```

### Via uv / pip (Python)
```bash
# Run instantly with uvx (no install required):
uvx stackvet doctor

# Or install with pip / pipx:
pip install stackvet
# or
pipx install stackvet
```

### Via npm / npx (Node.js)
```bash
# Run instantly with npx (no install required):
npx @inzamol/stackvet doctor

# Or install in your project:
npm install --save-dev @inzamol/stackvet
# or with pnpm / yarn:
pnpm add -D @inzamol/stackvet
yarn add -D @inzamol/stackvet
```

---

## Quick Start

Run a full project health check in your repository root:

```bash
stackvet doctor
```

```text
╭─────────────────────────────────────────────────────────────────────╮
│              StackVet Universal Project Health Report               │
╰─────────────────────────────────────────────────────────────────────╯

  Health Score:  [████████████████████] 100 / 100   [ Grade: A (Healthy) ]
  Overall Grade: A (Healthy)
  Calculation:   Base 100 - 0 deductions (0 unique findings, 0 duplicates ignored)

  Metrics:       ● 0 Critical  │  ● 0 Errors  │  ▲ 0 Warnings  │  ℹ 0 Info

No issues found! Your project stack is pristine.
```

---

## Terminal UI & Interactive Scorecard

StackVet features an enterprise-grade terminal user interface designed with adaptive formatting, UTF-8 rounded box-drawing characters, ANSI color-coded severity badges, and visual health score meters.

### Executive Diagnostic Report

When violations, runtime drift, or missing dependencies are discovered across your stack, `stackvet doctor` renders an interactive diagnostic dashboard:

```text
╭─────────────────────────────────────────────────────────────────────╮
│              StackVet Universal Project Health Report               │
╰─────────────────────────────────────────────────────────────────────╯

  Health Score:  [█████████████████░░░] 86 / 100   [ Grade: B (Fair) ]
  Overall Grade: B (Fair)
  Calculation:   Base 100 - 14 deductions (4 unique findings, 0 duplicates ignored)

  Metrics:       ● 0 Critical  │  ● 1 Errors  │  ▲ 3 Warnings  │  ℹ 0 Info

─── Health Score Deduction Breakdown ──────────────────────────────────
  • [ERROR]    NDEP-001   -8 pts     (1 finding)  ── Missing Node Package
  • [WARN]     IMP-003    -2 pts     (1 finding)  ── Unused / orphan local module
  • [WARN]     NDEP-004   -2 pts     (1 finding)  ── Lockfile Out of Sync
  • [WARN]     SEC-001    -2 pts     (1 finding)  ── Security analysis unavailable

─── Root Cause Clusters (4) ───────────────────────────────────────────
  1. [ERROR] Missing Project Dependency: 'axios' [Verified]  (1 finding)
     Summary:      Package 'axios' is imported across 1 file location(s) but undeclared in manifests.
     Remediation:  Add 'axios' to project manifest dependencies.
     Action Plan:
       1. Add 'axios' to project manifest dependencies.
       2. Synchronize package manager lockfile.

  2. [WARN] Environment, Lockfile & Runtime Drift [Verified]  (1 finding)
     Summary:      Detected 1 discrepancy(ies) between local manifests, Docker, CI matrix, or lockfiles.
     Remediation:  Align runtime version declarations across pyproject, package.json, Dockerfile, and CI workflows.
     Action Plan:
       1. Synchronize dependency declarations and lockfile versions.
       2. Align target runtime specifications across Dockerfiles, CI matrices, and project manifests.

  3. [WARN] Rule Violation: IMP-003 [Verified]  (1 finding)
     Summary:      File is never imported by any other source files in the project. It may be dead code or an unexported utility.
     Remediation:  Import the module or delete it if obsolete.
     Action Plan:
       1. Import the module or delete it if obsolete.

─── Diagnostic Details ────────────────────────────────────────────────
╭──────────┬──────────┬─────────┬───────────────────────────────────┬────────────────────────────────────────────────╮
│ Rule     ┆ Severity ┆ Target  ┆ File Location                     ┆ Message                                        │
╞══════════╪══════════╪═════════╪═══════════════════════════════════╪════════════════════════════════════════════════╡
│ NDEP-001 ┆ ERROR    ┆ Node.js ┆ server.js:2:15                    ┆ Package 'axios' is imported in source code     │
│          ┆          ┆         ┆                                   ┆ but not declared in package.json               │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ IMP-003  ┆ WARN     ┆ General ┆ server.js                         ┆ Orphan source file with 0 incoming imports:    │
│          ┆          ┆         ┆                                   ┆ 'server.js'                                    │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ NDEP-004 ┆ WARN     ┆ Node.js ┆ package.json                      ┆ No lockfile detected (pnpm-lock.yaml /         │
│          ┆          ┆         ┆                                   ┆ package-lock.json / yarn.lock)                 │
╰──────────┴──────────┴─────────┴───────────────────────────────────┴────────────────────────────────────────────────╯
```

### Auto-Remediation Verification Summary (`stackvet fix`)

When executing auto-remediation, StackVet provides safe format-preserving modifications with a transactional verification scorecard and rollback checkpoints:

```text
╭────────────────────────────────────────────────────────────────────────────────────────────────╮
│                                    Fix Verification Summary                                    │
╞════════════════════════════════════════════════════════════════════════════════════════════════╡
│ Status:          ✓ Verified Fixed                                                              │
│ Initial Score:   86 / 100 (Grade: B)                                                           │
│ Post-Fix Score:  100 / 100 (Grade: A)                                                          │
│ Files Modified:  package.json (1 dependency added, comments & formatting preserved)            │
│ Rollback:        Snapshot saved to .stackvet/rollbacks/20261023-112638-fix.json                │
╰────────────────────────────────────────────────────────────────────────────────────────────────╯
```

---

## CLI Command Guide

All subcommands support positional path arguments (e.g. `stackvet doctor ./backend`) as well as the global `-p / --path` option.

### `stackvet doctor` (aliases: `diagnose`, `health`)
Runs a comprehensive health check, aggregates symptoms into root causes, and assigns a health grade (A–F).

```bash
# Run on current directory
stackvet doctor

# Run on a target directory (positional or -p)
stackvet doctor ./backend
stackvet -p ./services/api doctor

# Using intuitive aliases
stackvet health ./backend
stackvet diagnose

# Fail CI with exit code 1 if health score is below 85
stackvet doctor --min-score 85

# Explain which diagnostics contributed to health score deductions
stackvet doctor --explain

# Compare against an established baseline
stackvet doctor --baseline .stackvet-baseline.json

# Output in standalone HTML report
stackvet --format html doctor > health-report.html
```

#### Health Scoring Weights & Grading Scale
StackVet computes an objective, deterministic 0–100 health score subtracted from a base of 100:

| Severity | Deduction | Description |
|---|---|---|
| **`CRITICAL`** | **-20 pts** | Blocking synchronous calls in async handlers, critical event loop freezes |
| **`ERROR`** | **-8 pts** | Missing dependencies, circular imports, unresolved modules |
| **`WARN`** | **-2 pts** | Lockfile drift, unused dependencies, version mismatches |
| **`INFO` / `HINT`** | **0 pts** | Informational architecture advice and guidance |

- **Letter Grades**: **A (90–100)** | **B (75–89)** | **C (50–74)** | **F (0–49)**
- **Deduplication Safety**: Duplicate findings across identical files and lines are suppressed so redundant occurrences never distort the score.


---

### `stackvet check` (aliases: `scan`, `audit`)
A fast gate optimized for pre-commit hooks and CI pipelines. Returns exit code `0` if clean, or `1` if errors are detected.

```bash
# Normal check (fails only on ERROR and CRITICAL)
stackvet check ./backend
stackvet scan ./backend

# Custom severity failure threshold (e.g. fail on warning or higher)
stackvet check --fail-on warning

# Strict mode (equivalent to --fail-on warning)
stackvet check --strict
stackvet audit --strict

# Baseline-aware check: tolerates legacy issues, fails only on new regressions
stackvet check --baseline .stackvet-baseline.json --fail-on error

# Update baseline to record newly accepted findings
stackvet check --baseline .stackvet-baseline.json --update-baseline

# Output machine-readable JSON in CI
stackvet --format json check
```

---

### `stackvet inspect` (aliases: `examine`, `details`)
Deep diagnostics inspection displaying code locations, snippets, severity, and targeted remediation guides.

```bash
# Inspect all detected diagnostics
stackvet inspect ./backend
stackvet examine ./backend

# Filter diagnostics by rule category prefix:
stackvet inspect --rule FAP   # Only FastAPI rules
stackvet inspect --rule SQL   # Only SQLAlchemy rules
stackvet inspect --rule DOC   # Only Docker rules
stackvet inspect --rule RCT   # Only React rules
```

---

### `stackvet explain`
Displays in-depth documentation, potential runtime consequences, code examples, and remediation steps for any diagnostic rule code.

```bash
# Explain any rule in detail
stackvet explain FAP-001
stackvet explain SQL-004
stackvet explain DOC-001
```

---

### `stackvet fix` (aliases: `remediate`, `auto-fix`)
Applies safe, format-preserving automated remediations (e.g. injecting missing packages into `pyproject.toml` or `package.json`).

```bash
# Preview available safe fixes
stackvet fix
stackvet remediate

# Apply all fixes automatically without prompts
stackvet fix --yes
stackvet auto-fix --yes
```

---

### `stackvet graph` (aliases: `architecture`, `dependencies`)
Visualizes project module dependencies and detects circular import cycles.

```bash
# Clean ASCII hierarchy
stackvet graph --style ascii
stackvet architecture

# Mermaid diagram (pasteable directly into GitHub Markdown or Notion)
stackvet graph --style mermaid

# Graphviz DOT format
stackvet graph --style dot
```

---

### `stackvet baseline`
Records existing issues in legacy codebases to prevent CI failures on technical debt while blocking new regressions.

```bash
# Record current diagnostics into baseline (.stackvet-baseline.json)
stackvet baseline record ./backend

# Custom output filepath
stackvet baseline record -o ./ci/baseline.json

# Verify in CI that no new regressions were introduced
stackvet baseline verify ./backend

# Verify failing only on errors (tolerating new warnings)
stackvet baseline verify ./backend --fail-on error

# Strict verification (fails on new warnings too)
stackvet baseline verify ./backend --strict

# Accept resolved debt and new findings by updating the baseline file
stackvet baseline verify ./backend --update
```

---

### `stackvet init` (alias: `setup`)
Bootstraps a recommended `.stackvet.toml` configuration and optional GitHub Actions CI workflow.

```bash
# Generate .stackvet.toml
stackvet init

# Generate configuration AND GitHub Actions CI workflow (.github/workflows/stackvet.yml)
stackvet init --ci
```

---

### `stackvet rules` (aliases: `catalog`, `list-rules`)
Displays the complete catalog of 132 supported diagnostic rules across Python, Node.js, Docker, CI/CD, and frameworks.

```bash
# Show all rules in a formatted table
stackvet rules

# Filter catalog by ecosystem or framework
stackvet rules docker
stackvet rules fastapi
stackvet rules sqlalchemy
stackvet rules react
```

---

## Diagnostic Rules Catalog

### Python Dependency Rules (`DEP-xxx`)

| Rule ID | Severity | Name | Description |
|---|---|---|---|
| `DEP-001` | `ERROR` | Version Conflict | Detects conflicting version specifiers in dependencies (e.g. `foo>=2.0` vs `foo<2.0`). |
| `DEP-002` | `ERROR` | Missing Declaration | Imported package is used in Python code but not declared in `pyproject.toml` or `requirements.txt`. |
| `DEP-003` | `WARN` | Unused Dependency | Package listed in dependencies is never imported by any Python source files. |
| `DEP-004` | `ERROR` | Python Version Incompatible | Package requirements target end-of-life Python runtimes (e.g. `<3.10`). |
| `DEP-005` | `WARN` | Lockfile Drift | Manifest file exists (`pyproject.toml`) but lockfile (`uv.lock`/`poetry.lock`) is missing. |
| `DEP-006` | `CRITICAL` | Vulnerability Audit | Batched security audit against OSV database for known CVEs. |
| `DEP-007` | `WARN` | Transitive Dependency Import | Package imported is present in lockfile but not directly declared in `pyproject.toml`. |

---

### Node Dependency Rules (`NDEP-xxx`)

| Rule ID | Severity | Name | Description |
|---|---|---|---|
| `NDEP-001` | `ERROR` | Missing Node Package | Package imported in `.ts`/`.tsx`/`.js` is missing from `package.json`. |
| `NDEP-002` | `WARN` | Unused Node Package | Production dependency in `package.json` is never imported in source files. |
| `NDEP-003` | `ERROR` | Unmet Peer Dependency | Declared dependencies conflict with peer dependency constraints. |
| `NDEP-004` | `WARN` | Lockfile Out of Sync | Missing lockfile (`pnpm-lock.yaml`, `package-lock.json`, `yarn.lock`). |
| `NDEP-005` | `ERROR` | DevDependency in Production | A package from `devDependencies` is imported inside production source files (outside test files). |

---

### Import & Architecture Rules (`IMP-xxx`, `ARC-xxx`)

| Rule ID | Severity | Name | Description |
|---|---|---|---|
| `IMP-001` | `ERROR` | Circular Imports | Import cycle detected across modules (e.g. `a -> b -> c -> a`). |
| `IMP-002` | `ERROR` | Unresolved Local Import | Relative import path (`./utils`, `from .mod`) has no corresponding file on disk. |
| `IMP-003` | `WARN` | Orphan File / Dead Code | Source file has 0 incoming imports and is not an application entrypoint. |
| `IMP-004` | `ERROR` | PEP 594 Removed Stdlib | Importing deprecated standard library module removed in Python 3.13 (`cgi`, `pipes`, `imp`, `crypt`, etc.). |
| `ARC-001` | `WARN` | Layer Violation | Lower-tier persistence layer directly imports upper-tier presentation or API layers. |
| `ARC-002` | `WARN` | Component Cycle | Cross-boundary cycles detected between top-level feature directories. |

---

### Environment & Drift Rules (`ENV-xxx`)

| Rule ID | Severity | Name | Description |
|---|---|---|---|
| `ENV-001` | `WARN` | Runtime Drift | `.python-version` or `.nvmrc` contradicts the runtime constraint in `pyproject.toml` or `package.json`. |
| `ENV-002` | `ERROR` | Docker Drift | `Dockerfile` base image (e.g. `python:3.10-slim`) contradicts project requirement (`>=3.12`). |
| `ENV-003` | `ERROR` | CI Matrix Drift | GitHub Actions workflow matrix runs tests on unsupported runtime versions. |

---

### FastAPI Framework Rules (`FAP-xxx`)

| Rule ID | Severity | Name | Description |
|---|---|---|---|
| `FAP-001` | `CRITICAL` | Blocking Call in Async Route | Calling blocking functions (`time.sleep()`, synchronous `requests.*`, `subprocess.*`) inside `async def` routes. |
| `FAP-003` | `WARN` | Missing Response Model | FastAPI route handler lacks a `response_model` argument or return type annotation (`-> Model`). |
| `FAP-004` | `CRITICAL` | Insecure Wildcard CORS | `CORSMiddleware` configured with `allow_origins=["*"]` combined with `allow_credentials=True`. |
| `FAP-006` | `ERROR` | Missing Path Parameter | URL template contains a placeholder (e.g. `/{item_id}`) not present in handler arguments. |
| `FAP-008` | `WARN` | Deprecated on_event | Using `@app.on_event("startup")` instead of modern `lifespan` context managers. |
| `FAP-010` | `ERROR` | Duplicate Route Path | Multiple handlers registered for the exact same HTTP method and path template. |
| `FAP-011` | `ERROR` | Mutable Default Argument | Route parameter has a mutable default value (e.g. `def list_items(tags: list = []):`). |
| `FAP-015` | `ERROR` | Sync File I/O in Async Handler | Synchronous `open()` or `with open(...)` executed inside an `async def` route handler. |

---

### React & Next.js Rules (`RCT-xxx`, `NXT-xxx`)

| Rule ID | Severity | Name | Description |
|---|---|---|---|
| `RCT-001` | `WARN` | Missing Hook Dependencies | `useEffect` or `useCallback` missing referenced variables in dependency array. |
| `RCT-002` | `ERROR` | Conditional Hook Call | React hook (`useState`, `useEffect`) invoked conditionally inside an `if` block. |
| `RCT-003` | `ERROR` | Direct State Mutation | Modifying state directly (`state.foo = 1`) rather than calling state dispatcher. |
| `RCT-004` | `WARN` | Missing Key in List | JSX elements rendered inside `.map()` missing a unique `key` prop. |
| `RCT-005` | `WARN` | Barrel Import Bloat | Importing from root barrel exports (e.g. `from '@mui/material'`) causing bundle bloat. |
| `NXT-001` | `ERROR` | Server Component Client API | Browser APIs (`window`, `localStorage`) or client hooks used without `"use client"` directive. |
| `NXT-002` | `CRITICAL` | Client Component Private Secret | Client Component accesses private environment variable (not prefixed with `NEXT_PUBLIC_`). |

---

## Output Formats

StackVet supports 4 built-in egress formats via `--format`:

| Format | Description | Command Example |
|---|---|---|
| `text` (default) | Colored ANSI terminal output with tables and scorecard | `stackvet doctor` |
| `json` | Structured JSON with full diagnostics and root causes | `stackvet --format json check` |
| `sarif` | OASIS SARIF v2.1.0 for GitHub Code Scanning tab | `stackvet --format sarif doctor > results.sarif` |
| `html` | Standalone responsive dark-mode HTML report | `stackvet --format html doctor > report.html` |

---

## Process Exit Codes

StackVet returns stable, predictable exit codes across all commands, checks, and output formats (`text`, `json`, `sarif`, `html`):

| Exit Code | Classification | Trigger Condition |
|---|---|---|
| `0` | **Success** | Clean project, no findings meeting/exceeding `--fail-on` threshold, health score meets `--min-score`, or all findings are tolerated legacy baseline issues. |
| `1` | **Findings Violation** | Findings detected meeting or exceeding `--fail-on` threshold, health score below `--min-score`, or new regression findings introduced against baseline. |
| `2` | **Configuration Error** | Configuration syntax error in `stackvet.toml` or `pyproject.toml`, unknown rule ID in `[rules]`, or unsupported severity override level. |
| `3` | **Execution / I/O Error** | Target path does not exist, specified baseline file not found, or filesystem read/write error. |

---

## Project Configuration

StackVet automatically discovers and loads configuration from `stackvet.toml`, `.stackvet.toml`, or `pyproject.toml` (under `[tool.stackvet]`), or from an explicit file via `--config <PATH>`.

### Precedence Hierarchy
1. **CLI Arguments** (`--fail-on`, `--strict`, `--min-score`, `--baseline`) — Highest precedence.
2. **Project Config File** (`stackvet.toml` / `.stackvet.toml` / `pyproject.toml`) — Overrides defaults.
3. **Built-in Defaults** (`fail_on = "error"`, `min_score = None`) — Fallback.

### Example `stackvet.toml`

```toml
[project]
name = "my-application"
min_score = 80
strict = false

[thresholds]
# Minimum severity that triggers CI failure: "hint", "info", "warning", "error", "critical"
fail_on = "error"
# Minimum health score required (0-100)
min_score = 80

[ci]
baseline = ".stackvet-baseline.json"
fail_on = "error"
strict = false

[rules]
# Override rule severities or disable rules entirely
DEP-001 = "warning"
CEL-001 = "disabled"   # or "off"
ARC-001 = "error"

# Detailed table syntax with explicit enable/disable flag
[rules.FAP-001]
severity = "error"
enabled = true

# Ignore patterns (file/directory globs relative to project root)
[ignore]
patterns = ["legacy/**", "benchmarks/**", "*.generated.ts"]

# Configured suppressions with rule, path pattern, and expiration date
[[suppressions]]
rule = "DEP-001"
path = "legacy/**/*.py"
reason = "Legacy package scheduled for removal"

[[suppressions]]
rule = "SQL-001"
until = "2026-12-31"   # ISO 8601 YYYY-MM-DD expiration
reason = "Temporary bypass during query optimization"
```

Inline comment suppressions are also supported in Python, JavaScript, and TypeScript files:
```python
# Inline comment suppression examples
time.sleep(1)  # stackvet:ignore[FAP-001]: intentional delay
# stackvet:ignore-next-line[DEP-001]
import optional_pkg
# stackvet:ignore-file[DJG-001]: legacy file exempt from django check
```

---

## CI/CD Integration

### GitHub Actions (with SARIF Security Tab)

```yaml
name: StackVet Health Check

on:
  push:
    branches: [main]
  pull_request:

jobs:
  health:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install StackVet
        run: cargo install stackvet

      # Fast CI Gate (Fails on Error)
      - name: Run StackVet Check
        run: stackvet check --strict

      # Generate SARIF report for GitHub Code Scanning
      - name: Generate SARIF Report
        if: always()
        run: stackvet --format sarif doctor > stackvet-results.sarif

      - name: Upload SARIF to GitHub Security Tab
        uses: github/codeql-action/upload-sarif@v3
        if: always()
        with:
          sarif_file: stackvet-results.sarif
```

### Pre-Commit Hook (`.pre-commit-config.yaml`)

```yaml
repos:
  - repo: local
    hooks:
      - id: stackvet
        name: stackvet check
        entry: stackvet check
        language: system
        pass_filenames: false
```

---

## Publishing to npm & PyPI

StackVet includes dedicated automated GitHub Actions workflows and local build scripts for publishing to npm and PyPI:

- **npm Workflow**: [`.github/workflows/publish-npm.yml`](.github/workflows/publish-npm.yml)
- **PyPI Workflow**: [`.github/workflows/publish-pypi.yml`](.github/workflows/publish-pypi.yml)
- **Detailed Step-by-Step Guide**: See the complete [Publishing Guide](docs/publishing_guide.md) for prerequisites, token setup, and verification.

```bash
# Build PyPI distribution (wheel & sdist)
npm run build:pypi

# Build npm package & tarball
npm run build:npm
```

---

## Architecture & Workspace Structure

StackVet is designed as a modular Cargo workspace:

```text
stackvet/
├── Cargo.toml                      # Workspace root
├── crates/
│   ├── stackvet-core/             # Core models: Diagnostic, Rule, Severity, ProjectContext, RootCause
│   ├── stackvet-scanner/          # Discovery engine with ignore::WalkBuilder
│   ├── stackvet-python/           # Python AST (ruff_python_parser), manifests & lockfiles
│   ├── stackvet-node/             # Node/TS AST (oxc_parser), package.json & builtins
│   ├── stackvet-graph/            # Tarjan's SCC cycle detection (petgraph)
│   ├── stackvet-analyzers/        # Core analyzers: deps, env, imports, drift
│   ├── stackvet-frameworks/       # Framework engines: FastAPI, React, Next.js
│   ├── stackvet-correlation/      # Root cause clustering and health scoring (0-100)
│   ├── stackvet-remediation/      # Format-preserving auto-fixes (toml_edit)
│   ├── stackvet-reporter/         # Terminal, SARIF v2.1.0, JSON, HTML reporters
│   └── stackvet-cli/              # CLI binary entrypoint (clap)
```

---

## Contributing

1. Clone repository:
   ```bash
   git clone https://github.com/stackvet/stackvet.git
   cd stackvet
   ```

2. Run test suite:
   ```bash
   cargo test
   ```

3. Run locally against sample code:
   ```bash
   cargo run -p stackvet -- doctor
   ```

Pull requests and rule proposals are welcome!

---

## License

StackVet is licensed under the [MIT License](LICENSE).
