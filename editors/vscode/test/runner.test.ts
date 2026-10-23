import assert from 'node:assert';
import { describe, it } from 'node:test';
import { resolveExecutable, resolveExecutionTargets, runStackvetCli } from '../src/runner';

describe('runner.ts', () => {
  it('resolves executable path correctly', () => {
    assert.strictEqual(
      resolveExecutable('/custom/path/to/stackvet'),
      '/custom/path/to/stackvet'
    );
    const resolved = resolveExecutable('');
    assert.ok(resolved === 'stackvet' || resolved.includes('stackvet'));
  });

  it('resolves live npx and pipx fallback targets when no custom path is set', () => {
    const targets = resolveExecutionTargets(undefined, undefined, '/test/workspace');
    assert.ok(targets.length >= 3);

    const hasNpx = targets.some((t) => t.name.includes('npx') && t.args.includes('@inzamol/stackvet'));
    assert.ok(hasNpx, 'Expected live npx fallback target');

    const hasPipx = targets.some((t) => t.name.includes('pipx') && t.args.includes('stackvet'));
    assert.ok(hasPipx, 'Expected live pipx fallback target');
  });

  it('restricts to single target when custom executable is configured', () => {
    const targets = resolveExecutionTargets('/custom/bin/stackvet', undefined, '/test/workspace');
    assert.strictEqual(targets.length, 1);
    assert.strictEqual(targets[0].command, '/custom/bin/stackvet');
  });

  it('rejects with actionable guidance when executable is not found (ENOENT)', async () => {
    await assert.rejects(
      async () => {
        await runStackvetCli({
          executablePath: 'nonexistent-stackvet-binary-xyz-12345',
          workspaceRoot: process.cwd(),
        });
      },
      (err: Error) => {
        assert.ok(
          err.message.includes('was not found') || err.message.includes('Failed to'),
          `Unexpected message: ${err.message}`
        );
        return true;
      }
    );
  });

  it('handles execution error exit codes cleanly', async () => {
    const fs = await import('node:fs');
    const path = await import('node:path');
    const fixtureDir = path.join(__dirname, 'fixtures');
    if (!fs.existsSync(fixtureDir)) {
      fs.mkdirSync(fixtureDir, { recursive: true });
    }
    const fixtureScript = path.join(fixtureDir, 'fail_exit2.js');
    fs.writeFileSync(
      fixtureScript,
      'process.stderr.write("Custom error on stderr\\n"); process.exit(2);\n',
      'utf8'
    );

    await assert.rejects(
      async () => {
        await runStackvetCli({
          executablePath: process.execPath,
          args: [fixtureScript],
          workspaceRoot: process.cwd(),
        });
      },
      (err: Error) => {
        assert.ok(err.message.includes('exit code 2'), `Message should include exit code: ${err.message}`);
        assert.ok(err.message.includes('Custom error on stderr'), `Message should include stderr: ${err.message}`);
        return true;
      }
    );
  });

  it('runs against real native StackVet binary when available', async (t) => {
    const fs = await import('node:fs');
    const path = await import('node:path');
    const os = await import('node:os');

    const rootCandidates = [
      path.resolve(__dirname, '..', '..', '..', '..'),
      path.resolve(__dirname, '..', '..', '..'),
      process.cwd(),
      path.resolve(process.cwd(), '..', '..'),
    ];
    let root = rootCandidates[0];
    for (const rc of rootCandidates) {
      if (fs.existsSync(path.join(rc, 'Cargo.toml'))) {
        root = rc;
        break;
      }
    }
    const isWindows = process.platform === 'win32';
    const binName = isWindows ? 'stackvet.exe' : 'stackvet';
    const candidates = [
      path.join(root, 'target', 'release', binName),
      path.join(root, 'target', 'debug', binName),
      path.join(os.homedir(), '.cargo-target', 'stackvet', 'release', binName),
      path.join(os.homedir(), '.cargo-target', 'stackvet', 'debug', binName),
    ];

    let foundBin: string | null = null;
    for (const c of candidates) {
      if (fs.existsSync(c)) {
        foundBin = c;
        break;
      }
    }

    if (!foundBin) {
      t.skip('Native StackVet binary not found');
      return;
    }

    const fixtureDir = path.join(root, 'tests', 'fixtures', 'regression', 'positive', 'django_violations');
    if (!fs.existsSync(fixtureDir)) {
      t.skip('django_violations fixture directory not found');
      return;
    }

    const result = await runStackvetCli({
      executablePath: foundBin,
      workspaceRoot: fixtureDir,
    });

    assert.ok(result.report);
    assert.ok(Array.isArray(result.report.diagnostics));
    assert.ok(result.report.diagnostics.length > 0);

    const djgFinding = result.report.diagnostics.find((d) => d.rule_id === 'DJG-002');
    assert.ok(djgFinding, 'Expected DJG-002 finding in django_violations fixture');
    assert.strictEqual(djgFinding.severity, 'Critical');
    assert.ok(djgFinding.suggested_remediation);
  });
});
