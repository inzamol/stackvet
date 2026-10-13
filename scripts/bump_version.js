#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

const root = path.resolve(__dirname, '..');
const cargoPath = path.join(root, 'Cargo.toml');

if (!fs.existsSync(cargoPath)) {
  console.error(`Cargo.toml not found at: ${cargoPath}`);
  process.exit(1);
}

const type = process.argv[2]; // 'patch', 'minor', 'major', or specific version
if (!type) {
  console.error('Usage: node scripts/bump_version.js <patch|minor|major|x.y.z>');
  console.error('  patch: Bug fixes (e.g. 0.1.2 -> 0.1.3)');
  console.error('  minor: New features (e.g. 0.1.2 -> 0.2.2 or 0.2.0)');
  console.error('  major: Breaking changes without backward compatibility (e.g. 1.0.0)');
  process.exit(1);
}

const cargoContent = fs.readFileSync(cargoPath, 'utf8');
const match = cargoContent.match(/(\[workspace\.package\][\s\S]*?version\s*=\s*")([^"]+)(")/);

if (!match) {
  console.error('Could not find [workspace.package] version in Cargo.toml');
  process.exit(1);
}

const currentVersion = match[2];
const parts = currentVersion.split('.').map(Number);
if (parts.length !== 3 || parts.some(isNaN)) {
  console.error(`Invalid semver in Cargo.toml: "${currentVersion}"`);
  process.exit(1);
}

let nextVersion = '';
if (type === 'patch') {
  // 1. Increase last part for bug fix (e.g. 0.1.2 -> 0.1.3)
  nextVersion = `${parts[0]}.${parts[1]}.${parts[2] + 1}`;
} else if (type === 'minor') {
  // 2. Increase middle number in case new feature
  nextVersion = `${parts[0]}.${parts[1] + 1}.${parts[2]}`;
} else if (type === 'major') {
  // 3. Increase first digit in case only breaking changes, no backward compatibility
  nextVersion = `${parts[0] + 1}.0.0`;
} else if (/^\d+\.\d+\.\d+$/.test(type)) {
  nextVersion = type;
} else {
  console.error(`Unknown bump type: ${type}. Use 'patch', 'minor', 'major', or explicit version.`);
  process.exit(1);
}

console.log(`Bumping version: ${currentVersion} -> ${nextVersion}`);

const updatedCargo = cargoContent.replace(
  /(\[workspace\.package\][\s\S]*?version\s*=\s*")[^"]+(")/,
  `$1${nextVersion}$2`
);
fs.writeFileSync(cargoPath, updatedCargo, 'utf8');

// Synchronize all manifests
execSync('node scripts/sync_version.js', { cwd: root, stdio: 'inherit' });
execSync('node scripts/sync_version.js --check', { cwd: root, stdio: 'inherit' });

console.log(`\nSuccessfully bumped version to ${nextVersion}!`);
