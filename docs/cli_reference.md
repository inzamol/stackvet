# CLI Reference

`stackvet` provides commands for scanning projects, explaining findings, configuring rules, and inspecting specific subsystems.

---

## 1. Global Options & Exit Codes

```bash
stackvet --help
stackvet --version
stackvet --config <PATH>
```

### Process Exit Codes

`stackvet` uses stable, standardized exit codes across all commands, checks, and output formats (Text, JSON, SARIF, HTML):

| Exit Code | Classification | Meaning |
|---|---|---|
| `0` | **Success** | Clean project, no findings meeting/exceeding `--fail-on` threshold, health score meets `--min-score`, or all findings are tolerated legacy baseline issues. |
| `1` | **Findings Violation** | Findings detected meeting or exceeding the configured `--fail-on` threshold, health score below `--min-score`, or new regression findings introduced against baseline. |
| `2` | **Configuration Error** | Invalid configuration syntax, unknown rule ID in `[rules]`, or unsupported severity override level. |
| `3` | **Execution / I/O Error** | Target path not found, specified baseline file not found, or filesystem read/write error. |

---

## 2. Project Configuration (`stackvet.toml` / `pyproject.toml`)

StackVet automatically discovers project configuration from `stackvet.toml`, `.stackvet.toml`, or `pyproject.toml` under `[tool.stackvet]`.

### Precedence Hierarchy
Settings are resolved deterministically using a 3-tier hierarchy:
1. **CLI Arguments** (e.g. `--fail-on`, `--strict`, `--min-score`, `--baseline`) — Highest precedence.
2. **Project Configuration File** (`stackvet.toml` / `.stackvet.toml` / `pyproject.toml`) — Overrides defaults.
3. **Built-in Defaults** — Fallback.

### Configuration Schema

```toml
# stackvet.toml or .stackvet.toml
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
CEL-001 = "disabled"
ARC-001 = "error"

# Detailed syntax with explicit enable flag
[rules.FAP-001]
severity = "error"
enabled = true
```

---

## 3. `stackvet init`

Generates or updates project configuration (`.stackvet.toml` or `pyproject.toml`) without overwriting existing settings. Optionally generates a pre-configured CI workflow for GitHub Actions.

```bash
stackvet init [PATH] [OPTIONS]
```

# Initialize in a specific project path
stackvet init ./services/backend

# Initialize configuration and generate GitHub Actions workflow
stackvet init --ci github

# Preview generated configuration without writing to disk
stackvet init --dry-run --ci github

# Overwrite existing [tool.stackvet] block
stackvet init --force
```

> [!NOTE]
> When generating CI workflows (`stackvet init --ci github`), QV automatically detects package managers. For `uv` projects (`uv.lock` present), it configures `uvx --from stackvet stackvet scan --ci --github-annotations`, allowing `stackvet` to run in an ephemeral tool environment without requiring `stackvet` to be added as a project dependency. For standard pip projects, it installs `stackvet` and runs `stackvet scan --ci --github-annotations`.

---

## 3. `stackvet doctor`

One command that provides a complete, high-level project health scorecard across all 6 core pillars (Dependencies, Security, Packaging, Architecture, Environment, Framework) with progress bars and top priority problems.

```bash
stackvet doctor [PATH] [OPTIONS]
```

Example:

```text
QV Project Health
────────────────────────────────────────

Dependencies      92/100   █████████░
Security           84/100   ████████░░
Packaging          96/100   ██████████
Architecture       78/100   ████████░░
Environment        91/100   █████████░
Framework          88/100   █████████░

Health Score: 87/100

3 high-priority issues
7 warnings
42 checks passed

Top problems:
  1. SQL-014  N+1 query detected
  2. DEP-002  Missing dependency: httpx
  3. ENV-003  Python version mismatch

Run `stackvet explain SQL-014` for details.
```

### 3.1 Arguments

| Argument | Description | Default |
|---|---|---|
| `PATH` | Path to the project root directory | `.` (current directory) |

### 3.2 Options

| Option | Description |
|---|---|
| `--min-score <SCORE>` | Fail CI with exit code 1 if health score is below threshold (e.g. `--min-score 85`) |
| `--explain` | Displays detailed breakdown and diagnostic attribution explaining health score deductions |
| `--strict` | Promotes warnings to errors (fails CI if any warnings exist) |
| `--ci` | Runs non-interactively with strict mode enabled |
| `--baseline <FILE>` | Filter findings against an established baseline file (ignores pre-existing issues) |
| `--json` | Emits doctor health scorecard and `score_breakdown` as structured JSON |
| `--sarif` | Emits scan results as standard SARIF v2.1.0 format |
| `--top <N>` | Number of top problems to highlight (default: 3) |
| `--format`, `-f <FORMAT>` | Specify output format (`text`, `json`, `sarif`, `html`) |
| `--html` | Generates an interactive, standalone HTML dashboard with health score breakdown |
| `--offline` | Disables remote vulnerability/CVE queries (airgapped mode) |
| `--output`, `-o <FILE>` | Writes output directly to a file |

---

## 4. `stackvet scan`

Runs full diagnostic analysis on the specified directory.

```bash
stackvet scan [PATH] [OPTIONS]
```

### 4.1 Arguments

| Argument | Description | Default |
|---|---|---|
| `PATH` | Path to the project root directory | `.` (current directory) |

### 4.2 Options

| Option | Description |
|---|---|
| `--strict` | Promotes warnings to errors (equivalent to `--fail-on warning`) |
| `--fail-on <SEVERITY>` | Fail CI if findings meet or exceed severity (`hint`, `info`, `warning`, `error`, `critical`) |
| `--ci` | Runs non-interactively with strict mode enabled |
| `--baseline <FILE>` | Filter findings against an established baseline file (ignores pre-existing issues) |
| `--update-baseline` | Update baseline file with current findings and remove resolved issues |
| `--json` | Emits scan results as structured JSON |
| `--sarif` | Emits scan results as standard SARIF v2.1.0 format |
| `--format`, `-f <FORMAT>` | Specify output format (`json`, `sarif`, `html`, `terminal`, `text`) |
| `--html <FILE>` | Generates an interactive, standalone HTML dashboard |
| `--github-annotations` | Emits GitHub Actions inline PR workflow annotations |
| `--offline` | Disables remote vulnerability/CVE queries (airgapped mode) |
| `--hide-warnings`, `-W` | Suppresses warning diagnostics from terminal output |
| `--errors-only`, `-E` | Only shows blocking error diagnostics (hides warnings and info) |
| `--severity <LEVEL>` | Filter findings by minimum severity (`error`, `warning`, `info`) |
| `--output`, `-o <FILE>` | Writes output directly to a file |

### 4.3 Exit Codes

| Exit Code | Meaning |
|---|---|
| `0` | Clean project — no blocking errors or warnings |
| `1` | Blocking findings detected |
| `2` | Invalid configuration or unknown rule ID |
| `3` | Analysis or runtime failure |

---

## 5. `stackvet inspect` (alias: `stackvet ui`)

Launches an interactive terminal dashboard (TUI) to navigate findings, expand evidence, view dependency trees, and apply fixes interactively with keyboard shortcuts.

```bash
stackvet inspect [PATH] [OPTIONS]
stackvet ui [PATH] [OPTIONS]
```

### 5.1 Controls

| Key | Action |
|---|---|
| `↑` / `k` | Navigate to previous finding |
| `↓` / `j` | Navigate to next finding |
| `Enter` / `Space` | Toggle details expansion |
| `f` | Apply remediation fix for selected finding |
| `t` | Switch between Diagnostics and Dependency Tree view |
| `q` / `Esc` | Exit interactive explorer |

---

## 6. `stackvet fix`

Safely and deterministically fixes detectable diagnostic health issues (e.g., adding missing dependencies to `pyproject.toml`, creating default configuration schemas, and initializing packaging metadata). Note that unused dependency removals (`DEP-003`) are marked for manual review to prevent accidental deletion of runtime plugins.

```bash
stackvet fix [PATH] [OPTIONS]
```

### 6.1 Options

| Option | Description |
|---|---|
| `--dry-run` | Shows proposed fixes and diffs without modifying any files |
| `--diff` | Shows unified color diff of proposed file modifications before applying |
| `-i`, `--interactive` | Prompts for interactive confirmation before applying each fix |
| `-y`, `--yes` | Automatically applies all safe fixes without interactive confirmation |
| `--rule <RULE_ID>` | Filters remediation to a specific rule ID (e.g. `DEP-002`) |
| `--sync` | Runs suggested package manager install/sync commands |

Example:

```bash
# Preview proposed unified diffs
stackvet fix --diff

# Apply fixes with interactive confirmation per finding
stackvet fix -i

# Preview proposed fixes in dry-run mode
stackvet fix --dry-run

# Apply all safe fixes automatically
stackvet fix -y
```

---

## 7. `stackvet baseline`

Manages baseline snapshots (`.stackvet-baseline.json`) of known diagnostic issues to enable differential scanning in legacy codebases without failing CI on pre-existing debt.

```bash
stackvet baseline record [PATH] [OPTIONS]
stackvet baseline verify [PATH] [OPTIONS]
```

### 7.1 Subcommands

#### `stackvet baseline record`
Records current diagnostic findings into a baseline JSON file:
```bash
# Record baseline to default .stackvet-baseline.json
stackvet baseline record

# Record baseline to a custom file
stackvet baseline record -o custom-baseline.json
```

#### `stackvet baseline verify`
Verifies that no new diagnostic issues have been introduced beyond the baseline, while tracking resolved debt:
```bash
# Verify against default .stackvet-baseline.json
stackvet baseline verify

# Verify failing only on errors (tolerating new warnings or info)
stackvet baseline verify --fail-on error

# Verify in strict mode against a specific baseline file
stackvet baseline verify -b custom-baseline.json --strict

# Verify and update the baseline file to accept resolved and new issues
stackvet baseline verify --update
```

| Option | Type | Description |
|---|---|---|
| `-b, --baseline <FILE>` | Path | Baseline filepath to check against [default: `.stackvet-baseline.json`] |
| `--fail-on <SEVERITY>` | String | Fail CI if new findings meet or exceed severity (`hint`, `info`, `warning`, `error`, `critical`) |
| `--strict` | Flag | Promotes warnings to failure threshold (equivalent to `--fail-on warning`) |
| `-u, --update` | Flag | Update baseline file with current findings and remove resolved issues |

---

## 8. `stackvet graph` / `stackvet graph`

Visualizes direct vs transitive package dependencies and internal source module import architecture (with circular import cycles highlighted).

```bash
stackvet graph [PATH] [OPTIONS]
```

### 8.1 Options

| Option | Description |
|---|---|
| `-d`, `--dependencies` | Visualizes direct and transitive package dependencies |
| `-i`, `--imports` | Visualizes internal Python module imports and circular cycles |
| `-r`, `--risk` | Annotates dependency tree with functional classifications, AST imports, and risk scores |
| `-L`, `--depth <N>` | Maximum depth level for the tree (default: 5) |
| `--json` | Emits dependency and import statistics as JSON |

Example:

```bash
# View full project tree (dependencies and imports)
stackvet graph

# View dependency risk and usage graph
stackvet graph --risk

# View only internal module import hierarchy and circular loops
stackvet graph --imports

# View dependency tree up to 2 levels deep
stackvet graph -d -L 2
```

---

## 9. `stackvet explain`

Displays detailed explanations, evidence requirements, and remediation instructions for a rule.

```bash
stackvet explain <RULE_ID>
```

Example:

```bash
stackvet explain DEP-002
stackvet explain CI-001
stackvet explain DJG-004
```

---

## 10. Targeted Subsystem Commands

Run focused checks on specific areas without executing the full scan:

### 10.1 `stackvet dependency`
Scans for dependency conflicts, missing imports, unused packages, and version mismatches, or renders an annotated Dependency Risk Graph.

```bash
stackvet dependency [PATH] [OPTIONS]
```

Options:
- `-g, --graph`: Render annotated dependency risk and functional usage graph.
- `-r, --risk [all|critical|high|medium|low|healthy]`: Filter dependency graph by risk level.
- `-L, --depth <N>`: Maximum depth level for the tree (default: 5).
- `--no-annotate`: Render compact dependency graph without multi-line annotation details.
- `--json`: Output scan results or dependency risk graph as structured JSON.

Example:
```bash
# Run dependency diagnostic checks
stackvet dependency

# Render annotated dependency risk graph
stackvet dependency --graph

# Filter dependency graph for critical/high risks only
stackvet dependency --graph --risk high
```

### 10.2 `stackvet environment`
Checks for Python runtime drift between local environment, Dockerfiles, and CI matrices.

```bash
stackvet environment [PATH]
```

### 10.3 `stackvet architecture`
Scans source code for circular imports, unresolved internal modules, architectural layer violations (`ARC-001`), and circular component cycles (`ARC-002`), or visualizes the project's Architectural Layer Map.

```bash
stackvet architecture [PATH] [OPTIONS]
stackvet architecture graph [PATH] [OPTIONS]
```

Options:
- `-g, --graph`: Render visual architecture layer map and dependency flow.
- `-f, --format [terminal|ascii|mermaid|dot|json]`: Output format for the architecture graph (default: `terminal`).
- `--json`: Output architecture map as structured JSON.
- `--strict`: Fail with exit code `1` if any architectural layer violations or circular component dependencies are detected.
- `-o, --output <FILE>`: Save generated architecture map or diagram directly to disk.

Example:
```bash
# Run architecture static diagnostic scan
stackvet architecture

# Visualize architectural layer map & flow in rich terminal
stackvet architecture graph
# or shorthand
stackvet architecture --graph

# Export architecture map as GitHub-flavored Mermaid diagram
stackvet architecture graph --format mermaid -o architecture.md

# Export architecture graph as structured JSON
stackvet architecture graph --json

# Fail CI if layer violations or component cycles exist
stackvet architecture graph --strict
```

### 10.4 `stackvet docker`
Audits Dockerfiles and docker-compose configurations for security best practices, root user risks, unpinned base images, sensitive file leaks, and cache optimization (`DOC-001` - `DOC-014`).

```bash
stackvet docker [PATH]
```

### 10.5 `stackvet ci`
Audits Continuous Integration workflows (`.github/workflows/*.yml`) for matrix mismatches, unpinned dependencies, outdated GitHub Actions, missing test/lint quality gates, hardcoded secrets, and missing concurrency cancellation (`CI-001` - `CI-006`).

```bash
stackvet ci [PATH]
```

### 10.6 `stackvet framework` (alias: `stackvet frameworks`)
Runs framework-specific diagnostic rules for FastAPI, SQLAlchemy, Django, and Celery.

```bash
stackvet framework [PATH] [OPTIONS]
```

Options:
- `-n, --name [fastapi|sqlalchemy|django|celery|sql|all]`: Filter analysis to a specific framework (default: all detected frameworks).
- `--json`: Output framework scan results as structured JSON.
- `--sarif`: Output framework scan results in SARIF v2.1.0 format.

Example:
```bash
# Scan current project for all framework issues
stackvet framework

# Scan specifically for FastAPI issues
stackvet framework --name fastapi

# Scan specifically for Django issues
stackvet framework --name django

# Scan specifically for Celery issues
stackvet framework --name celery

# Scan specifically for SQLAlchemy / SQL database issues
stackvet framework --name sqlalchemy
```

---

## 11. `stackvet pr` (alias: `stackvet pr-analysis`)

GitHub Pull Request Intelligence engine that analyzes only the changed code in a PR, reporting newly introduced issues versus resolved issues, emitting inline GitHub Actions workflow annotations, and generating PR markdown summaries.

```bash
stackvet pr [PATH] [OPTIONS]
```

Example Output:
```text
QV Pull Request Analysis

Changed files: 8
Affected checks: 17

New issues
──────────
FAP-021  users.py:42
Async endpoint performs blocking I/O

DEP-002  requirements.txt
Missing declaration for httpx

Resolved
────────
SQL-008
```

### 11.1 Options

| Option | Description |
|---|---|
| `-b, --base <REF>` | Base git branch or ref (default: auto-detected from `GITHUB_BASE_REF` or git history) |
| `--head <REF>` | Head git commit or ref (default: `HEAD`) |
| `--strict` | Promotes warnings to errors (fails CI on warnings) |
| `--ci` | Runs non-interactively with step summary & inline annotations |
| `--github-annotations` | Emits GitHub Actions inline workflow command annotations (`::error` and `::warning`) |
| `--comment` | Outputs formatted Markdown suitable for GitHub PR comments |
| `--json` | Emits PR analysis results as structured JSON |
| `--sarif` | Emits new PR findings in standard SARIF v2.1.0 format |
| `--baseline <FILE>` | Compare against an established baseline snapshot file |
| `--files <LIST>` | Comma-separated list of changed files (e.g. `users.py,requirements.txt`) |
| `--diff <FILE>` | Path to a unified diff patch file |
| `-o, --output <FILE>` | Writes report directly to a file |


