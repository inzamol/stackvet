# DISC-001: Unreadable source file

| Attribute | Value |
|-----------|-------|
| **Rule ID** | `DISC-001` |
| **Category** | `discovery` |
| **Default Severity** | `WARNING` |

## Description
A source file could not be read during project discovery due to permissions or encoding errors.

## Why This Matters
Detecting and addressing unreadable source file prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
Check file read permissions and ensure file is encoded in valid UTF-8.

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

```python
# stackvet:ignore DISC-001
```

Or configure a rule override in `.stackvet.toml`:

```toml
[rules]
"DISC-001" = "off" # or "warning", "error"
```
