# stackvet Documentation

<div align="center" markdown>

**Diagnose why your Python project is unhealthy — understand the root cause and get safe, actionable fixes.**

[![PyPI Version](https://img.shields.io/badge/pypi-v0.1.1-blue.svg)](https://pypi.org/project/stackvet/)
[![Python Versions](https://img.shields.io/badge/python-3.10%20%7C%203.11%20%7C%203.12%20%7C%203.13-blue)](https://pypi.org/project/stackvet/)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)

</div>

`stackvet` is a high-performance, offline-first health analyzer for Python projects. It inspects your project manifests, active virtual environment, source module AST, and packaging metadata in milliseconds to uncover broken dependencies, undeclared packages, circular import loops, and runtime drift across Docker and CI.

---

## 1. Overview & Architecture

Unlike generic tools that only inspect a single lockfile or flat requirements list, `stackvet` correlates multi-layer diagnostic signals across your entire project ecosystem:

```mermaid
flowchart TD
    subgraph Discovery["1. Discovery & Context Ingestion"]
        A["<b>Project Manifests & Active Environment</b><br/>pyproject.toml • requirements.txt • active virtualenv"]
        B["<b>Source Code AST & Infrastructure</b><br/>Module AST imports • Dockerfile • CI workflows"]
    end

    subgraph Analyzers["2. Subsystem Analyzers"]
        C["<b>Dependencies Analyzer</b><br/>Constraint conflicts, missing & unused packages"]
        D["<b>Environment & Drift Analyzer</b><br/>Python interpreter, Docker base image & CI matrix drift"]
        E["<b>Imports & Architecture Analyzer</b><br/>Circular import cycle detection & unresolvable modules"]
        F["<b>Packaging Analyzer</b><br/>PEP 621 metadata & configuration schema"]
    end

    subgraph Correlator["3. Root Cause Engine & Health Score"]
        G["<b>Correlated Diagnostic Model</b><br/>Evidence mapping, root causes & Health Score (0–100)"]
    end

    subgraph Output["4. Multi-Channel Output & Remediation"]
        H["<b>Interactive Terminal Dashboard</b> (`stackvet inspect`)"]
        I["<b>Standalone HTML Reports</b> (`stackvet scan --html`)"]
        J["<b>CI / SARIF & Annotations</b> (`stackvet scan --sarif`)"]
        K["<b>Automated Safe Fixes</b> (`stackvet fix -y`)"]
    end

    A --> Analyzers
    B --> Analyzers
    Analyzers --> Correlator
    Correlator --> H
    Correlator --> I
    Correlator --> J
    Correlator --> K
```

👉 *Learn more in the comprehensive [Architecture & Internal Design](architecture.md) documentation.*

---

## 2. Install & Usage

Run `stackvet doctor` or `stackvet scan` directly in any Python project root:

=== "Run without installing (npx)"
    ```bash
    npx @inzamol/stackvet doctor
    ```

=== "Using npm (Node.js)"
    ```bash
    npm install --save-dev @inzamol/stackvet
    npx stackvet doctor
    ```

=== "Using pnpm / yarn"
    ```bash
    # pnpm
    pnpm add -D @inzamol/stackvet
    pnpm exec stackvet doctor

    # yarn
    yarn add -D @inzamol/stackvet
    yarn stackvet doctor
    ```

=== "Run without installing (uvx / pipx)"
    ```bash
    uvx stackvet doctor
    # or
    pipx run stackvet doctor
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

=== "Via Cargo (Rust)"
    ```bash
    cargo install stackvet
    stackvet doctor
    ```

=== "From GitHub (Latest)"
    ```bash
    pip install git+https://github.com/inzamol/stackvet.git
    stackvet doctor
    ```

=== "From Local Codebase"
    ```bash
    git clone https://github.com/inzamol/stackvet.git
    cd stackvet
    pip install -e .
    # or with uv
    uv pip install -e .
    ```

### 2.1 Live Diagnostic Output

```text
🔍 stackvet
Project: payment-service
Python:  3.12.7
Package Manager: uv

🔴 1 Errors   🟡 1 Warnings   🟢 48 Checks Passed

┌────────────────── 🔴 DEP-001 Dependency constraint conflict ─────────────────┐
│ celery 5.4.0 requires kombu<5.4.0,>=5.3.0, but installed is kombu 5.5.2.     │
│                                                                              │
│ Evidence:                                                                    │
│   • celery declared requirement: kombu<5.4.0,>=5.3.0                         │
│   • Installed kombu version: 5.5.2 in active environment                     │
│                                                                              │
│ Suggested fix:                                                               │
│   Upgrade celery or pin kombu to <5.4.0,>=5.3.0.                             │
│   $ uv add 'kombu<5.4.0,>=5.3.0'                                             │
└──────────────────────────────────────────────────────────────────────────────┘

Health Score: 85/100
```

---

## 3. Core Feature Pillars

<div class="grid cards" markdown>

-   __Interactive Terminal Dashboard__

    ---

    Explore findings interactively with keyboard navigation (`↑`/`↓`), expand evidence, view live dependency trees (`t`), and apply fixes (`f`) in a rich TUI.

    [:material-arrow-right: Open CLI Reference](cli_reference.md)

-   __Self-Contained HTML Reports__

    ---

    Generate portable, zero-dependency HTML reports (`stackvet scan --html`) with circular health score gauges, real-time search, severity filters, and dark mode.

    [:material-arrow-right: Open HTML Report Guide](html_report.md)

-   __Safe Automated Fixes__

    ---

    Compute deterministic remediation plans with `stackvet fix --dry-run` and safely synchronize manifests and package managers with `stackvet fix --sync -y`.

    [:material-arrow-right: Open Remediation Guide](remediation.md)

-   __Dependency & Import Trees__

    ---

    Visualize direct vs transitive package trees (`stackvet graph -d`) and internal module import architecture with circular loops highlighted (`stackvet graph -i`).

    [:material-arrow-right: Open Tree Visualizer Guide](tree.md)

</div>

---

## 4. Diagnostic Subsystems

| Subsystem | Scope & Capabilities | Key Rules |
|---|---|---|
| **Dependencies** | Constraint conflicts, undeclared imports, unused packages, Python compatibility mismatches | `DEP-001` through `DEP-006` |
| **Environment** | Interpreter version drift, Dockerfile base image mismatches, CI matrix synchronization | `ENV-001` through `ENV-003` |
| **Architecture** | Circular import cycle detection, unresolvable module imports, dead orphan files, deprecated stdlib | `IMP-001` through `IMP-004` |
| **Packaging** | PEP 621 metadata validation, TOML syntax verification, entrypoint checks | `PKG-001`, `PKG-002` |
| **Docker** | Security best practices, root user risks, unpinned base images, sensitive file leaks | `DOC-001` through `DOC-014` |
| **CI Workflows** | Matrix mismatches, unpinned dependencies, outdated GitHub Actions, quality gates, hardcoded secrets | `CI-001` through `CI-006` |
| **Frameworks** | FastAPI (`FAP-xxx`), SQLAlchemy (`SQL-xxx`), Django (`DJG-xxx`), Celery (`CEL-xxx`) | `FAP`, `SQL`, `DJG`, `CEL` |

👉 *Browse full rule descriptions and remediation strategies in the [Rules Catalog](rules.md).*

---

## 5. Feature Comparison

| Capability | `stackvet` | `pip check` | `deptry` | `pip-audit` |
|---|:---:|:---:|:---:|:---:|
| **Dependency Constraint Conflicts** | :white_check_mark: | :white_check_mark: | :x: | :x: |
| **Undeclared & Unused Dependencies** | :white_check_mark: | :x: | :white_check_mark: | :x: |
| **Circular Import Cycle Detection** | :white_check_mark: | :x: | :x: | :x: |
| **Python & Docker/CI Runtime Drift** | :white_check_mark: | :x: | :x: | :x: |
| **Interactive Terminal Explorer (TUI)** | :white_check_mark: | :x: | :x: | :x: |
| **Standalone Interactive HTML Reports** | :white_check_mark: | :x: | :x: | :x: |
| **Automated Safe Remediation (`stackvet fix`)** | :white_check_mark: | :x: | :x: | :x: |
| **SARIF v2.1.0 & GitHub PR Annotations** | :white_check_mark: | :x: | :x: | :x: |
| **Sub-Second Offline Execution** | :white_check_mark: | :white_check_mark: | :white_check_mark: | :x: (requires network) |

---

## 6. Explore Documentation

<div class="grid cards" markdown>

-   __Architecture & Design__

    ---

    Deep dive into discovery pipelines, immutable models, and analyzer subsystems.

    [:material-arrow-right: Explore Architecture](architecture.md)

-   __Getting Started Guide__

    ---

    Step-by-step setup, first health scan, and baseline configuration.

    [:material-arrow-right: Read Getting Started](getting_started.md)

-   __CLI Commands Reference__

    ---

    Complete flag reference, exit codes, and output formatting options.

    [:material-arrow-right: Explore CLI Reference](cli_reference.md)

-   __Diagnostic Rules Catalog__

    ---

    Exhaustive documentation for every diagnostic rule and remediation plan.

    [:material-arrow-right: View Rules Catalog](rules.md)

-   __Configuration Guide__

    ---

    Configure severities, ignore rules, and exclude path patterns in `pyproject.toml`.

    [:material-arrow-right: View Configuration](configuration.md)

-   __CI/CD & GitHub Actions__

    ---

    Integrate with GitHub Actions, SARIF code scanning alerts, and workflows.

    [:material-arrow-right: View CI Integration](ci_integration.md)

-   __Pre-commit Integration__

    ---

    Enforce clean health scans before commits reach your remote repository.

    [:material-arrow-right: View Pre-commit Guide](pre_commit.md)

-   __Contributing Guide__

    ---

    Development environment setup, Tox testing matrix, and authoring new rules.

    [:material-arrow-right: Read Contributing](contributing.md)

</div>
