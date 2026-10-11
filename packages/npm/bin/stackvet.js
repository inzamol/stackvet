#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

function getBinaryPath() {
  const isWindows = process.platform === 'win32';
  const binaryName = isWindows ? 'stackvet.exe' : 'stackvet';

  // 1. Check local bin directory inside package (production bundled)
  const localBin = path.join(__dirname, binaryName);
  if (fs.existsSync(localBin)) {
    return localBin;
  }

  // 2. Check repository root target/release directory (local development & CI)
  const rootDir = path.resolve(__dirname, '..', '..', '..');
  const rootReleaseBin = path.join(rootDir, 'target', 'release', binaryName);
  if (fs.existsSync(rootReleaseBin)) {
    return rootReleaseBin;
  }
  const rootDebugBin = path.join(rootDir, 'target', 'debug', binaryName);
  if (fs.existsSync(rootDebugBin)) {
    return rootDebugBin;
  }

  // 3. Check cargo target and home cargo bin
  const os = require('os');
  const cargoTargetBin = path.join(os.homedir(), '.cargo-target', 'stackvet', 'release', binaryName);
  if (fs.existsSync(cargoTargetBin)) {
    return cargoTargetBin;
  }
  const cargoBin = path.join(os.homedir(), '.cargo', 'bin', binaryName);
  if (fs.existsSync(cargoBin)) {
    return cargoBin;
  }

  // 4. Fallback to system PATH
  return binaryName;
}

const binary = getBinaryPath();
const args = process.argv.slice(2);

try {
  const result = spawnSync(binary, args, {
    stdio: 'inherit',
    windowsHide: false
  });

  if (result.error) {
    if (result.error.code === 'ENOENT') {
      console.error(`\x1b[31mError: StackVet native binary '${binary}' not found.\x1b[0m`);
      console.error('Please ensure the native binary is built or installed via cargo:');
      console.error('  cargo install stackvet\n');
    } else {
      console.error(`\x1b[31mExecution error: ${result.error.message}\x1b[0m`);
    }
    process.exit(1);
  }

  process.exit(result.status ?? 0);
} catch (err) {
  console.error(`\x1b[31mUnexpected error: ${err.message}\x1b[0m`);
  process.exit(1);
}
