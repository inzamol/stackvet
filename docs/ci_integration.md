# CI/CD Integration

`stackvet` is designed to seamlessly run in Continuous Integration workflows and upload code scanning alerts.

---

## 1. Official GitHub Action (`inzamol/stackvet`)

Use the official composite GitHub Action to run `stackvet` with automated PR annotations, SARIF reports, and step summaries:

```yaml
name: Python Project Health Check

on: [push, pull_request]

permissions:
  contents: read
  security-events: write

jobs:
  stackvet-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Run stackvet Health Scan
        uses: inzamol/stackvet@v1
        with:
          format: sarif
          output: stackvet-results.sarif
          fail-on: error

      - name: Upload SARIF to GitHub Code Scanning
        uses: github/codeql-action/upload-sarif@v4
        if: always()
        with:
          sarif_file: stackvet-results.sarif
          category: stackvet
```

### 1.1 Action Inputs & Configuration

| Input | Description | Default |
|---|---|---|
| `path` | Path to the Python project directory to analyze | `.` |
| `version` | Version of `stackvet` to use (pinned version like `1.0.1`, `latest`, or `local`) | `1.0.1` |
| `python-version` | Python version used to run stackvet (`3.10`, `3.11`, `3.12`, `3.13`) | `3.12` |
| `format` | Output report format (`sarif`, `terminal`, `json`, `html`, `text`) | `sarif` |
| `output` | Output file path for generated report | `stackvet-results.sarif` |
| `fail-on` | Finding severity causing workflow failure (`error`, `warning`, `none`) | `error` |
| `github-annotations` | Emit GitHub Actions inline annotations (`::error`, `::warning`) | `true` |
| `offline` | Disable remote vulnerability queries (airgapped mode) | `false` |
| `baseline` | Path to baseline snapshot to ignore existing findings | `""` |
| `args` | Additional CLI arguments to pass to `stackvet scan` | `""` |

> **Tip (Reproducible Builds)**: For deterministic production workflows, pin the GitHub Action with a full commit SHA in `uses:` (e.g. `uses: inzamol/stackvet@<commit-sha>`) and pin the `stackvet` package using the `version` input (e.g. `version: "1.0.1"`), rather than floating on `latest`. When `version: latest` is requested, the action resolves the newest release from PyPI and fails clearly if network resolution is unavailable.

### 1.2 Action Outputs

| Output | Description |
|---|---|
| `sarif-file` | Path to the generated SARIF file |
| `findings` | Total number of diagnostic findings detected (populated for `sarif` or `json` formats) |
| `errors` | Total number of error findings detected (populated for `sarif` or `json` formats) |
| `warnings` | Total number of warning findings detected (populated for `sarif` or `json` formats) |
| `exit-code` | Exit code returned by `stackvet` |

---

## 2. Custom CI Configuration

### 2.1 Basic CI Check

Add a step to your `.github/workflows/ci.yml`:

```yaml
name: CI

on: [push, pull_request]

jobs:
  stackvet:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v3
        with:
          version: "latest"
      - run: uv run stackvet scan --ci --github-annotations
```

---

### 2.2 In Node.js / TypeScript CI Workflows (via npm / npx)

Run `stackvet` directly in your Node.js or monorepo CI without Python or Rust installation:

```yaml
name: Node.js Project Health Check

on: [push, pull_request]

jobs:
  stackvet-node:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: "22"

      # Run StackVet directly using npx
      - name: Run StackVet Health Scan
        run: npx @inzamol/stackvet scan --ci --github-annotations
```

---

### 2.3 GitHub PR Intelligence (Changed Code & Annotations)

Run PR intelligence on pull requests to analyze only modified lines/files, emit inline workflow command annotations (`::error` and `::warning`), report resolved issues, and generate a step summary:

```yaml
name: PR Intelligence

on:
  pull_request:
    types: [opened, synchronize, reopened]

jobs:
  stackvet-pr:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0 # Fetch git history for base ref comparison
      - uses: astral-sh/setup-uv@v3
        with:
          version: "latest"

      # Analyzes changed code, emits GitHub annotations, and generates step summary
      - name: Run QV PR Intelligence
        run: uv run stackvet pr --ci --github-annotations
```

---

### 2.4 Baseline & Differential Scanning (Prevent New Regressions)

For large existing codebases with pre-existing warnings or technical debt, use baseline snapshots so CI only fails on **new** diagnostic issues introduced in pull requests:

```yaml
name: CI (Baseline Verification)

on: [push, pull_request]

jobs:
  stackvet-baseline:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v3

      # Verify that no new issues exist beyond .stackvet-baseline.json
      - name: Verify QV Baseline
        run: uv run stackvet baseline verify --strict
```

Or pass `--baseline` to standard scans:
```bash
uv run stackvet scan --ci --baseline .stackvet-baseline.json
```

---

### 2.3 CI/CD Workflow & Dockerfile Auditing

Add targeted health audits to ensure your CI workflows and Docker environments conform to production security best practices:

```yaml
      # Audit GitHub Actions workflow definitions
      - name: Audit CI Workflows
        run: uv run stackvet ci

      # Audit Dockerfiles and docker-compose configurations
      - name: Audit Docker Assets
        run: uv run stackvet docker
```

---

### 2.4 GitHub Code Scanning with SARIF

`stackvet` supports emitting findings in **SARIF v2.1.0** format for GitHub Advanced Security and Code Scanning:

```yaml
name: Security & Health Scan

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  stackvet-sarif:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v3

      - name: Run stackvet SARIF scan
        run: uv run stackvet scan --sarif --output results.sarif
        continue-on-error: true

      - name: Upload SARIF report
        uses: github/codeql-action/upload-sarif@v4
        with:
          sarif_file: results.sarif
```

---

## 3. Pre-commit Integration

`stackvet` provides native pre-commit hooks via `.pre-commit-hooks.yaml`.

Add this to your project's `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/inzamol/stackvet
    rev: v0.1.0
    hooks:
      # Block commits if diagnostic errors or warnings are detected
      - id: stackvet-scan
        args: [--strict, --offline]

      # Optional: Automatically remediate safe issues on commit
      # - id: stackvet-fix
```

### 3.1 Available Hooks

| Hook ID | Description | Default Command |
|---|---|---|
| `stackvet-scan` | Runs diagnostic health scan on the project | `stackvet scan` |
| `stackvet-fix` | Automatically fixes safe issues (`DEP-002`, `ENV-002`, `PKG-001`) | `stackvet fix -y` |

---

## 4. Standardized Exit Codes for CI Automation

StackVet returns predictable exit codes across all commands and output formats (`text`, `json`, `sarif`, `html`):

| Exit Code | Classification | Trigger Condition |
|---|---|---|
| `0` | **Success** | Scan passed with no findings at or above `--fail-on` threshold, health score meets `--min-score`, or all findings are tolerated legacy baseline issues. |
| `1` | **Findings Violation** | Findings detected meeting or exceeding `--fail-on` threshold, health score below `--min-score`, or new regression findings introduced against baseline. |
| `2` | **Configuration Error** | Configuration syntax error in `stackvet.toml` or `pyproject.toml`, unknown rule ID in `[rules]`, or unsupported severity override level. |
| `3` | **Execution / I/O Error** | Target path does not exist, specified baseline file not found, or filesystem read/write error. |

---

## 5. Baseline-Aware CI Checks

In existing or legacy codebases, failing CI on pre-existing technical debt blocks modernization. StackVet's differential baseline system allows teams to establish a baseline snapshot and fail CI only when new regressions are introduced:

### Step 1: Establish Initial Baseline
```bash
# Record existing issues into .stackvet-baseline.json
stackvet baseline record
```
Commit `.stackvet-baseline.json` into your Git repository.

### Step 2: Enforce in CI
```bash
# In CI check: tolerate legacy baseline issues, fail only if new errors are introduced
stackvet check --baseline .stackvet-baseline.json --fail-on error
```
If developers resolve existing debt, StackVet automatically reports the resolved findings:
```text
✓ Baseline CI Check Passed: No new regressions at or above ERROR threshold.
  (42 legacy finding(s) tolerated by baseline)
✓ Resolved: 3 legacy baseline finding(s) fixed!
```

### Step 3: Update Baseline When Debt is Paid Down
```bash
# Update baseline to remove resolved issues and reflect current state
stackvet check --baseline .stackvet-baseline.json --update-baseline
```
