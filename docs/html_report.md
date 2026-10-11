# Self-Contained HTML Report

`stackvet` can generate a single-file, zero-dependency, self-contained HTML report that is fully portable and interactive. You can open it in any web browser, attach it to CI artifacts, or email it to teammates.

---

## 1. Generating an HTML Report

Use the `--html` flag with `stackvet scan`:

```bash
# Generate report for current project
stackvet scan --html report.html

# Scan a microservice and write to a specific location
stackvet scan ./services/auth --html ./build/auth-health.html

# Combine with strict mode
stackvet scan --strict --html health-report.html
```

---

## 2. Features of the HTML Report

### 2.1 Zero External Dependencies
- All styles (CSS) and interactive scripts (vanilla JavaScript) are **embedded directly** in a single `.html` file.
- Works 100% offline — no CDNs, external web fonts, or tracking scripts required.

### 2.2 Interactive Circular Health Score
- Displays the project's health score (0–100) using a smooth SVG circular progress gauge.
- Dynamically color-coded: **Green** (80–100), **Yellow** (50–79), **Red** (0–49).

### 2.3 Real-Time Search & Filtering
- **Search Bar**: Instant client-side filtering by rule ID (`DEP-002`), title, description, or file name.
- **Severity Filters**: Filter diagnostics by **All**, **Errors Only**, or **Warnings Only** with interactive counters.

### 2.4 Detailed Diagnostic Cards
Each finding card includes:
- Severity pill badge and rule ID tag.
- Location badge (e.g., `pyproject.toml`, `main.py:14`).
- Collapsible **Evidence** section listing facts extracted from manifests and AST.
- **Remediation Box** with one-click copy buttons for CLI commands and code snippets.

### 2.5 Dark / Light Mode Toggle
- Includes a theme switch button in the top navigation bar.
- Automatically detects the user's system preferences (`prefers-color-scheme`).

---

## 3. Automating in GitHub Actions

You can automatically generate and upload the HTML report as a build artifact on every Pull Request:

```yaml
name: Project Health Scan

on: [push, pull_request]

jobs:
  health-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v5

      - name: Generate HTML Report
        run: uv run stackvet scan --html stackvet-report.html
        if: always()

      - name: Upload Report Artifact
        uses: actions/upload-artifact@v4
        with:
          name: stackvet-health-report
          path: stackvet-report.html
        if: always()
```
