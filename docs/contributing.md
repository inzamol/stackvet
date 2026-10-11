# Contributing to stackvet

Thank you for your interest in contributing to `stackvet`! We welcome bug reports, documentation improvements, new diagnostic rules, performance enhancements, and ecosystem visualizers.

---

## 1. Development Setup

`stackvet` uses [`uv`](https://docs.astral.sh/uv/) for high-performance virtual environments and dependency management.

### 1.1 Prerequisites

- Python 3.10, 3.11, 3.12, or 3.13
- `uv` installed (`curl -LsSf https://astral.sh/uv/install.sh | sh` or `pip install uv` / `brew install uv`)
- Git

### 1.2 Clone & Sync Environment

```bash
git clone https://github.com/inzamol/stackvet.git
cd stackvet

# Sync all dependencies including dev, test, and doc tools
uv sync --all-extras
```

### 1.3 Install Pre-commit Hooks

Ensure all code meets style and safety checks automatically before committing:

```bash
uv run pre-commit install --hook-type pre-commit --hook-type pre-push
```

You can manually trigger pre-commit checks on all files at any time:

```bash
uv run pre-commit run --all-files
```

---

## 2. Testing & Validation

### 2.1 Fast Local Unit Tests

```bash
# Run full test suite
uv run pytest

# Run specific analyzer or reporter tests
uv run pytest tests/analyzers/
uv run pytest tests/reporters/
uv run pytest tests/test_tui.py

# Run with coverage report
uv run pytest --cov=src/stackvet --cov-report=term-missing
```

### 2.2 Type Checking & Linting

```bash
# Lint and format checks
uv run ruff check .
uv run ruff format --check .

# Strict static type checking
uv run pyright
```

### 2.3 Multi-Environment Matrix Testing (Tox)

Always ensure your changes pass locally across all supported Python versions and QA environments before opening a PR:

```bash
# Run all configured tox environments (py310, py311, py312, py313, lint, typecheck, docs)
uv run tox

# Run specific target environments
uv run tox -e py312
uv run tox -e lint
uv run tox -e typecheck
uv run tox -e docs
```

---

## 3. Architecture Overview

```text
                             CLI / TUI (click + rich)
                                        │
                                Project Discovery
                                        │
                            ProjectContext (Immutable)
                                        │
         ┌───────────────┬──────────────┴───────────────┬────────────────┐
   Dependencies    Environment                      Imports          Packaging
     Analyzer        Analyzer                       Analyzer          Analyzer
         └───────────────┼──────────────────────────────┴────────────────┘
                                        │
                                 AnalysisEngine
                                        │
                                Diagnostic Model
                                        │
                              Root Cause Correlator
                                        │
    ┌──────────────┬─────────────┼─────────────┬──────────────┬──────────────┐
 Terminal         JSON         SARIF         HTML         GitHub Actions   Interactive
 Reporter       Reporter     Reporter      Reporter         Annotator      TUI Dashboard
```

- **`src/stackvet/core/`**: Core immutable models (`ProjectContext`, `Diagnostic`, `ScanResult`), rule catalog registry, and config loading.
- **`src/stackvet/analyzers/`**: Subsystem analyzers (`dependencies`, `environment`, `imports`, `packaging`). Analyzers inspect `ProjectContext` and yield structured `Diagnostic` objects.
- **`src/stackvet/remediation/`**: Safe, deterministic fix generation and diff planning engine.
- **`src/stackvet/reporters/`**: Terminal, JSON, SARIF, HTML report generator, and GitHub Actions PR annotator.
- **`src/stackvet/tui/`**: Interactive Rich-based terminal dashboard (`stackvet inspect` / `stackvet ui`).
- **`src/stackvet/visualizers/`**: Dependency and circular import tree visualizers (`stackvet graph`).

---

## 4. Adding a New Diagnostic Rule

Adding new rules is straightforward and modular:

1. **Assign a Stable Rule ID**:
   - `DEP-xxx`: Dependency constraint, missing requirement, or version drift.
   - `ENV-xxx`: Python interpreter, Docker runtime, or CI matrix drift.
   - `IMP-xxx`: Circular imports or broken module import paths.
   - `PKG-xxx`: PEP 621 packaging and metadata validation.

2. **Register in Rules Registry**:
   - Add rule metadata (title, severity, documentation, description) in [`src/stackvet/rules/registry.py`](file:///c:/Users/inzam/RND/stackvet/src/stackvet/rules/registry.py).

3. **Implement Analyzer Check**:
   - Add the logic to the appropriate analyzer in `src/stackvet/analyzers/`.
   - Ensure the diagnostic includes clear **`evidence`** (facts and file sources) and actionable **`suggestions`** (recommended CLI command or code snippet).

4. **Add Automated Remediation (Optional)**:
   - If the issue can be safely and deterministically resolved, implement a fix handler in [`src/stackvet/remediation/engine.py`](file:///c:/Users/inzam/RND/stackvet/src/stackvet/remediation/engine.py).

5. **Write Comprehensive Unit Tests**:
   - Add positive detection tests and negative (false-positive prevention) tests under `tests/analyzers/`.

6. **Update Documentation**:
   - Document the rule, its root cause, and remediation steps in [`docs/rules.md`](file:///c:/Users/inzam/RND/stackvet/docs/rules.md) and [`README.md`](file:///c:/Users/inzam/RND/stackvet/README.md).

---

## 5. Code Quality Standards

- **Strict Type Annotations**: All functions, methods, and public interfaces must be fully typed (verified with `pyright`).
- **Zero-Network Analyzer Core**: Standard diagnostic checks must remain 100% offline, local, and fast (sub-second execution).
- **Verifiable Evidence**: Every flagged diagnostic must provide specific source lines, version facts, or manifest references in its evidence list.
- **Clean Diffs & Safety**: Remediation fixes must preview exact diffs and commands before modifying user projects.
- **Passing Matrix**: All Tox environments (`py310` through `py313`, `lint`, `typecheck`, `docs`) must pass with 100% test coverage for newly introduced rules.

---

## 6. Submitting a Pull Request

1. Fork the repository and create a feature branch (`git checkout -b feat/my-new-rule`).
2. Implement your changes following the standards above.
3. Verify your work locally:
   ```bash
   uv run tox
   ```
4. Commit your changes with clear, descriptive commit messages.
5. Push to your branch and submit a Pull Request on GitHub.
