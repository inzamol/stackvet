# Pre-commit Hook Integration

Catch dependency drift, missing imports, and circular cycles before code is ever committed with `stackvet`'s native [pre-commit](https://pre-commit.com/) integration.

---

## 1. Quick Setup

Add the following to your project's `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/inzamol/stackvet
    rev: v0.1.0
    hooks:
      # 1. Diagnostic scan (blocking on warnings & errors)
      - id: stackvet-scan
        args: [--strict, --offline]

      # 2. Optional: Auto-fix safe issues on commit
      # - id: stackvet-fix
```

Install and activate the hooks in your local Git repository:

```bash
pre-commit install
```

---

## 2. Available Hooks

### 2.1 `stackvet-scan`
Runs `stackvet scan` across your repository.

- **Default Arguments**: `[--strict, --offline]`
- **Behavior**: Fast, airgapped execution. Fails the commit if any blocking errors or warnings are detected.

### 2.2 `stackvet-fix`
Runs `stackvet fix -y` across your repository.

- **Default Arguments**: `[-y, --offline]`
- **Behavior**: Automatically remediates safe issues (like adding missing dependencies to `pyproject.toml`) and stages the clean file.

---

## 3. Testing Hooks Locally

Run pre-commit against all files without committing:

```bash
pre-commit run --all-files
```
