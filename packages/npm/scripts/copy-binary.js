const fs = require('fs');
const path = require('path');
const os = require('os');

const isWindows = process.platform === 'win32';
const binName = isWindows ? 'stackvet.exe' : 'stackvet';
const binDir = path.join(__dirname, '..', 'bin');
const dest = path.join(binDir, binName);

if (!fs.existsSync(binDir)) {
  fs.mkdirSync(binDir, { recursive: true });
}

const rootDir = path.resolve(__dirname, '..', '..', '..');
const candidatePaths = [
  path.join(rootDir, 'target', 'release', binName),
  path.join(rootDir, 'target', 'debug', binName),
  path.join(__dirname, '..', '..', 'target', 'release', binName),
  path.join(__dirname, '..', '..', 'target', 'debug', binName),
  path.join(os.homedir(), '.cargo-target', 'stackvet', 'release', binName),
  path.join(os.homedir(), '.cargo-target', 'stackvet', 'debug', binName),
  path.join(os.homedir(), '.cargo', 'bin', binName),
];

let copied = false;
for (const src of candidatePaths) {
  if (fs.existsSync(src)) {
    console.log(`Bundling native binary from ${src} -> ${dest}`);
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

if (!copied) {
  // If no binary found, check if a pre-existing destination already exists
  if (fs.existsSync(dest)) {
    console.log(`Using existing bundled binary at ${dest}`);
    if (!isWindows) {
      try {
        fs.chmodSync(dest, 0o755);
      } catch (_) {}
    }
  } else {
    throw new Error(
      `Failed to bundle native binary: '${binName}' was not found in candidate paths:\n  ${candidatePaths.join('\n  ')}\n` +
      `Run 'cargo build -p stackvet --release' first.`
    );
  }
}
