#!/usr/bin/env node
const fs = require('fs');
const path = require('path');

const isCheckMode = process.argv.includes('--check');

const root = path.resolve(__dirname, '..');
const cargoPath = path.join(root, 'Cargo.toml');

if (!fs.existsSync(cargoPath)) {
  console.error(`Cargo.toml not found at: ${cargoPath}`);
  process.exit(1);
}

const cargoContent = fs.readFileSync(cargoPath, 'utf8');
const match = cargoContent.match(/\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"/);

if (!match) {
  console.error('Could not find version in [workspace.package] of root Cargo.toml');
  process.exit(1);
}

const targetVersion = match[1];

const rootPkgPath = path.join(root, 'package.json');
const npmPkgPath = path.join(root, 'packages', 'npm', 'package.json');
const vscodePkgPath = path.join(root, 'editors', 'vscode', 'package.json');
const pyprojectPath = path.join(root, 'pyproject.toml');

if (isCheckMode) {
  console.log(`Verifying version consistency against Cargo.toml: ${targetVersion}`);
  let hasError = false;

  if (fs.existsSync(rootPkgPath)) {
    const rootPkg = JSON.parse(fs.readFileSync(rootPkgPath, 'utf8'));
    if (rootPkg.version !== targetVersion) {
      console.error(`✗ Mismatch in package.json: expected "${targetVersion}", found "${rootPkg.version}"`);
      hasError = true;
    } else {
      console.log(`✓ package.json matches (${targetVersion})`);
    }
  } else {
    console.error(`✗ Missing package.json at: ${rootPkgPath}`);
    hasError = true;
  }

  if (fs.existsSync(npmPkgPath)) {
    const npmPkg = JSON.parse(fs.readFileSync(npmPkgPath, 'utf8'));
    if (npmPkg.version !== targetVersion) {
      console.error(`✗ Mismatch in packages/npm/package.json: expected "${targetVersion}", found "${npmPkg.version}"`);
      hasError = true;
    } else {
      console.log(`✓ packages/npm/package.json matches (${targetVersion})`);
    }
  } else {
    console.error(`✗ Missing packages/npm/package.json at: ${npmPkgPath}`);
    hasError = true;
  }

  if (fs.existsSync(vscodePkgPath)) {
    const vscodePkg = JSON.parse(fs.readFileSync(vscodePkgPath, 'utf8'));
    if (vscodePkg.version !== targetVersion) {
      console.error(`✗ Mismatch in editors/vscode/package.json: expected "${targetVersion}", found "${vscodePkg.version}"`);
      hasError = true;
    } else {
      console.log(`✓ editors/vscode/package.json matches (${targetVersion})`);
    }
  }

  if (fs.existsSync(pyprojectPath)) {
    const pyContent = fs.readFileSync(pyprojectPath, 'utf8');
    if (!pyContent.includes('dynamic = ["version"]') && !pyContent.includes(`version = "${targetVersion}"`)) {
      console.error(`✗ pyproject.toml does not declare dynamic version or version "${targetVersion}"`);
      hasError = true;
    } else {
      console.log(`✓ pyproject.toml delegates version to Cargo workspace`);
    }
  }

  if (hasError) {
    console.error('\nVersion check FAILED. Run "node scripts/sync_version.js" to fix.');
    process.exit(1);
  }

  console.log('\n✓ Version consistency check PASSED.');
  process.exit(0);
}

console.log(`Single source of truth version: ${targetVersion}`);

// 1. Sync root package.json
if (fs.existsSync(rootPkgPath)) {
  const rootPkg = JSON.parse(fs.readFileSync(rootPkgPath, 'utf8'));
  if (rootPkg.version !== targetVersion) {
    rootPkg.version = targetVersion;
    fs.writeFileSync(rootPkgPath, JSON.stringify(rootPkg, null, 2) + '\n');
    console.log(`Updated root package.json -> ${targetVersion}`);
  }
}

// 2. Sync packages/npm/package.json
if (fs.existsSync(npmPkgPath)) {
  const npmPkg = JSON.parse(fs.readFileSync(npmPkgPath, 'utf8'));
  if (npmPkg.version !== targetVersion) {
    npmPkg.version = targetVersion;
    fs.writeFileSync(npmPkgPath, JSON.stringify(npmPkg, null, 2) + '\n');
    console.log(`Updated packages/npm/package.json -> ${targetVersion}`);
  }
}

// 3. Sync editors/vscode/package.json
if (fs.existsSync(vscodePkgPath)) {
  const vscodePkg = JSON.parse(fs.readFileSync(vscodePkgPath, 'utf8'));
  if (vscodePkg.version !== targetVersion) {
    vscodePkg.version = targetVersion;
    fs.writeFileSync(vscodePkgPath, JSON.stringify(vscodePkg, null, 2) + '\n');
    console.log(`Updated editors/vscode/package.json -> ${targetVersion}`);
  }
}

console.log('Version synchronization complete.');
