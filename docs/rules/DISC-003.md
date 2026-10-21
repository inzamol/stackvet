# DISC-003: JavaScript or TypeScript syntax error in source file

| Attribute | Value |
|-----------|-------|
| **Rule ID** | `DISC-003` |
| **Category** | `discovery` |
| **Default Severity** | `WARNING` |

## Description
A JavaScript or TypeScript source file contains invalid syntax and could not be parsed into an AST.

## Why This Matters
Detecting and addressing javascript or typescript syntax error in source file prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
Fix the syntax error in the source file.

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

```python
# stackvet:ignore DISC-003
```

Or configure a rule override in `.stackvet.toml`:

```toml
[rules]
"DISC-003" = "off" # or "warning", "error"
```
