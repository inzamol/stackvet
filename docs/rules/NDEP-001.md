# NDEP-001: Missing Node Package

| Attribute | Value |
|-----------|-------|
| **Rule ID** | `NDEP-001` |
| **Category** | `node` |
| **Default Severity** | `ERROR` |

## Description
Imported package in .ts/.tsx/.js is missing from package.json dependencies.

## Why This Matters
Detecting and addressing missing node package prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
Run 'pnpm add <pkg>' or 'npm install <pkg>'.

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

```python
# stackvet:ignore NDEP-001
```

Or configure a rule override in `.stackvet.toml`:

```toml
[rules]
"NDEP-001" = "off" # or "warning", "error"
```
