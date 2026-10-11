#!/usr/bin/env node
const fs = require('fs');
const path = require('path');

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
console.log(`Single source of truth version: ${targetVersion}`);

// 1. Sync root package.json
const rootPkgPath = path.join(root, 'package.json');
if (fs.existsSync(rootPkgPath)) {
  const rootPkg = JSON.parse(fs.readFileSync(rootPkgPath, 'utf8'));
  if (rootPkg.version !== targetVersion) {
    rootPkg.version = targetVersion;
    fs.writeFileSync(rootPkgPath, JSON.stringify(rootPkg, null, 2) + '\n');
    console.log(`Updated root package.json -> ${targetVersion}`);
  }
}

// 2. Sync packages/npm/package.json
const npmPkgPath = path.join(root, 'packages', 'npm', 'package.json');
if (fs.existsSync(npmPkgPath)) {
  const npmPkg = JSON.parse(fs.readFileSync(npmPkgPath, 'utf8'));
  if (npmPkg.version !== targetVersion) {
    npmPkg.version = targetVersion;
    fs.writeFileSync(npmPkgPath, JSON.stringify(npmPkg, null, 2) + '\n');
    console.log(`Updated packages/npm/package.json -> ${targetVersion}`);
  }
}

console.log('Version synchronization complete.');
