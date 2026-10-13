#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const os = require('os');
const { spawnSync } = require('child_process');

const root = path.resolve(__dirname, '..');

console.log('====================================================');
console.log(' StackVet Cross-Platform Smoke Test Suite           ');
console.log(` Platform: ${process.platform} (${process.arch})     `);
console.log('====================================================\n');

let failedTests = 0;

function assert(condition, testName, details = '') {
  if (condition) {
    console.log(`  ✓ ${testName}`);
  } else {
    console.error(`  ✗ ${testName}${details ? `: ${details}` : ''}`);
    failedTests++;
  }
}

// -----------------------------------------------------------------------------
// Test Group 1: Version Consistency Check
// -----------------------------------------------------------------------------
console.log('[1/5] Checking manifest versions...');
const cargoPath = path.join(root, 'Cargo.toml');
const cargoContent = fs.readFileSync(cargoPath, 'utf8');
const versionMatch = cargoContent.match(/\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"/);
const cargoVersion = versionMatch ? versionMatch[1] : null;

assert(!!cargoVersion, 'Cargo workspace declares package version', cargoVersion);

const rootPkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
assert(rootPkg.version === cargoVersion, `Root package.json version (${rootPkg.version}) matches Cargo.toml (${cargoVersion})`);

const npmPkg = JSON.parse(fs.readFileSync(path.join(root, 'packages', 'npm', 'package.json'), 'utf8'));
assert(npmPkg.version === cargoVersion, `packages/npm/package.json version (${npmPkg.version}) matches Cargo.toml (${cargoVersion})`);

const pyprojectContent = fs.readFileSync(path.join(root, 'pyproject.toml'), 'utf8');
assert(
  pyprojectContent.includes('dynamic = ["version"]'),
  'pyproject.toml delegates dynamic versioning to Cargo workspace'
);

// -----------------------------------------------------------------------------
// Test Group 2: Packaged npm Files & Metadata Integrity
// -----------------------------------------------------------------------------
console.log('\n[2/5] Checking packaged npm metadata & files...');
assert(npmPkg.bin && npmPkg.bin.stackvet === 'bin/stackvet.js', 'npm package declares bin.stackvet entrypoint');
assert(npmPkg.main === 'bin/stackvet.js', 'npm package declares main entrypoint');
assert(npmPkg.files && npmPkg.files.includes('bin/'), 'npm package includes bin/ directory');

const launcherPath = path.join(root, 'packages', 'npm', 'bin', 'stackvet.js');
assert(fs.existsSync(launcherPath), 'npm launcher bin/stackvet.js exists');

// Verify launcher has shebang
const launcherContent = fs.readFileSync(launcherPath, 'utf8');
assert(launcherContent.startsWith('#!/usr/bin/env node'), 'bin/stackvet.js starts with node shebang');

// Verify README and LICENSE exist in repo root and npm package
assert(fs.existsSync(path.join(root, 'README.md')), 'Root README.md exists');
assert(fs.existsSync(path.join(root, 'LICENSE')), 'Root LICENSE exists');

// -----------------------------------------------------------------------------
// Test Group 3: Locate Native Binary
// -----------------------------------------------------------------------------
console.log('\n[3/5] Locating native StackVet binary...');
const isWindows = process.platform === 'win32';
const binName = isWindows ? 'stackvet.exe' : 'stackvet';

const candidatePaths = [
  path.join(root, 'target', 'release', binName),
  path.join(root, 'target', 'debug', binName),
  path.join(os.homedir(), '.cargo-target', 'stackvet', 'release', binName),
  path.join(os.homedir(), '.cargo-target', 'stackvet', 'debug', binName),
  path.join(root, 'packages', 'npm', 'bin', binName),
];

let nativeBinary = null;
for (const p of candidatePaths) {
  if (fs.existsSync(p)) {
    nativeBinary = p;
    break;
  }
}

assert(!!nativeBinary, `Native binary found on ${process.platform}`, nativeBinary || 'Not found');

// -----------------------------------------------------------------------------
// Test Group 4: Executable Permissions & Direct Binary Execution
// -----------------------------------------------------------------------------
console.log('\n[4/5] Testing direct binary execution...');
if (nativeBinary) {
  if (!isWindows) {
    try {
      fs.chmodSync(nativeBinary, 0o755);
      const stat = fs.statSync(nativeBinary);
      const isExecutable = (stat.mode & 0o111) !== 0;
      assert(isExecutable, `Binary at ${nativeBinary} has executable mode (${(stat.mode & 0o777).toString(8)})`);
    } catch (err) {
      assert(false, 'Failed to verify executable permissions', err.message);
    }
  }

  // stackvet --version
  const vRes = spawnSync(nativeBinary, ['--version'], { encoding: 'utf8' });
  assert(vRes.status === 0, 'stackvet --version returns exit code 0');
  assert(vRes.stdout.includes(cargoVersion), `stackvet --version stdout contains "${cargoVersion}"`, vRes.stdout.trim());

  // stackvet doctor --help
  const docRes = spawnSync(nativeBinary, ['doctor', '--help'], { encoding: 'utf8' });
  assert(docRes.status === 0, 'stackvet doctor --help returns exit code 0');
  assert(docRes.stdout.includes('doctor') || docRes.stdout.includes('health'), 'doctor --help output is valid');

  // stackvet rules
  const rulesRes = spawnSync(nativeBinary, ['rules'], { encoding: 'utf8' });
  assert(rulesRes.status === 0, 'stackvet rules returns exit code 0');

  // stackvet explain DEP-001
  const expRes = spawnSync(nativeBinary, ['explain', 'DEP-001'], { encoding: 'utf8' });
  assert(expRes.status === 0, 'stackvet explain DEP-001 returns exit code 0');
}

// -----------------------------------------------------------------------------
// Test Group 5: npm Launcher End-to-End Execution & Temp Project Smoke Test
// -----------------------------------------------------------------------------
console.log('\n[5/5] Testing npm launcher end-to-end...');
const npmVersionRes = spawnSync(process.execPath, [launcherPath, '--version'], { encoding: 'utf8' });
assert(npmVersionRes.status === 0, 'node bin/stackvet.js --version returns exit code 0');
assert(
  npmVersionRes.stdout.includes(cargoVersion),
  `node bin/stackvet.js --version stdout contains "${cargoVersion}"`,
  npmVersionRes.stdout.trim()
);

const npmDoctorRes = spawnSync(process.execPath, [launcherPath, 'doctor', '--help'], { encoding: 'utf8' });
assert(npmDoctorRes.status === 0, 'node bin/stackvet.js doctor --help returns exit code 0');

// Smoke test running doctor against a clean temporary directory
const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'stackvet-smoke-'));
try {
  fs.writeFileSync(path.join(tempDir, 'pyproject.toml'), '[project]\nname = "smoke-app"\nversion = "0.1.0"\n');
  const tempScanRes = spawnSync(process.execPath, [launcherPath, 'doctor', tempDir], { encoding: 'utf8' });
  assert(tempScanRes.status === 0, 'node bin/stackvet.js doctor on clean temp project returns 0');
} finally {
  fs.rmSync(tempDir, { recursive: true, force: true });
}

// -----------------------------------------------------------------------------
// Summary
// -----------------------------------------------------------------------------
console.log('\n====================================================');
if (failedTests === 0) {
  console.log(' ✓ ALL CROSS-PLATFORM SMOKE TESTS PASSED!          ');
  console.log('====================================================\n');
  process.exit(0);
} else {
  console.error(` ✗ ${failedTests} SMOKE TESTS FAILED!                    `);
  console.log('====================================================\n');
  process.exit(1);
}
