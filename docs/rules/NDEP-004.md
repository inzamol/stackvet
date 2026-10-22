# NDEP-004: Lockfile Out of Sync

| Attribute | Value |
|-----------|-------|
| **Rule ID** | `NDEP-004` |
| **Category** | `node` |
| **Default Severity** | `WARN` |

## Description
Package lockfile (pnpm-lock.yaml / package-lock.json) is missing or out of sync.

## Why This Matters
Detecting and addressing lockfile out of sync prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
Run 'pnpm install' or 'npm install' to generate and sync the lockfile.

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

```python
# stackvet:ignore NDEP-004
```

Or configure a rule override in `.stackvet.toml`:

```toml
[rules]
"NDEP-004" = "off" # or "warning", "error"
```
