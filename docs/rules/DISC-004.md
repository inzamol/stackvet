# DISC-004: Malformed project manifest or configuration file

| Attribute | Value |
|-----------|-------|
| **Rule ID** | `DISC-004` |
| **Category** | `discovery` |
| **Default Severity** | `ERROR` |

## Description
A project manifest or configuration file contains invalid syntax, corrupted structure, or unreadable content.

## Why This Matters
Detecting and addressing malformed project manifest or configuration file prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
Validate and correct the syntax of the manifest file (e.g. pyproject.toml, package.json, or tsconfig.json).

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

```python
# stackvet:ignore DISC-004
```

Or configure a rule override in `.stackvet.toml`:

```toml
[rules]
"DISC-004" = "off" # or "warning", "error"
```
