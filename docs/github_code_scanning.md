# GitHub Code Scanning Integration

StackVet produces fully compliant **OASIS SARIF 2.1.0** (Static Analysis Results Interchange Format) reports designed for direct ingestion into GitHub Advanced Security / GitHub Code Scanning.

---

## 1. Quick Setup: GitHub Actions Workflow

Create `.github/workflows/stackvet-analysis.yml` in your repository:

```yaml
name: "StackVet Security & Health Scan"

on:
  push:
    branches: [ "main" ]
  pull_request:
    branches: [ "main" ]
  schedule:
    - cron: '30 4 * * 1' # Weekly scan every Monday at 04:30 UTC

permissions:
  contents: read
  security-events: write
  actions: read

jobs:
  analyze:
    name: Run StackVet Diagnostics
    runs-on: ubuntu-latest

    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Set up Python
        uses: actions/setup-python@v5
        with:
          python-version: "3.11"

      - name: Install StackVet
        run: |
          pip install stackvet
          # Or download binary / use npm: npx -y @inzamol/stackvet

      - name: Run StackVet Scan
        run: |
          stackvet scan --format sarif --output stackvet-results.sarif || true

      - name: Upload SARIF to GitHub Code Scanning
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: stackvet-results.sarif
          category: stackvet-diagnostics
```

> [!IMPORTANT]
> The workflow requires `security-events: write` permissions to upload SARIF results to GitHub Code Scanning. For public repositories, GitHub Code Scanning is free. For private repositories, GitHub Advanced Security is required.

---

## 2. SARIF Level Mapping

StackVet diagnostic severities map directly to GitHub Code Scanning alert levels:

| StackVet Severity | SARIF Level | GitHub Alert State | Blocks PR Merge (if configured) |
|-------------------|-------------|--------------------|--------------------------------|
| `CRITICAL`        | `error`     | Critical / Error   | Yes                            |
| `ERROR`           | `error`     | Error              | Yes                            |
| `WARNING`         | `warning`   | Warning            | Optional rule                  |
| `INFO`            | `note`      | Notice / Info      | No                             |
| `HINT`            | `note`      | Recommendation     | No                             |

---

## 3. Advanced Features Supported in Code Scanning

### 3.1 Relative URIs and `%SRCROOT%`
StackVet relativizes all repository file paths against `%SRCROOT%`, URI-encoding spaces and special characters. This ensures GitHub can link findings directly to source code diffs in Pull Requests.

### 3.2 Precise Multiline Regions & Snippets
When diagnostics include line and column spans (`startLine`, `startColumn`, `endLine`, `endColumn`), GitHub highlights the exact lines and tokens in the file view.

### 3.3 Related Locations & Dependency Chains
For complex diagnostics like circular imports (`IMP-001`) or architectural layer violations (`ARC-001`), StackVet populates `relatedLocations`. GitHub Code Scanning displays the step-by-step navigation path through the cycle directly in the alert details.

### 3.4 Automated Fixes in PR Diffs
When StackVet provides automated remediations (e.g., updating version pins in `pyproject.toml`), fix changes are embedded in the SARIF `fixes` array.

---

## 4. Local Validation

To validate your generated SARIF file locally prior to pushing:

```bash
# Generate SARIF report
stackvet scan --format sarif --output scan.sarif

# Verify SARIF format
node -e "
  const sarif = JSON.parse(require('fs').readFileSync('scan.sarif', 'utf8'));
  console.log('Version:', sarif.version);
  console.log('Total Results:', sarif.runs[0].results.length);
"
```
