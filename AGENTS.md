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

