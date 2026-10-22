# Contributing Diagnostic Rules to StackVet

StackVet enforces strict standards for all diagnostic rules to ensure deterministic execution, actionable guidance, and zero false positives. Follow this guide when proposing or implementing a new diagnostic rule.

---

## 1. Rule ID Taxonomy Standards

Every diagnostic rule must use a standardized 2 to 5 letter uppercase prefix, a hyphen (`-`), and a 3-digit zero-padded number (e.g., `DEP-008`, `RCT-006`):

| Prefix | Domain / Technology | Example Rules |
|--------|---------------------|---------------|
| `DEP-` | Python dependency declarations & resolution | `DEP-001`, `DEP-002` |
| `NDEP-`| Node.js / JavaScript / TypeScript dependencies | `NDEP-001`, `NDEP-004` |
| `IMP-` | Python imports & module structure | `IMP-001`, `IMP-004` |
| `ARC-` | Architectural layer constraints | `ARC-001`, `ARC-002` |
| `SEC-` | Vulnerability & security posture | `SEC-001`, `SEC-002` |
| `ENV-` | Runtime & environment drift | `ENV-001`, `ENV-003` |
| `DISC-`| Discovery, file parsing & scanner limits | `DISC-001`, `DISC-005` |
| `CI-`  | CI/CD workflow security & hygiene | `CI-001`, `CI-005` |
| `DJG-` | Django framework best practices | `DJG-001`, `DJG-006` |
| `CEL-` | Celery task & worker safety | `CEL-001`, `CEL-004` |
| `FAP-` | FastAPI API hygiene & async safety | `FAP-001`, `FAP-005` |
| `SQL-` | SQLAlchemy session & query optimization | `SQL-001`, `SQL-030` |
| `RCT-` | React component & hooks rules | `RCT-001`, `RCT-005` |
| `NXT-` | Next.js App Router & server/client safety | `NXT-001`, `NXT-002` |

---

## 2. Catalog Registration

All rules must be registered in `crates/stackvet-core/src/rules_catalog.rs` inside the `ALL_RULES` array:

```rust
RuleMeta {
    id: "DEP-008",
    category: "dependencies",
    title: "Pinned dependency conflicts with runtime platform",
    severity: "ERROR", // HINT, INFO, WARNING, WARN, ERROR, or CRITICAL
    description: "A dependency declaration specifies platform constraints that exclude the current target environment.",
    remediation: "Update the environment markers in pyproject.toml to accommodate the target platforms.",
}
```

Rules are validated at compile and test time using `validate_rule(&rule)`. Empty fields or non-standard IDs will fail automated verification.

---

## 3. Zero False Positives & Paired Fixtures

Every rule implementation must include paired positive and negative regression test fixtures:

1. **Positive Fixture** (`tests/fixtures/regression/positive/<rule_id>_*.py` or appropriate extension):
   - A minimal reproducer demonstrating the exact defect triggering the rule.
2. **Negative Fixture** (`tests/fixtures/regression/negative/<rule_id>_*.py` or appropriate extension):
   - A idiomatic, valid code pattern that must **never** trigger the diagnostic.

These fixtures are continuously executed by `tests/diagnostic_accuracy_test.rs`.

---

## 4. AST-First Parsing (No RegEx for Code Semantics)

StackVet strictly prohibits using regular expressions to parse code logic or import statements:
- Use `ruff_python_parser` and `ruff_python_ast` for Python AST traversal.
- Use `oxc_parser` and `oxc_ast` for JavaScript/TypeScript AST traversal.
- Python package names must always be normalized according to PEP 503 (`name.replace('_', '-').to_lowercase()`).

---

## 5. Standalone & Summary Documentation

Every rule requires two documentation touchpoints:

1. **Individual Rule Document** (`docs/rules/<RULE_ID>.md`):
   ```markdown
   # <RULE_ID>: <Title>

   | Attribute | Value |
   |-----------|-------|
   | **Rule ID** | `<RULE_ID>` |
   | **Category** | `<category>` |
   | **Default Severity** | `<SEVERITY>` |

   ## Description
   <Detailed explanation of the issue.>

   ## Why This Matters
   <Impact on production stability, performance, security, or maintainability.>

   ## Remediation
   <Step-by-step instructions and code snippets showing how to fix.>

   ## Configuration & Suppression
   <Inline comment suppression syntax and .stackvet.toml override examples.>
   ```

2. **Summary Catalog** (`docs/rules.md`):
   Add the rule under its category section with ID, severity, description, and remediation.

---

## 6. Pre-Submission Verification

Run the rule audit script and test suite before submitting a Pull Request:

```bash
# Automated rule catalog & documentation audit
node scripts/audit_rules.js

# Rust catalog integrity & documentation test
cargo test -p stackvet-core --test rule_audit_test

# Full suite verification
cargo fmt --all --check
cargo clippy --all-targets --all-features
cargo test --all-targets --all-features
```
