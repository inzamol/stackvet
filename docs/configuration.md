# Project Configuration Guide

StackVet automatically discovers and loads configuration from any of the following files in your project root:
1. `stackvet.toml`
2. `.stackvet.toml`
3. `pyproject.toml` (under the `[tool.stackvet]` table)

You can also explicitly specify a configuration file using `--config <PATH>`:
```bash
stackvet --config ./configs/custom-stackvet.toml check
```

---

## 1. Configuration Schema

Below is an annotated example of a complete `stackvet.toml` or `.stackvet.toml` file:

```toml
[project]
name = "my-application"
min_score = 80
strict = false

[thresholds]
# Minimum severity that triggers CI failure: "hint", "info", "warning", "error", "critical"
fail_on = "error"
# Minimum health score required (0-100)
min_score = 80

[ci]
baseline = ".stackvet-baseline.json"
fail_on = "error"
strict = false

[rules]
# Simple override syntax: set custom severity or disable rules
DEP-001 = "warning"
ARC-001 = "error"
CEL-001 = "disabled"   # or "off"

# Detailed table syntax with explicit enable/disable flag
[rules.FAP-001]
severity = "error"
enabled = true
```

---

## 2. Configuring via `pyproject.toml`

For Python projects, you can embed your StackVet configuration directly in `pyproject.toml`:

```toml
[tool.stackvet]
min_severity = "warning"
fail_on = "error"
min_score = 80

[tool.stackvet.rules]
DEP-001 = "warning"
CEL-001 = "off"
```

---

## 3. Rule Overrides & Disabling

You can customize how individual diagnostic rules are evaluated:

| Value | Behavior |
|---|---|
| `"critical"` | Treats finding as Critical (-20 health score points, blocks CI if `fail_on` is critical/error/warning/info) |
| `"error"` | Treats finding as Error (-8 health score points, blocks CI if `fail_on` is error/warning/info) |
| `"warning"` / `"warn"` | Treats finding as Warning (-2 health score points, blocks CI if `--strict` or `fail_on = "warning"`) |
| `"info"` | Informational observation (0 health score points deduction) |
| `"hint"` | Advisory observation (0 health score points deduction) |
| `"disabled"` / `"off"` | Disables the rule completely; omitted from scans, reports, and health score deductions |

### Validation & Zero False Positives
StackVet rigorously validates all rule IDs against the 132-rule catalog:
- If an unknown rule ID (e.g. `NONEXISTENT-999`) is declared in `[rules]`, StackVet exits with **Exit Code 2 (Configuration Error)** and provides a clear error message.
- If an unsupported severity string (e.g. `"catastrophic"`) is provided, StackVet exits with **Exit Code 2**.

---

## 4. Precedence Hierarchy

StackVet resolves settings deterministically using a strict 3-tier precedence hierarchy:

```text
┌─────────────────────────────────────────────────────────┐
│ 1. CLI Arguments (--fail-on, --strict, --min-score)     │  (Highest precedence)
├─────────────────────────────────────────────────────────┤
│ 2. Project Config (stackvet.toml / pyproject.toml)      │
├─────────────────────────────────────────────────────────┤
│ 3. Built-in Defaults (fail_on = Error, min_score = None)│  (Lowest precedence)
└─────────────────────────────────────────────────────────┘
```

For example:
- If `fail_on = "warning"` is configured in `stackvet.toml`, running `stackvet check` will fail on warnings.
- Passing `stackvet check --fail-on critical` overrides the configuration file for that specific invocation.
