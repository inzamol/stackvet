# Project Workflow Guidelines

## GitHub Pull Requests & Issues
- **Never close GitHub issues directly**: Always create a Pull Request with `Closes #<issue_number>` to resolve and close issues upon merge.
- **Target Branch**: Always create and submit Pull Requests targeting the `main` branch.

## CodeRabbit Reviews
- CodeRabbit configuration in `.coderabbit.yaml` must exclude test fixtures:
  ```yaml
  reviews:
    path_filters:
      - "!**/fixtures/**"
      - "!tests/fixtures/**"
  ```

## Release & Versioning Guidelines
Whenever asked to make or prepare a release:
- Read current version from `Cargo.toml` (`[workspace.package].version`, e.g., `0.1.2`).
- Apply the semantic version bump:
  1. **Bug fix**: Increase the last part (e.g. to `0.1.3`).
  2. **New feature**: Increase the middle number (e.g. to `0.2.2` or `0.2.0`).
  3. **Breaking changes (no backward compatibility)**: Increase the first digit (e.g. to `1.0.0`).
- Always synchronize manifests with `node scripts/sync_version.js` and verify with `node scripts/sync_version.js --check`.
- Always run pre-release validation: `cargo test --all-targets --all-features` and `node scripts/smoke_test.js`.

## Core Engineering Best Practices

### 1. Diagnostic Rule Standards & Accuracy
- **Rule ID Taxonomy**: Every diagnostic rule must use a standard 3-letter prefix + 3 digits (e.g. `DEP-xxx`, `NDEP-xxx`, `IMP-xxx`, `ENV-xxx`, `DOC-xxx`, `FAP-xxx`, `SQL-xxx`, `DJG-xxx`, `CEL-xxx`, `RCT-xxx`, `NXT-xxx`).
- **Catalog Registration**: Every rule must be registered with complete metadata in `crates/stackvet-core/src/rules.rs` (`id`, `name`, `description`, `category`, `default_severity`, `rationale`, and `remediation`).
- **Zero False Positives Principle**: Analyzers must include paired positive (`tests/fixtures/regression/positive/`) and negative fixtures (`tests/fixtures/regression/negative/`), verified via `diagnostic_accuracy_test.rs`.

### 2. AST-First Parsing (No Fragile RegEx)
- Never use regular expressions for code semantics.
- Use `ruff_python_parser` and `ruff_python_ast` for Python AST traversal.
- Use `oxc_parser` and `oxc_ast` for JavaScript/TypeScript AST traversal.
- Always normalize Python package names per PEP 503 (`name.replace('_', '-').to_lowercase()`).

### 3. Format-Preserving, Atomic Remediation Safety
- **Preserve Formatting & Comments**: Never deserialize TOML into generic structs and dump it back with `toml::to_string`. Always use `toml_edit` to preserve user comments, indentation, and structure.
- **Idempotency**: Running `stackvet fix` repeatedly must produce zero diff after initial remediation.
- **Transactional Atomicity**: Write changes to a temporary file before atomic rename, and record persistent rollback snapshots under `.stackvet/rollbacks/`.

### 4. Performance & Concurrency
- Utilize Rayon (`par_iter()`) or Tokio tasks for repository traversal and multi-file analysis.
- Always use `ignore::WalkBuilder` to respect `.gitignore`, `.ignore`, and standard build artifact directories (`node_modules/`, `target/`, `.venv/`, `.git/`).

### 5. Pre-Push Validation Checklist
Before opening or updating a Pull Request, always execute:
```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features
cargo test --all-targets --all-features
node scripts/sync_version.js --check
node scripts/smoke_test.js
```
