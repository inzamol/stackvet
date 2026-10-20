# Project Configuration Guide

StackVet automatically discovers and loads configuration from any of the following files in your project root:
1. `stackvet.toml`
2. `.stackvet.toml`
3. `pyproject.toml` (under the `[tool.stackvet]` table)

You can also explicitly specify a configuration file using `--config <PATH>`:
```bash
stackvet --config ./configs/custom-stackvet.toml check
```

---

## 1. Configuration Schema

Below is an annotated example of a complete `stackvet.toml` or `.stackvet.toml` file:

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
# Simple override syntax: set custom severity or disable rules
DEP-001 = "warning"
ARC-001 = "error"
CEL-001 = "disabled"   # or "off"

# Detailed table syntax with explicit enable/disable flag
[rules.FAP-001]
severity = "error"
enabled = true

# Ignore patterns (glob expressions relative to project root)
[ignore]
patterns = [
    "legacy/**",
    "scripts/generate_*.py",
    "docs/snippets/**",
]

# Configured suppressions with rule, path, expiration date, and rationale
[[suppressions]]
rule = "DEP-001"
path = "legacy/**/*.py"
reason = "Legacy package will be deprecated in Q3"

[[suppressions]]
rule = "SQL-001"
until = "2026-12-31"
reason = "Temporary bypass during query refactor"

[[suppressions]]
rules = ["NDEP-001", "RCT-001"]
path = "frontend/src/legacy/**"
reason = "Exempt legacy react components from strict checks"
```

---

## 2. Configuring via `pyproject.toml`

For Python projects, you can embed your StackVet configuration directly in `pyproject.toml`:

```toml
[tool.stackvet]
min_severity = "warning"
fail_on = "error"
min_score = 80
ignore = ["legacy/**", "benchmarks/**"]

[tool.stackvet.rules]
DEP-001 = "warning"
CEL-001 = "off"

[[tool.stackvet.suppressions]]
rule = "DEP-001"
path = "legacy/**/*.py"
until = "2026-12-31"
reason = "Temporary migration grace period"
```

---

## 3. Rule Overrides & Disabling

You can customize how individual diagnostic rules are evaluated:

| Value | Behavior |
|---|---|
| `"critical"` | Treats finding as Critical (-20 health score points, blocks CI if `fail_on` is critical/error/warning/info) |
| `"error"` | Treats finding as Error (-8 health score points, blocks CI if `fail_on` is error/warning/info) |
| `"warning"` / `"warn"` | Treats finding as Warning (-2 health score points, blocks CI if `--strict` or `fail_on = "warning"`) |
| `"info"` | Informational observation (0 health score points deduction) |
| `"hint"` | Advisory observation (0 health score points deduction) |
| `"disabled"` / `"off"` | Disables the rule completely; omitted from scans, reports, and health score deductions |

### Validation & Zero False Positives
StackVet rigorously validates all rule IDs against the 132-rule catalog:
- If an unknown rule ID (e.g. `NONEXISTENT-999`) is declared in `[rules]` or `[[suppressions]]`, StackVet exits with **Exit Code 2 (Configuration Error)** and provides a clear error message.
- If an unsupported severity string (e.g. `"catastrophic"`) is provided, StackVet exits with **Exit Code 2**.
- If a suppression entry specifies an invalid ISO date format for `until` (not `YYYY-MM-DD`), StackVet exits with **Exit Code 2**.

---

## 4. Ignore Rules & File Exclusions

StackVet provides a multi-tiered file exclusion system that prevents scanning irrelevant build outputs, package caches, virtual environments, and generated code:

### 4.1 Built-in Default Exclusions
By default, StackVet automatically excludes known build artifacts and package dependencies without requiring configuration:
- `.git`
- `node_modules`
- `target`
- `dist`, `build`, `out`, `.output`
- `.venv`, `venv`, `env`, `.env`
- `__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.tox`
- `.next`, `.nuxt`, `.turbo`, `.cache`

Root project manifests (`package.json`, `pyproject.toml`, `requirements.txt`, `Cargo.toml`, etc.) are always discovered and inspected even when build folders exist in the repository root.

### 4.2 `.stackvetignore` File
You can place a `.stackvetignore` file in the root of your project using standard `.gitignore` syntax:
```gitignore
# Ignore custom test fixtures and legacy scripts
tests/fixtures/**
legacy/**/*.py
*.generated.ts
```

### 4.3 Configuration `[ignore]`
Add ignore patterns in `stackvet.toml` or `pyproject.toml`:
```toml
[ignore]
patterns = [
    "generated/**",
    "scripts/benchmarks/**",
]
```

### 4.4 CLI `--ignore` Flag
Pass repeatable `--ignore <PATTERN>` flags to exclude patterns for a single command invocation:
```bash
stackvet doctor --ignore "temp_*.py" --ignore "archive/**"
```

### 4.5 Exclusion Precedence
Exclusions are evaluated using a 4-tier precedence:
1. **CLI `--ignore <PATTERN>`** (Highest precedence)
2. **Configuration file `[ignore]`** (`stackvet.toml` / `pyproject.toml`)
3. **`.stackvetignore`** file in repository root
4. **Built-in default exclusions** (Lowest precedence)

---

## 5. Configurable Diagnostic Suppressions

When a diagnostic cannot be resolved immediately, you can suppress it via configuration or inline comments. Suppressed diagnostics do not deduct health score points and do not trigger check failures.

### 5.1 Configuration Suppressions (`[[suppressions]]`)
Suppressions can be scoped by rule ID, file glob pattern, specific line number, and expiration date:

```toml
[[suppressions]]
rule = "DEP-001"
path = "legacy/**/*.py"
reason = "Scheduled for cleanup in v2.0"

[[suppressions]]
rule = "SQL-001"
until = "2026-12-31"   # ISO 8601 YYYY-MM-DD
reason = "Temporary bypass while DB query is optimized"

[[suppressions]]
rules = ["NDEP-001", "RCT-001"]
path = "src/legacy/**"
reason = "Legacy frontend components"
```

#### Expiration Monitoring
When `until = "YYYY-MM-DD"` is specified, StackVet verifies the expiration date during every scan:
- **Active suppressions** (date not reached): Findings are silenced and tallied in the suppression count.
- **Expired suppressions** (date passed): The suppression becomes inactive. Findings are re-evaluated, and StackVet issues a prominent warning:
  ```text
  ⚠️ Warning: 1 suppression entry(ies) in configuration have expired and are no longer active.
  ```

### 5.2 Inline Comment Suppressions
You can suppress diagnostics directly inside Python, JavaScript, and TypeScript source files using comments:

#### Line-Level Suppression
Applies to the exact line containing the comment:
```python
# Python
time.sleep(1)  # stackvet:ignore[FAP-001]: intentional delay
```
```typescript
// TypeScript / JavaScript
import 'legacy-pkg'; // stackvet:ignore[NDEP-001]: intentional legacy import
// or block comment:
useLegacyHook(); /* stackvet:ignore[RCT-001]: legacy hook */
```

#### Next-Line Suppression
Applies to the immediately following line:
```python
# stackvet:ignore-next-line[DEP-001]: expected missing package
import optional_feature
```
```typescript
// stackvet:ignore-next-line[NXT-001]
export const dynamic = 'force-dynamic';
```

#### File-Level Suppression
Suppresses all instances of the rule across the entire file:
```python
# stackvet:ignore-file[DJG-001]: Suppress Django warning across entire file
```
```typescript
// stackvet:ignore-file[NDEP-001]: Legacy compatibility wrapper
```

#### Alternative / Linter Syntax
StackVet also recognizes standard alternative directives:
- `# stackvet:disable=RULE` / `// stackvet:disable=RULE`
- `# stackvet:disable-next-line=RULE` / `// stackvet:disable-next-line=RULE`
- `# stackvet:disable-file=RULE` / `// stackvet:disable-file=RULE`
- `# noqa: stackvet[RULE]`

---

## 6. Precedence Hierarchy

StackVet resolves settings deterministically using a strict precedence hierarchy:

```text
┌─────────────────────────────────────────────────────────┐
│ 1. CLI Arguments (--fail-on, --strict, --min-score)     │  (Highest precedence)
├─────────────────────────────────────────────────────────┤
│ 2. Project Config (stackvet.toml / pyproject.toml)      │
├─────────────────────────────────────────────────────────┤
│ 3. Built-in Defaults (fail_on = Error, min_score = None)│  (Lowest precedence)
└─────────────────────────────────────────────────────────┘
```

