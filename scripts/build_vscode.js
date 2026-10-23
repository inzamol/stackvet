#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const os = require('os');
const { execSync } = require('child_process');

const root = path.dirname(__dirname);
const vscodeDir = path.join(root, 'editors', 'vscode');
const distDir = path.join(root, 'dist', 'vscode');
const binDir = path.join(vscodeDir, 'bin');

if (!fs.existsSync(distDir)) {
  fs.mkdirSync(distDir, { recursive: true });
}
if (!fs.existsSync(binDir)) {
  fs.mkdirSync(binDir, { recursive: true });
}

console.log('1. Syncing version from Cargo.toml...');
execSync('node scripts/sync_version.js', { cwd: root, stdio: 'inherit' });

console.log('2. Building native Rust binary in release mode...');
execSync('cargo build -p stackvet --release --locked', { cwd: root, stdio: 'inherit' });

console.log('3. Copying native binary to editors/vscode/bin/...');
const isWindows = process.platform === 'win32';
const binName = isWindows ? 'stackvet.exe' : 'stackvet';
const candidatePaths = [
  path.join(root, 'target', 'release', binName),
  path.join(os.homedir(), '.cargo-target', 'stackvet', 'release', binName),
  path.join(root, 'packages', 'npm', 'bin', binName),
];

let copied = false;
const dest = path.join(binDir, binName);
for (const src of candidatePaths) {
  if (fs.existsSync(src)) {
    console.log(`   Found binary at: ${src}`);
    fs.copyFileSync(src, dest);
    if (!isWindows) {
      try {
        fs.chmodSync(dest, 0o755);
      } catch (_) {}
    }
    copied = true;
    break;
  }
}

if (!copied && !fs.existsSync(dest)) {
  console.error(`ERROR: Could not find ${binName} in candidate paths.`);
  process.exit(1);
}

console.log('4. Copying LICENSE to editors/vscode/...');
fs.copyFileSync(path.join(root, 'LICENSE'), path.join(vscodeDir, 'LICENSE'));

console.log('5. Compiling VS Code extension...');
execSync('npm run compile', { cwd: vscodeDir, stdio: 'inherit' });

console.log('6. Packaging VSIX with bundled binary...');
// Clean existing .vsix files in vscodeDir and distDir
for (const f of fs.readdirSync(vscodeDir).filter((x) => x.endsWith('.vsix'))) {
  fs.unlinkSync(path.join(vscodeDir, f));
}
for (const f of fs.readdirSync(distDir).filter((x) => x.endsWith('.vsix'))) {
  fs.unlinkSync(path.join(distDir, f));
}

execSync('npx @vscode/vsce package --no-git-tag-version', { cwd: vscodeDir, stdio: 'inherit' });

// Copy generated .vsix to dist/vscode
const generatedVsix = fs.readdirSync(vscodeDir).find((f) => f.endsWith('.vsix'));
if (generatedVsix) {
  const vsixSrc = path.join(vscodeDir, generatedVsix);
  const vsixDest = path.join(distDir, generatedVsix);
  fs.copyFileSync(vsixSrc, vsixDest);
  const stats = fs.statSync(vsixDest);
  console.log(`\n✓ Successfully packaged extension with bundled binary:`);
  console.log(`  File: ${vsixDest}`);
  console.log(`  Size: ${(stats.size / 1024 / 1024).toFixed(2)} MB`);
}
