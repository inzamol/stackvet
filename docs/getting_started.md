# Getting Started with stackvet

`stackvet` is a high-performance, polyglot diagnostic and project health engine for Python, Node.js/TypeScript, Docker, and CI/CD environments. It analyzes your project configuration, environment, source code imports, ASTs, and dependency trees to pinpoint health issues and suggest safe, actionable fixes.

---

## 1. Installation & Execution

=== "Ephemeral Run (npx)"
    ```bash
    # Run project health scorecard without installing
    npx @inzamol/stackvet doctor

    # Run detailed diagnostic scan
    npx @inzamol/stackvet scan

    # Run automated remediation
    npx @inzamol/stackvet fix
    ```

=== "Using npm (Node.js)"
    ```bash
    # Install as dev dependency
    npm install --save-dev @inzamol/stackvet

    # Run health check
    npx stackvet doctor
    ```

=== "Using pnpm"
    ```bash
    # Install as dev dependency
    pnpm add -D @inzamol/stackvet

    # Run health check
    pnpm exec stackvet doctor
    ```

=== "Using yarn"
    ```bash
    # Install as dev dependency
    yarn add -D @inzamol/stackvet

    # Run health check
    yarn stackvet doctor
    ```

=== "Ephemeral Run (uvx / pipx)"
    ```bash
    # Run project health scorecard
    uvx stackvet doctor
    # or
    pipx run stackvet doctor

    # Run detailed diagnostic scan
    uvx stackvet scan
    ```

=== "Using uv (Python)"
    ```bash
    uv add stackvet --dev
    uv run stackvet doctor
    ```

=== "Using pip (Python)"
    ```bash
    pip install stackvet
    stackvet doctor
    ```

=== "Using poetry"
    ```bash
    poetry add stackvet --group dev
    poetry run stackvet doctor
    ```

=== "Via Cargo (Rust)"
    ```bash
    cargo install stackvet
    stackvet doctor
    ```

=== "Global CLI Tool"
    ```bash
    # Using npm
    npm install -g @inzamol/stackvet

    # Using uv tool
    uv tool install stackvet

    # Using pipx
    pipx install stackvet
    ```

=== "From GitHub (Latest)"
    ```bash
    pip install git+https://github.com/inzamol/stackvet.git
    ```

=== "From Local Codebase"
    ```bash
    git clone https://github.com/inzamol/stackvet.git
    cd stackvet

    # Using uv
    uv pip install -e .

    # Using pip
    pip install -e .
    ```

---

## 2. Initialize Project Configuration (`stackvet init`)

Before running extensive scans or configuring custom rule thresholds, initialize standard settings in your `pyproject.toml`:

```bash
stackvet init
```

This creates a standard `[tool.stackvet]` configuration block without overwriting any of your existing project settings:

```toml
[tool.stackvet]
min_severity = "warning"

[tool.stackvet.dependencies]
ignore = [
    "amqp",
]

[tool.stackvet.rules]
DEP-003 = "warning"
SQL-014 = "error"

[tool.stackvet.paths]
exclude = [
    "tests",
    "migrations",
]
```

You can also generate a ready-to-run GitHub Actions workflow:

```bash
stackvet init --ci github
```

---

## 3. Run Project Health Doctor (`stackvet doctor`)

Get a complete project health report across all 6 core pillars (Dependencies, Security, Packaging, Architecture, Environment, Framework) with health score, letter grade, root cause clusters, and detailed diagnostic breakdown:

```bash
stackvet doctor
```

Example output:

```text
=======================================================================
                StackVet Universal Project Health Report               
=======================================================================

  Health Score: 87 / 100
  Overall Grade: B (Satisfactory)
  Calculation:   Base 100 - 13 deductions (3 unique findings, 0 duplicates ignored)

  Critical: 0  |  Errors: 1  |  Warnings: 2

--- Health Score Deduction Breakdown ---
  • [ERROR] SQL-014: -8 pts (1 finding) - N+1 query detected in user fetch
  • [WARN] DEP-002: -2 pts (1 finding) - Missing dependency: httpx
  • [WARN] ENV-003: -2 pts (1 finding) - Python version mismatch

--- Root Cause Clusters (3) ---
1. [ERROR] Rule Violation: SQL-014
   Summary: Query loop executing separate queries for related entities without joined load or subquery eager load.
   Remediation: Use selectinload() or joinedload() for relationship loading.
   Symptoms Count: 1

2. [WARN] Rule Violation: DEP-002
   Summary: Imported package is used in source code but not declared in pyproject.toml or requirements.txt.
   Remediation: Add the missing dependency to pyproject.toml or package.json.
   Symptoms Count: 1

3. [WARN] Rule Violation: ENV-003
   Summary: Detected Python runtime discrepancy between pyproject.toml and Dockerfile.
   Remediation: Align the python version specification across all configuration files.
   Symptoms Count: 1

--- Diagnostic Details ---
╭─────────┬──────────┬────────┬─────────────────────────┬──────────────────────────────────╮
│ Rule    ┆ Severity ┆ Target ┆ File Location           ┆ Message                          │
╞═════════╪══════════╪════════╪═════════════════════════╪══════════════════════════════════╡
│ SQL-014 ┆ ERROR    ┆ Code   ┆ app/routers/users.py:42 ┆ N+1 query detected in user fetch │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ DEP-002 ┆ WARN     ┆ Deps   ┆ app/services/client.py  ┆ Missing dependency: httpx        │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ ENV-003 ┆ WARN     ┆ Env    ┆ Dockerfile:1            ┆ Python version mismatch          │
╰─────────┴──────────┴────────┴─────────────────────────┴──────────────────────────────────╯
```

---

## 4. Run Fast CI Gate (`stackvet check` / `stackvet scan`)

Run `stackvet scan` in your project root to perform a fast CI check with exit code gating (0 for clean, 1 for errors):

```bash
stackvet scan
```

Example output:

```text
✗ CI Check Failed: 1 finding(s) at or above ERROR threshold.
  [ERROR] DEP-001: celery 5.4.0 requires kombu<5.4.0,>=5.3.0, but installed is kombu 5.5.2
     at requirements.txt:4
```

---

## 5. Explore & Remediate Findings

### 5.1 Interactive TUI Dashboard (`stackvet inspect`)
Launch the interactive terminal dashboard to navigate findings, inspect evidence, view dependency trees, and apply fixes with keyboard shortcuts:

```bash
stackvet inspect
```

### 5.2 Safe Automated Remediation (`stackvet fix`)
Safely apply deterministic fixes (such as adding undeclared dependencies or pruning unused packages):

```bash
# Preview proposed diffs without touching disk
stackvet fix --dry-run

# Apply all safe fixes automatically
stackvet fix -y
```

---

## 6. Understand Diagnostic Rules (`stackvet explain`)

If a specific diagnostic rule is flagged, you can get in-depth guidance, common causes, and remediation advice:

```bash
stackvet explain DEP-001
```

---

## 7. Next Steps

- Explore the [CLI Commands Reference](cli_reference.md) for all commands, arguments, and flags.
- Learn about the [Rules Catalog](rules.md) to understand detected issues.
- Read the [Configuration Guide](configuration.md) to customize rules for your project.
- Integrate into [CI/CD](ci_integration.md) with SARIF and strict modes.
