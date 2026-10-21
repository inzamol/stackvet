# NDEP-002: Unused Node Package

| Attribute | Value |
|-----------|-------|
| **Rule ID** | `NDEP-002` |
| **Category** | `node` |
| **Default Severity** | `WARN` |

## Description
Production dependency in package.json is never imported in any source files.

## Why This Matters
Detecting and addressing unused node package prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
Remove unused package from dependencies or move to devDependencies.

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

```python
# stackvet:ignore NDEP-002
```

Or configure a rule override in `.stackvet.toml`:

```toml
[rules]
"NDEP-002" = "off" # or "warning", "error"
```
