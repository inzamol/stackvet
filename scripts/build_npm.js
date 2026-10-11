#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const root = path.dirname(__dirname);
const outDir = path.join(root, 'dist', 'npm');
const npmPkgDir = path.join(root, 'packages', 'npm');

if (!fs.existsSync(outDir)) {
  fs.mkdirSync(outDir, { recursive: true });
}

console.log('Syncing version from Cargo.toml...');
execSync('node scripts/sync_version.js', { cwd: root, stdio: 'inherit' });

console.log('Building native Rust binary in release mode...');
execSync('cargo build -p stackvet --release --locked', { cwd: root, stdio: 'inherit' });

console.log('Syncing README.md and LICENSE to npm package...');
fs.copyFileSync(path.join(root, 'README.md'), path.join(npmPkgDir, 'README.md'));
fs.copyFileSync(path.join(root, 'LICENSE'), path.join(npmPkgDir, 'LICENSE'));

console.log('Copying binary...');
execSync('node scripts/copy-binary.js', { cwd: npmPkgDir, stdio: 'inherit' });

console.log('Packaging npm tarball...');
execSync('npm pack --pack-destination ../../dist/npm', { cwd: npmPkgDir, stdio: 'inherit' });

console.log(`\nArtifacts generated in ${outDir}:`);
for (const f of fs.readdirSync(outDir)) {
  console.log(` - ${f}`);
}
