# DISC-005: Scan resource limit or repository boundary exceeded

| Attribute | Value |
|-----------|-------|
| **Rule ID** | `DISC-005` |
| **Category** | `discovery` |
| **Default Severity** | `WARNING` |

## Description
A file, symlink, or directory structure exceeded the configured scanner limits (such as max file size, max depth, or boundary escaping).

## Why This Matters
Detecting and addressing scan resource limit or repository boundary exceeded prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
Increase scanner limits in stackvet.toml or exclude the problematic path using .stackvetignore.

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

```python
# stackvet:ignore DISC-005
```

Or configure a rule override in `.stackvet.toml`:

```toml
[rules]
"DISC-005" = "off" # or "warning", "error"
```
