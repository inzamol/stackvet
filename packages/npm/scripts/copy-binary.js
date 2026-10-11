const fs = require('fs');
const path = require('path');
const os = require('os');

const isWindows = process.platform === 'win32';
const binName = isWindows ? 'stackvet.exe' : 'stackvet';
const dest = path.join(__dirname, '..', 'bin', binName);

const candidatePaths = [
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
  console.warn(`Warning: Could not find prebuilt native binary in candidate paths.`);
}
