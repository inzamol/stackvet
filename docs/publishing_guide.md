# Publishing Guide: npm & PyPI

This guide walks you through publishing **StackVet** to both the **npm Registry** (for Node.js / JavaScript users) and **PyPI** (for Python users).

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

Before releasing a new version, ensure the version number is synchronized across the project manifests:

1. `crates/stackvet-cli/Cargo.toml`: `version = "0.1.0"`
2. `packages/npm/package.json`: `"version": "0.1.0"`
3. `pyproject.toml`: `version = "0.1.0"`

Commit your version updates:
```bash
git add .
git commit -m "chore: bump version to 0.1.0"
```

---

## 2. Publishing to npm

StackVet's npm distribution is located in `packages/npm/` and ships a cross-platform executable runner (`bin/stackvet.js`) bundled with the native Rust binary.

### A. Prerequisites (npm)
1. An active account at [npmjs.com](https://www.npmjs.com/).
2. Enable Two-Factor Authentication (2FA).
3. Generate an **Automation Token**:
   - Go to **npmjs.com** -> Click your Profile -> **Access Tokens**.
   - Click **Generate New Token** -> Choose **Classic Token** -> Select **Automation**.
   - Copy the generated token (starts with `npm_...`).

---

### B. Automated Publishing via GitHub Actions

The repository includes a ready-to-run workflow at [`.github/workflows/publish-npm.yml`](https://github.com/inzamol/stackvet/blob/main/.github/workflows/publish-npm.yml).

#### Step 1: Configure GitHub Secret
1. Open your repository on GitHub.
2. Go to **Settings** -> **Secrets and variables** -> **Actions**.
3. Click **New repository secret**.
4. Set:
   - **Name**: `NPM_TOKEN`
   - **Secret**: Paste your npm automation token.
5. Click **Add secret**.

#### Step 2: Trigger Publication
Create and push a Git release tag:
```bash
git tag v0.1.0
git push origin v0.1.0
```

GitHub Actions will automatically:
1. Compile the optimized native Rust binary.
2. Bundle the binary into the npm package structure.
3. Validate that `stackvet --version` executes cleanly.
4. Publish `stackvet` to `https://registry.npmjs.org/` with public access.

> **Tip (Dry Run)**: You can also manually trigger this workflow via the **Actions** tab on GitHub and check the `dry_run` option to test packaging without publishing.

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

# 4. Publish to registry
npm publish --access public
```

---

### D. Verifying npm Installation

After publishing, test running StackVet directly with `npx` (no prior installation needed):

```bash
# Run instantly via npx
npx stackvet doctor

# Or install globally
npm install -g stackvet
stackvet --version

# Or install as a project devDependency
pnpm add -D stackvet
# or
npm install --save-dev stackvet
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
git tag v0.1.0
git push origin v0.1.0
```

GitHub Actions will automatically:
1. Matrix build wheels for:
   - Linux: `x86_64` and `aarch64`
   - Windows: `x86_64`
   - macOS: `x86_64` and `aarch64` (Apple Silicon)
2. Build the source distribution (`sdist`).
3. Securely upload all artifacts to PyPI via OIDC.

> **Alternative (API Token)**: If you prefer an API token, create a token on PyPI and add it to GitHub Secrets as `PYPI_API_TOKEN`.

---

### C. Manual Publishing via CLI (PyPI)

To manually build and upload the current platform's wheel and sdist:

```bash
# 1. Install maturin and twine
pip install maturin twine

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

### Q: `npm ERR! 403 Forbidden - You do not have permission to publish "stackvet"`
- The package name `stackvet` may already be registered or reserved by someone else on npm.
- If the name is taken, you can publish under an npm organization scope (e.g. `@yourorg/stackvet`) by updating `"name": "@yourorg/stackvet"` in `packages/npm/package.json`.

### Q: `HTTPError: 400 Bad Request from PyPI - File already exists`
- PyPI never allows re-uploading an existing version number. If a build failed or needs changes, increment the patch version (e.g. `0.1.1`) in `pyproject.toml` and `Cargo.toml`.

### Q: Windows `error: os error 32 (file in use)` during local cargo build
- This occurs if Windows Defender or a file indexer holds a temporary `.rcgu.o` lock on the build directory.
- StackVet is configured with `.cargo/config.toml` targeting `~/.cargo-target/stackvet` outside watched directories to eliminate lock collisions.
