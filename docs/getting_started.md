# Getting Started with stackvet

`stackvet` is a package-manager-agnostic diagnostic platform for Python projects. It analyzes your project configuration, environment, source code imports, and dependency tree to pinpoint health issues and suggest safe, actionable fixes.

---

## 1. Installation & Execution

=== "Ephemeral Run (uvx / pipx)"
    ```bash
    # Run project health scorecard
    uvx stackvet doctor
    # or
    pipx run stackvet doctor

    # Run detailed diagnostic scan
    uvx stackvet scan
    ```

=== "Using uv"
    ```bash
    uv add stackvet --dev
    ```

=== "Using pip"
    ```bash
    pip install stackvet
    ```

=== "Using poetry"
    ```bash
    poetry add stackvet --group dev
    ```

=== "Global CLI Tool"
    ```bash
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

Get a complete project health report across all 6 core pillars (Dependencies, Security, Packaging, Architecture, Environment, Framework) with progress bars and top priority issues:

```bash
stackvet doctor
```

Example output:

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

---

## 4. Run Detailed Diagnostic Scan (`stackvet scan`)

Run `stackvet scan` in your project root to perform a comprehensive diagnostic audit with full evidence panels:

```bash
stackvet scan
```

Example output:

```text
🔍 stackvet
Project: my-api
Python:  3.12.7
Package Manager: uv

🔴 1 Errors   🟡 1 Warnings   🟢 48 Checks Passed

┌────────────────── 🔴 DEP-001 Dependency constraint conflict ─────────────────┐
│ celery 5.4.0 requires kombu<5.4.0,>=5.3.0, but installed is kombu 5.5.2.     │
│                                                                              │
│ Evidence:                                                                    │
│   • celery declared requirement: kombu<5.4.0,>=5.3.0                         │
│   • Installed kombu version: 5.5.2                                           │
│                                                                              │
│ Suggested fix:                                                               │
│   Upgrade celery or pin kombu to <5.4.0,>=5.3.0.                             │
│   $ uv add 'kombu<5.4.0,>=5.3.0'                                             │
└──────────────────────────────────────────────────────────────────────────────┘

Health Score: 85/100
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
