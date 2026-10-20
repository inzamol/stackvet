# Safe Automated Remediation

`stackvet` follows a strict design principle: **Diagnose first. Explain second. Fix safely.**

Unlike aggressive tools that rewrite code unpredictably, `stackvet fix` computes deterministic, reversible remediation plans. You can preview proposed diffs in advance with `--dry-run`, apply fixes interactively, or automate safe repairs in CI scripts.

---

## 1. Basic Usage

```bash
# Preview proposed unified color diffs without modifying files
stackvet fix --diff

# Interactive confirmation prompt before applying each individual fix
stackvet fix -i
# or
stackvet fix --interactive

# Preview proposed plan in tabular dry-run mode
stackvet fix --dry-run

# Automatically apply all safe fixes without prompting
stackvet fix -y

# Fix only a specific rule (e.g. missing dependencies)
stackvet fix --rule DEP-002 -y

# Apply fixes and execute package manager sync commands
stackvet fix --sync -y
```

---

## 2. Unified Diff Preview Mode (`--diff`)

Before modifying any files on disk, inspect the exact line-by-line changes `stackvet fix` proposes using `--diff`:

```bash
stackvet fix --diff
```

```diff
--- pyproject.toml
+++ pyproject.toml
@@ -8,6 +8,7 @@
 dependencies = [
     "click>=8.0.0",
     "rich>=13.0.0",
+    "httpx>=0.27.0",
 ]
```

---

## 3. Interactive Remediation (`-i` / `--interactive`)

Run fixes in interactive step-through mode where each individual diagnostic fix prompts for your explicit approval before mutating configuration or source code:

```bash
stackvet fix -i
```

```text
[1/2] Apply fix for DEP-002 (Add 'httpx' to pyproject.toml)? [y/N]: y
  + Successfully applied fix to pyproject.toml

[2/2] Apply fix for PKG-001 (Initialize pyproject.toml metadata)? [y/N]: n
  - Skipped fix for PKG-001
```

---

## 4. Safety Guarantees & Remediation Levels

`stackvet` classifies fixes into four safety categories:

| Fix Category | Example | Behavior |
|---|---|---|
| **Safe (Deterministic)** | Adding missing dependencies to `pyproject.toml` (`DEP-002`)<br>Adding default `[project]` / `[tool.stackvet]` settings (`PKG-001`) | Applied automatically with `-y` or after interactive confirmation. |
| **Review Required** | Removing unimported declared dependencies (`DEP-003`) | Requires explicit review because packages may be used dynamically, as plugins, or as database drivers. Skipped during `-y`. |
| **Command-Assisted** | Running `uv add` or `pip install` (`DEP-001`, `DEP-005`, `DEP-006`) | Proposed as shell commands. Only executed if `--sync` is explicitly passed. |
| **Manual / Complex** | Refactoring circular import cycles (`IMP-001`) | Provides architectural refactoring hints and code snippets for human review; no blind mutations. |

---

## 5. Remediation Plan Inspection (Dry Run)

Running `stackvet fix --dry-run` displays available automated fixes and diff previews:

```text
Found 2 automated fix(es):
1. pyproject.toml -> Add 'httpx' to project dependencies
2. pyproject.toml -> Remove unused dependency 'requests'

--- Proposed Changes (Preview) ---

File: pyproject.toml
@@ -10,3 +10,4 @@
 [project]
 dependencies = [
+    "httpx",
-    "requests",
 ]

Dry-run complete. No files were modified.
```

---

## 6. Package Manager Auto-Sync

When dependencies are modified, running `stackvet fix --sync` automatically calls the detected package manager (`uv sync`, `poetry install`, `pip install`) to keep your active virtual environment in lockstep:

```bash
stackvet fix --sync -y
```
