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
