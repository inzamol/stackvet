---
name: release-management
description: Automated workflow for preparing, bumping versions, and publishing releases for StackVet. Use whenever the user asks to "make a release", "cut a release", "publish release", or bump project versions.
---

# Release Management & Version Bumping Guide

When asked to make or prepare a release, follow this mandatory semantic versioning and release workflow.

## 1. Semantic Version Bumping Rules
First, get the current version from `Cargo.toml` (`[workspace.package].version`, e.g., `0.1.2`).
Determine the release type based on the changes:

1. **Bug Fix**: Increase the last part (Patch).
   - E.g. `0.1.2` -> `0.1.3`
   - Command: `node scripts/bump_version.js patch`
2. **New Feature**: Increase the middle number (Minor).
   - E.g. `0.1.2` -> `0.2.2` (or `0.2.0`)
   - Command: `node scripts/bump_version.js minor`
3. **Breaking Changes (No Backward Compatibility)**: Increase the first digit (Major).
   - E.g. `0.1.2` -> `1.0.0`
   - Command: `node scripts/bump_version.js major`

## 2. Manifest Synchronization & Validation
The single source of truth for the project version is `Cargo.toml` under `[workspace.package]`.
After updating `Cargo.toml`, synchronize and validate all packages:
```bash
# Sync package.json and packages/npm/package.json
node scripts/sync_version.js

# Verify all manifests are strictly in sync
node scripts/sync_version.js --check
```

Verify that:
- `Cargo.toml` has the target version
- Root `package.json` has the target version
- `packages/npm/package.json` has the target version
- `pyproject.toml` delegates dynamic versioning to the Cargo workspace

## 3. Pre-Release Verification & Smoke Testing
Before committing or publishing the release, run the complete verification suite:
```bash
# Code formatting check
cargo fmt --all --check

# Full test suite
cargo test --all-targets --all-features

# Build release binary
cargo build -p stackvet --release --locked

# Bundle binary into npm package
node packages/npm/scripts/copy-binary.js

# Run cross-platform smoke tests
node scripts/smoke_test.js
```

## 4. Release Creation & Tagging
1. Commit the synchronized version manifests:
   ```bash
   git add Cargo.toml Cargo.lock package.json packages/npm/package.json
   git commit -m "chore(release): bump version to v<VERSION>"
   ```
2. For GitHub Releases and CI/CD distribution:
   - Always target the `main` branch.
   - Tag the release commit:
     ```bash
     git tag v<VERSION>
     git push origin v<VERSION>
     ```
   - Pushing tag `v*` automatically triggers:
     - Multi-platform binary builds & GitHub Release creation (`.github/workflows/release.yml`)
     - PyPI wheel & sdist publish (`.github/workflows/publish-pypi.yml`)
     - npm package publish (`.github/workflows/publish-npm.yml`)
