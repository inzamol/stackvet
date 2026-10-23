import './mock-vscode';
import { test, describe, beforeEach } from 'node:test';
import * as assert from 'node:assert';
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as os from 'node:os';
import {
  findNestedProjectRoot,
  resolveDiagnosticBaseDir,
  clearMarkerCache,
} from '../src/extension';
import { DiagnosticFinding } from '../src/types';

describe('extension.ts - diagnostic base directory & project root resolution', () => {
  let tempWorkspace: string;
  let nestedApp: string;
  let nestedTargetFile: string;

  beforeEach(() => {
    clearMarkerCache();
    tempWorkspace = fs.mkdtempSync(path.join(os.tmpdir(), 'stackvet-ext-test-'));
    nestedApp = path.join(tempWorkspace, 'apps', 'web');
    fs.mkdirSync(path.join(nestedApp, 'src'), { recursive: true });
    fs.writeFileSync(
      path.join(nestedApp, 'package.json'),
      JSON.stringify({ name: 'web-app', version: '1.0.0' }, null, 2)
    );
    nestedTargetFile = path.join(nestedApp, 'src', 'index.ts');
    fs.writeFileSync(nestedTargetFile, 'console.log("hello");');
  });

  test('resolves nested project root when CLI diagnostic matches file target relative to nested manifest', () => {
    const diagnostics: DiagnosticFinding[] = [
      {
        rule_id: 'NDEP-001',
        message: 'Missing package',
        severity: 'Error',
        location: {
          file_path: 'src/index.ts',
        },
      },
    ];

    const baseDir = resolveDiagnosticBaseDir(
      nestedTargetFile,
      tempWorkspace,
      diagnostics,
      true
    );

    assert.strictEqual(
      path.resolve(baseDir).toLowerCase(),
      path.resolve(nestedApp).toLowerCase()
    );
  });

  test('falls back to workspace root when relative diagnostic does not match target', () => {
    const diagnostics: DiagnosticFinding[] = [
      {
        rule_id: 'NDEP-001',
        message: 'Other file',
        severity: 'Error',
        location: {
          file_path: 'src/other.ts',
        },
      },
    ];

    const baseDir = resolveDiagnosticBaseDir(
      nestedTargetFile,
      tempWorkspace,
      diagnostics,
      true
    );

    assert.strictEqual(
      path.resolve(baseDir).toLowerCase(),
      path.resolve(tempWorkspace).toLowerCase()
    );
  });

  test('falls back to workspace root when no markers are found between target and stopDir', () => {
    const noMarkerDir = path.join(tempWorkspace, 'plain', 'sub');
    fs.mkdirSync(noMarkerDir, { recursive: true });
    const targetFile = path.join(noMarkerDir, 'test.py');
    fs.writeFileSync(targetFile, 'print("test")');

    const diagnostics: DiagnosticFinding[] = [
      {
        rule_id: 'DEP-001',
        message: 'Missing dependency',
        severity: 'Error',
        location: {
          file_path: 'test.py',
        },
      },
    ];

    const baseDir = resolveDiagnosticBaseDir(
      targetFile,
      tempWorkspace,
      diagnostics,
      true
    );

    assert.strictEqual(
      path.resolve(baseDir).toLowerCase(),
      path.resolve(tempWorkspace).toLowerCase()
    );
  });

  test('caches marker checks per directory and respects clearMarkerCache', () => {
    const root1 = findNestedProjectRoot(path.dirname(nestedTargetFile), tempWorkspace);
    assert.strictEqual(
      path.resolve(root1!).toLowerCase(),
      path.resolve(nestedApp).toLowerCase()
    );

    // Call again to verify cached result
    const root2 = findNestedProjectRoot(path.dirname(nestedTargetFile), tempWorkspace);
    assert.strictEqual(root1, root2);

    clearMarkerCache();
    const root3 = findNestedProjectRoot(path.dirname(nestedTargetFile), tempWorkspace);
    assert.strictEqual(
      path.resolve(root3!).toLowerCase(),
      path.resolve(nestedApp).toLowerCase()
    );
  });

  test('resolves nested project root when CLI report includes relative findings for manifest or sibling files', () => {
    const siblingFile = path.join(nestedApp, 'src', 'utils.ts');
    fs.writeFileSync(siblingFile, 'export const x = 1;');

    const diagnostics: DiagnosticFinding[] = [
      {
        rule_id: 'DEP-002',
        message: 'Unused manifest dependency',
        severity: 'Warning',
        location: {
          file_path: 'package.json',
        },
      },
      {
        rule_id: 'NDEP-001',
        message: 'Missing package in sibling',
        severity: 'Error',
        location: {
          file_path: 'src/utils.ts',
        },
      },
    ];

    const baseDir = resolveDiagnosticBaseDir(
      nestedTargetFile,
      tempWorkspace,
      diagnostics,
      true
    );

    assert.strictEqual(
      path.resolve(baseDir).toLowerCase(),
      path.resolve(nestedApp).toLowerCase()
    );
  });

  test('invalidates cached misses when project marker is created in directory', async () => {
    const freshDir = path.join(tempWorkspace, 'apps', 'dynamic-app');
    const freshSrc = path.join(freshDir, 'src');
    fs.mkdirSync(freshSrc, { recursive: true });
    const freshFile = path.join(freshSrc, 'index.ts');
    fs.writeFileSync(freshFile, 'console.log("dynamic");');

    // Initially no markers
    const rootInitial = findNestedProjectRoot(path.dirname(freshFile), tempWorkspace);
    assert.strictEqual(rootInitial, undefined);

    // Wait briefly to ensure filesystem mtime difference, then write package.json
    await new Promise((resolve) => setTimeout(resolve, 50));
    fs.writeFileSync(
      path.join(freshDir, 'package.json'),
      JSON.stringify({ name: 'dynamic-app' })
    );

    // Without manual clearMarkerCache(), the directory mtime change causes recheck
    const rootAfter = findNestedProjectRoot(path.dirname(freshFile), tempWorkspace);
    assert.strictEqual(
      path.resolve(rootAfter!).toLowerCase(),
      path.resolve(freshDir).toLowerCase()
    );
  });

  test('resolves nested project root when sibling finding has mixed or uppercase characters', () => {
    const uppercaseSibling = path.join(nestedApp, 'src', 'AppHeader.tsx');
    fs.writeFileSync(uppercaseSibling, 'export const AppHeader = () => null;');

    const diagnostics: DiagnosticFinding[] = [
      {
        rule_id: 'RCT-001',
        message: 'React issue in sibling',
        severity: 'Warning',
        location: {
          file_path: 'src/AppHeader.tsx',
        },
      },
    ];

    const baseDir = resolveDiagnosticBaseDir(
      nestedTargetFile,
      tempWorkspace,
      diagnostics,
      true
    );

    assert.strictEqual(
      path.resolve(baseDir).toLowerCase(),
      path.resolve(nestedApp).toLowerCase()
    );
  });
});
