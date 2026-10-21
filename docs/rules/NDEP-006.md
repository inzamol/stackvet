# NDEP-006: Inconsistent Dependency Version

| Attribute | Value |
|-----------|-------|
| **Rule ID** | `NDEP-006` |
| **Category** | `node` |
| **Default Severity** | `WARN` |

## Description
Dependency versions across packages, workspace members, or lockfile resolutions are inconsistent or conflicting.

## Why This Matters
Detecting and addressing inconsistent dependency version prevents runtime failures, dependency conflicts, or configuration drift in production environments.

## Remediation
Align dependency version constraints across packages and synchronize the lockfile.

## Configuration & Suppression
To suppress this rule for a specific file or line, add an inline comment:

```python
# stackvet:ignore NDEP-006
```

Or configure a rule override in `.stackvet.toml`:

```toml
[rules]
"NDEP-006" = "off" # or "warning", "error"
```
