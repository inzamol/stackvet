#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const os = require('os');
const { spawnSync } = require('child_process');
const https = require('https');

const pkg = require('../package.json');
const VERSION = pkg.version;

function getPlatformTriple() {
  const platform = process.platform;
  const arch = process.arch;

  let osName = '';
  if (platform === 'linux') {
    osName = 'linux';
  } else if (platform === 'darwin') {
    osName = 'macos';
  } else if (platform === 'win32') {
    osName = 'windows';
  } else {
    return null;
  }

  let archName = '';
  if (arch === 'x64') {
    archName = 'x86_64';
  } else if (arch === 'arm64') {
    archName = 'aarch64';
  } else {
    return null;
  }

  return `${osName}-${archName}`;
}

function ensureExecutable(filePath) {
  if (process.platform === 'win32') return;
  try {
    const stat = fs.statSync(filePath);
    // If not executable by user (0o100)
    if ((stat.mode & 0o100) === 0) {
      fs.chmodSync(filePath, 0o755);
    }
  } catch (_) {}
}

function getCandidateBinaryPaths() {
  const isWindows = process.platform === 'win32';
  const binaryName = isWindows ? 'stackvet.exe' : 'stackvet';
  const triple = getPlatformTriple();
  const platformBinaryName = triple ? (isWindows ? `stackvet-${triple}.exe` : `stackvet-${triple}`) : binaryName;

  const candidates = [];

  // 1. Local bin directory inside package (production bundled)
  candidates.push(path.join(__dirname, binaryName));
  if (triple) {
    candidates.push(path.join(__dirname, platformBinaryName));
  }

  // 2. User cache directory (~/.stackvet/bin/)
  const userCacheDir = path.join(os.homedir(), '.stackvet', 'bin');
  candidates.push(path.join(userCacheDir, binaryName));
  if (triple) {
    candidates.push(path.join(userCacheDir, platformBinaryName));
  }

  // 3. Repository root target/release & target/debug directory (development & CI)
  const rootDir = path.resolve(__dirname, '..', '..', '..');
  candidates.push(path.join(rootDir, 'target', 'release', binaryName));
  candidates.push(path.join(rootDir, 'target', 'debug', binaryName));

  // 4. Cargo target directories and home cargo bin
  candidates.push(path.join(os.homedir(), '.cargo-target', 'stackvet', 'release', binaryName));
  candidates.push(path.join(os.homedir(), '.cargo-target', 'stackvet', 'debug', binaryName));
  candidates.push(path.join(os.homedir(), '.cargo', 'bin', binaryName));

  return { candidates, binaryName, platformBinaryName, triple };
}

function findExistingBinary() {
  const { candidates } = getCandidateBinaryPaths();
  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      ensureExecutable(candidate);
      return candidate;
    }
  }
  return null;
}

function downloadBinary(targetTriple, destPath) {
  return new Promise((resolve, reject) => {
    const isWindows = process.platform === 'win32';
    const ext = isWindows ? '.exe' : '';
    const releaseAsset = `stackvet-${targetTriple}${ext}`;
    const url = `https://github.com/inzamol/stackvet/releases/download/v${VERSION}/${releaseAsset}`;

    process.stderr.write(`\x1b[36mStackVet:\x1b[0m Downloading native binary for ${targetTriple} from GitHub Releases...\n`);

    const destDir = path.dirname(destPath);
    if (!fs.existsSync(destDir)) {
      try {
        fs.mkdirSync(destDir, { recursive: true });
      } catch (err) {
        return reject(err);
      }
    }

    function followRedirect(requestUrl, redirectCount = 0) {
      if (redirectCount > 5) {
        return reject(new Error('Too many HTTP redirects'));
      }

      https.get(requestUrl, { headers: { 'User-Agent': `stackvet-npm/${VERSION}` } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          return followRedirect(res.headers.location, redirectCount + 1);
        }

        if (res.statusCode !== 200) {
          return reject(new Error(`Download failed with HTTP ${res.statusCode}: ${res.statusMessage}`));
        }

        const tempPath = `${destPath}.tmp.${Date.now()}`;
        const fileStream = fs.createWriteStream(tempPath);

        res.pipe(fileStream);

        fileStream.on('finish', () => {
          fileStream.close(() => {
            try {
              ensureExecutable(tempPath);
              fs.renameSync(tempPath, destPath);
              ensureExecutable(destPath);
              resolve(destPath);
            } catch (err) {
              reject(err);
            }
          });
        });

        fileStream.on('error', (err) => {
          try { fs.unlinkSync(tempPath); } catch (_) {}
          reject(err);
        });
      }).on('error', reject);
    }

    followRedirect(url);
  });
}

function printInstallHelp(triple) {
  const targetDesc = triple || `${process.platform}-${process.arch}`;
  console.error(`\n\x1b[31mError: StackVet native binary for platform '${targetDesc}' is unavailable.\x1b[0m`);
  console.error('\nTo use StackVet on this system, choose one of the following installation methods:');
  console.error('  1. Install via Rust / Cargo:');
  console.error('     \x1b[32mcargo install stackvet\x1b[0m\n');
  console.error('  2. Install via Python / PyPI:');
  console.error('     \x1b[32mpip install stackvet\x1b[0m  or  \x1b[32muvx stackvet doctor\x1b[0m\n');
  console.error('  3. Download prebuilt standalone binary:');
  console.error(`     \x1b[34mhttps://github.com/inzamol/stackvet/releases/tag/v${VERSION}\x1b[0m\n`);
}

function runExecutable(binPath, args) {
  ensureExecutable(binPath);
  const result = spawnSync(binPath, args, {
    stdio: 'inherit',
    windowsHide: false
  });

  if (result.error) {
    return { ok: false, error: result.error };
  }
  return { ok: true, status: result.status ?? 0 };
}

async function main() {
  const args = process.argv.slice(2);
  let binary = findExistingBinary();

  if (binary) {
    const execResult = runExecutable(binary, args);
    if (execResult.ok) {
      process.exit(execResult.status);
    }

    // If execution failed due to incompatible architecture (e.g. bundled linux binary executed on macOS)
    if (execResult.error && execResult.error.code === 'ENOEXEC') {
      binary = null;
    } else if (execResult.error && execResult.error.code !== 'ENOENT') {
      console.error(`\x1b[31mExecution error: ${execResult.error.message}\x1b[0m`);
      process.exit(1);
    }
  }

  // If binary not found or had ENOEXEC, try system PATH fallback first
  const isWindows = process.platform === 'win32';
  const systemBinName = isWindows ? 'stackvet.exe' : 'stackvet';
  const pathExecResult = runExecutable(systemBinName, args);
  if (pathExecResult.ok) {
    process.exit(pathExecResult.status);
  }

  // Attempt on-demand download from GitHub Releases
  const triple = getPlatformTriple();
  if (triple) {
    const userCacheBin = path.join(os.homedir(), '.stackvet', 'bin', isWindows ? 'stackvet.exe' : 'stackvet');
    try {
      const downloadedPath = await downloadBinary(triple, userCacheBin);
      const downloadExec = runExecutable(downloadedPath, args);
      if (downloadExec.ok) {
        process.exit(downloadExec.status);
      }
    } catch (_) {
      // Download failed or offline, fall through to clear instructions
    }
  }

  printInstallHelp(triple);
  process.exit(1);
}

main().catch((err) => {
  console.error(`\x1b[31mUnexpected launcher error: ${err.message}\x1b[0m`);
  process.exit(1);
});
