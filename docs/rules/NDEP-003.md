# NDEP-003: Unmet Peer Dependency

| Attribute | Value |
|-----------|-------|
| **Rule ID** | `NDEP-003` |
| **Category** | `node` |
| **Default Severity** | `ERROR` |

## Description
Declared package version conflicts with required peer dependency constraints.

## Why This Matters
Detecting and addressing unmet peer dependency prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
Align installed package versions with peer dependency requirements.

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

```python
# stackvet:ignore NDEP-003
```

Or configure a rule override in `.stackvet.toml`:

```toml
[rules]
"NDEP-003" = "off" # or "warning", "error"
```
