# NDEP-005: DevDependency in Production

| Attribute | Value |
|-----------|-------|
| **Rule ID** | `NDEP-005` |
| **Category** | `node` |
| **Default Severity** | `ERROR` |

## Description
Package from devDependencies is imported inside production source files.

## Why This Matters
Detecting and addressing devdependency in production prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
Move package from devDependencies to dependencies in package.json.

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

```python
# stackvet:ignore NDEP-005
```

Or configure a rule override in `.stackvet.toml`:

```toml
[rules]
"NDEP-005" = "off" # or "warning", "error"
```
