const fs = require('fs');
const path = require('path');
const os = require('os');

const isWindows = process.platform === 'win32';
const binName = isWindows ? 'stackvet.exe' : 'stackvet';
const dest = path.join(__dirname, '..', 'bin', binName);

const rootDir = path.resolve(__dirname, '..', '..', '..');
const candidatePaths = [
  path.join(rootDir, 'target', 'release', binName),
  path.join(rootDir, 'target', 'debug', binName),
  path.join(__dirname, '..', '..', 'target', 'release', binName),
  path.join(os.homedir(), '.cargo-target', 'stackvet', 'release', binName),
  path.join(os.homedir(), '.cargo', 'bin', binName),
];

let copied = false;
for (const src of candidatePaths) {
  if (fs.existsSync(src)) {
    console.log(`Bundling native binary from ${src} -> ${dest}`);
    fs.copyFileSync(src, dest);
    if (!isWindows) {
      fs.chmodSync(dest, 0o755);
    }
    copied = true;
    break;
  }
}

if (!copied) {
  throw new Error(`Failed to bundle native binary: '${binName}' was not found in candidate paths:\n  ${candidatePaths.join('\n  ')}`);
}
