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

// Resolve Cargo's active target directory
let activeTargetDir = process.env.CARGO_TARGET_DIR;
if (activeTargetDir) {
  if (!path.isAbsolute(activeTargetDir)) {
    activeTargetDir = path.resolve(root, activeTargetDir);
  }
} else {
  try {
    const metadataRaw = execSync('cargo metadata --format-version 1 --no-deps', {
      cwd: root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    const metadata = JSON.parse(metadataRaw);
    activeTargetDir = metadata.target_directory;
  } catch (_) {
    activeTargetDir = path.join(root, 'target');
  }
}

const activeBinary = path.join(activeTargetDir, 'release', binName);
let copied = false;
const dest = path.join(binDir, binName);

if (fs.existsSync(activeBinary)) {
  console.log(`   Found newly built binary at active target: ${activeBinary}`);
  fs.copyFileSync(activeBinary, dest);
  copied = true;
} else {
  const candidatePaths = [
    path.join(root, 'target', 'release', binName),
    path.join(os.homedir(), '.cargo-target', 'stackvet', 'release', binName),
    path.join(root, 'packages', 'npm', 'bin', binName),
  ];
  for (const src of candidatePaths) {
    if (fs.existsSync(src)) {
      console.log(`   Found fallback binary at: ${src}`);
      fs.copyFileSync(src, dest);
      copied = true;
      break;
    }
  }
}

if (!copied && !fs.existsSync(dest)) {
  console.error(`ERROR: Could not find ${binName} in candidate paths.`);
  process.exit(1);
}

if (!isWindows) {
  try {
    fs.chmodSync(dest, 0o755);
  } catch (_) {}
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

// Map platform-arch to supported VSCE platform target
const platformMap = {
  'win32-x64': 'win32-x64',
  'win32-arm64': 'win32-arm64',
  'linux-x64': 'linux-x64',
  'linux-arm64': 'linux-arm64',
  'darwin-x64': 'darwin-x64',
  'darwin-arm64': 'darwin-arm64',
};

const targetArg = process.argv.find((arg) => arg.startsWith('--target='));
const targetIdx = process.argv.indexOf('--target');
const targetFromArg = targetArg
  ? targetArg.split('=')[1]
  : targetIdx !== -1 && targetIdx + 1 < process.argv.length
  ? process.argv[targetIdx + 1]
  : undefined;

const hostPlatformKey = `${process.platform}-${process.arch}`;
const hostPlatform = platformMap[hostPlatformKey];
const currentTarget = targetFromArg || hostPlatform;

// Validate target platform against the binary built and copied
if (currentTarget && hostPlatform && currentTarget !== hostPlatform) {
  console.error(`ERROR: Target mismatch. VSIX target is '${currentTarget}', but bundled binary was built for '${hostPlatform}'.`);
  process.exit(1);
}

const vsceTargetFlag = currentTarget ? ` --target ${currentTarget}` : '';
const pkgJson = JSON.parse(fs.readFileSync(path.join(vscodeDir, 'package.json'), 'utf8'));
const vsixFilename = currentTarget
  ? `${pkgJson.name}-${pkgJson.version}-${currentTarget}.vsix`
  : `${pkgJson.name}-${pkgJson.version}.vsix`;
const vsixSrc = path.join(vscodeDir, vsixFilename);
const vsixDest = path.join(distDir, vsixFilename);

// Ensure no stale VSIX candidate exists before packaging
if (fs.existsSync(vsixSrc)) {
  fs.unlinkSync(vsixSrc);
}
if (fs.existsSync(vsixDest)) {
  fs.unlinkSync(vsixDest);
}

console.log(`   Targeting platform: ${currentTarget || 'universal'}`);
execSync(`npx @vscode/vsce package${vsceTargetFlag} --out "${vsixSrc}" --no-git-tag-version`, { cwd: vscodeDir, stdio: 'inherit' });

if (fs.existsSync(vsixSrc)) {
  fs.copyFileSync(vsixSrc, vsixDest);
  const stats = fs.statSync(vsixDest);
  console.log(`\n✓ Successfully packaged platform-targeted extension with bundled binary:`);
  console.log(`  File: ${vsixDest}`);
  console.log(`  Size: ${(stats.size / 1024 / 1024).toFixed(2)} MB`);
} else {
  console.error(`ERROR: Expected output VSIX not found at ${vsixSrc}`);
  process.exit(1);
}
