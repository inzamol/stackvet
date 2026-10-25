# Publishing Guide: npm & PyPI

This guide walks you through publishing **StackVet** to both the **npm Registry** (as `@inzamol/stackvet` for Node.js / JavaScript users) and **PyPI** (as `stackvet` for Python users).

---

## Table of Contents

1. [Version Synchronization](#1-version-synchronization)
2. [Publishing to npm](#2-publishing-to-npm)
   - [Prerequisites](#a-prerequisites-npm)
   - [Automated Publishing via GitHub Actions (Recommended)](#b-automated-publishing-via-github-actions)
   - [Manual Publishing via CLI](#c-manual-publishing-via-cli)
   - [Verifying npm Installation](#d-verifying-npm-installation)
3. [Publishing to PyPI](#3-publishing-to-pypi)
   - [Prerequisites](#a-prerequisites-pypi)
   - [Automated Publishing via GitHub Actions (Recommended)](#b-automated-publishing-via-github-actions-pypi)
   - [Manual Publishing via CLI](#c-manual-publishing-via-cli-pypi)
   - [Verifying PyPI Installation](#d-verifying-pypi-installation)
4. [Troubleshooting & FAQs](#4-troubleshooting-faqs)

---

## 1. Version Synchronization

The single source of truth for the project version is the workspace root [`Cargo.toml`](../Cargo.toml) under `[workspace.package].version` (e.g. `0.3.2`):

- All Rust crates inherit this version via `version.workspace = true`.
- `pyproject.toml` declares dynamic versioning (`dynamic = ["version"]`), delegating version resolution to Maturin from the Cargo workspace.
- `package.json`, `packages/npm/package.json`, and `editors/vscode/package.json` are synchronized to match the Cargo workspace version.

### Option A: Automated Version Bumping (Recommended)
Use the automated helper script to bump the version and synchronize all manifests:

```bash
# Bump according to SemVer rules:
node scripts/bump_version.js patch   # Bug fixes (e.g. 0.3.2 -> 0.3.3)
node scripts/bump_version.js minor   # New features (e.g. 0.3.2 -> 0.4.0)
node scripts/bump_version.js major   # Breaking changes (e.g. 0.3.2 -> 1.0.0)

# Or bump to an explicit target version:
node scripts/bump_version.js <VERSION>
```

This updates `Cargo.toml`, executes `node scripts/sync_version.js`, and verifies manifest consistency.

### Option B: Manual Synchronization
If editing manifests manually, update the root `Cargo.toml`:

```toml
[workspace.package]
version = "<VERSION>" # e.g. "0.3.2"
```

Then synchronize and verify all manifests:

```bash
# Synchronize root package.json, packages/npm/package.json, and editors/vscode/package.json
node scripts/sync_version.js

# Verify all manifests are strictly in sync
node scripts/sync_version.js --check
```

Commit your synchronized version updates:
```bash
git add Cargo.toml Cargo.lock package.json packages/npm/package.json editors/vscode/package.json
git commit -m "chore(release): bump version to v<VERSION>"
```

---

## 2. Publishing to npm

StackVet's npm distribution is located in `packages/npm/` and ships a cross-platform executable runner (`bin/stackvet.js`) bundled with the native Rust binary under the scoped package `@inzamol/stackvet`.

### A. Prerequisites (npm)
1. An active account at [npmjs.com](https://www.npmjs.com/).
2. Enable Two-Factor Authentication (2FA).
3. Authentication setup:
   - **For Automated GitHub Actions (OIDC Trusted Publishing - Recommended)**: Configure npm Trusted Publishing for `@inzamol/stackvet` on npmjs.com linked to your GitHub repository and the `publish-npm.yml` workflow under the `npm` environment.
   - **For Token-based Publishing / Local CLI**: An npm automation token or active login via `npm login`.

---

### B. Automated Publishing via GitHub Actions

The repository includes a ready-to-run workflow at [`.github/workflows/publish-npm.yml`](https://github.com/inzamol/stackvet/blob/main/.github/workflows/publish-npm.yml).

The workflow uses npm's modern **Trusted Publishing (OIDC)** with `--provenance` running under the `npm` environment.

#### Trigger Publication
Create and push a Git release tag:
```bash
git tag v<VERSION> # e.g. v0.3.2
git push origin v<VERSION>
```

GitHub Actions will automatically:
1. Compile the optimized native Rust binary (`cargo build -p stackvet --release --locked`).
2. Bundle the binary and package metadata via `node scripts/build_npm.js`.
3. Validate that `stackvet --version` executes cleanly.
4. Publish `@inzamol/stackvet` to `https://registry.npmjs.org/` with public access and cryptographic provenance.

> **Tip (Dry Run)**: You can also manually trigger this workflow via the **Actions** tab on GitHub with `workflow_dispatch` and check the `dry_run` option to test packaging without publishing.

---

### C. Manual Publishing via CLI

To build and publish manually from your local machine:

```bash
# 1. Build and package the npm distribution
npm run build:npm

# 2. Navigate to the npm package folder
cd packages/npm

# 3. Log into npm (if not already logged in)
npm login

# 4. Publish to registry with public access
npm publish --access public
```

---

### D. Verifying npm Installation

After publishing, test running StackVet directly with `npx` (no prior installation needed):

```bash
# Run instantly via npx without installing
npx @inzamol/stackvet doctor

# Or install globally
npm install -g @inzamol/stackvet
stackvet --version

# Or install as a project devDependency
npm install --save-dev @inzamol/stackvet
# or
pnpm add -D @inzamol/stackvet

# Run the locally installed tool
npx stackvet doctor
# or
pnpm exec stackvet doctor
```

---

## 3. Publishing to PyPI

StackVet uses [Maturin](https://www.maturin.rs/) (`bindings = "bin"`) to build native Python wheels containing the compiled binary for Linux, macOS, and Windows.

### A. Prerequisites (PyPI)
1. An active account at [pypi.org](https://pypi.org/).
2. Enable 2FA on PyPI.

---

### B. Automated Publishing via GitHub Actions (PyPI)

The repository includes a multi-platform build workflow at [`.github/workflows/publish-pypi.yml`](https://github.com/inzamol/stackvet/blob/main/.github/workflows/publish-pypi.yml).

It uses PyPI's modern **Trusted Publishing (OIDC)**, eliminating the need to store long-lived API tokens.

#### Setting up PyPI Trusted Publishing (Zero-Token OIDC)
1. Log in to [pypi.org](https://pypi.org/) and go to **Account Settings** -> **Publishing**.
2. Click **Add a new pending publisher**.
3. Enter:
   - **PyPI Project Name**: `stackvet`
   - **Owner**: Your GitHub username or organization (e.g. `inzamol`)
   - **Repository name**: `stackvet`
   - **Workflow name**: `publish-pypi.yml`
   - **Environment name**: `pypi`
4. Click **Add**.

#### Triggering Publication
Push your version tag:
```bash
git tag v<VERSION> # e.g. v0.3.2
git push origin v<VERSION>
```

GitHub Actions will automatically:
1. Matrix build wheels for:
   - Linux: `x86_64` and `aarch64`
   - Windows: `x64`
   - macOS: `x86_64` and `aarch64` (Apple Silicon)
2. Build the source distribution (`sdist`).
3. Securely upload all artifacts to PyPI via OIDC.

> **Alternative (API Token)**: If you prefer an API token, create a token on PyPI and add it to GitHub Secrets as `PYPI_API_TOKEN`.

---

### C. Manual Publishing via CLI (PyPI)

To manually build and upload the current platform's wheel and sdist:

```bash
# 1. Install maturin and twine
pip install "maturin>=1.5,<2.0" twine

# 2. Build the wheel and source distribution
npm run build:pypi
# Artifacts are written to dist/pypi/

# 3. Verify packages
twine check dist/pypi/*

# 4. Upload to PyPI
twine upload dist/pypi/*
```

---

### D. Verifying PyPI Installation

Test running StackVet via Python tooling:

```bash
# Run instantly with uvx (no install required)
uvx stackvet doctor

# Or install with pip / pipx
pip install stackvet
# or
pipx install stackvet

stackvet --version
```

---

## 4. Troubleshooting & FAQs

### Q: `npm ERR! 403 Forbidden - You do not have permission to publish "@inzamol/stackvet"`
- Ensure you are logged in with an npm account that has write access to the `@inzamol` scope or package.
- If publishing a fork under your own namespace, update `"name": "@<your-username>/stackvet"` in `packages/npm/package.json` and root `package.json`, then run `node scripts/sync_version.js`.

### Q: `HTTPError: 400 Bad Request from PyPI - File already exists`
- PyPI never allows re-uploading an existing version number. If a build failed or needs changes, increment the patch version (e.g. `node scripts/bump_version.js patch` or update root `Cargo.toml`), run `node scripts/sync_version.js`, and push a new tag.

### Q: Windows `error: os error 32 (file in use)` during local cargo build
- This occurs if Windows Defender or a file indexer holds a temporary `.rcgu.o` lock on the build directory.
- StackVet is configured with `.cargo/config.toml` targeting `~/.cargo-target/stackvet` outside watched directories to eliminate lock collisions.
