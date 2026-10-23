import './mock-vscode';
import assert from 'node:assert';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { describe, it } from 'node:test';
import * as vscode from 'vscode';
import {
  formatDiagnosticMessage,
  mapFindingsToDiagnostics,
  mapSeverity,
  spanToRange,
} from '../src/diagnostics';
import { parseStackvetOutput } from '../src/parser';
import { DiagnosticFinding } from '../src/types';

describe('diagnostics.ts', () => {
  it('maps severities correctly with safe fallback', () => {
    assert.strictEqual(mapSeverity('Critical'), vscode.DiagnosticSeverity.Error);
    assert.strictEqual(mapSeverity('fatal'), vscode.DiagnosticSeverity.Error);
    assert.strictEqual(mapSeverity('Error'), vscode.DiagnosticSeverity.Error);
    assert.strictEqual(mapSeverity('Warning'), vscode.DiagnosticSeverity.Warning);
    assert.strictEqual(mapSeverity('warn'), vscode.DiagnosticSeverity.Warning);
    assert.strictEqual(mapSeverity('Info'), vscode.DiagnosticSeverity.Information);
    assert.strictEqual(mapSeverity('Hint'), vscode.DiagnosticSeverity.Hint);
    assert.strictEqual(mapSeverity('unknown_custom_sev'), vscode.DiagnosticSeverity.Warning);
    assert.strictEqual(mapSeverity(''), vscode.DiagnosticSeverity.Warning);
  });

  it('converts 1-based TextSpan to 0-based VS Code Range', () => {
    const range = spanToRange({
      start_line: 7,
      start_col: 1,
      end_line: 7,
      end_col: 35,
    });

    assert.strictEqual(range.start.line, 6);
    assert.strictEqual(range.start.character, 0);
    assert.strictEqual(range.end.line, 6);
    assert.strictEqual(range.end.character, 34);

    // Missing span falls back to (0, 0) to (0, 0)
    const fallback = spanToRange(null);
    assert.strictEqual(fallback.start.line, 0);
    assert.strictEqual(fallback.start.character, 0);
    assert.strictEqual(fallback.end.line, 0);
    assert.strictEqual(fallback.end.character, 0);
  });

  it('formats diagnostic message with remediation guidance when present', () => {
    const findingWithRemediation: DiagnosticFinding = {
      rule_id: 'DEP-001',
      message: 'Unpinned dependency',
      severity: 'Warning',
      location: { file_path: 'req.txt' },
      suggested_remediation: 'Pin requests==2.31.0',
    };
    const msg = formatDiagnosticMessage(findingWithRemediation);
    assert.ok(msg.includes('Unpinned dependency'));
    assert.ok(msg.includes('Remediation: Pin requests==2.31.0'));

    const findingWithoutRemediation: DiagnosticFinding = {
      rule_id: 'DOC-001',
      message: 'Missing README',
      severity: 'Info',
      location: { file_path: 'README.md' },
    };
    const cleanMsg = formatDiagnosticMessage(findingWithoutRemediation);
    assert.strictEqual(cleanMsg, 'Missing README');
    assert.ok(!cleanMsg.includes('Remediation'));
  });

function getFixturePath(name: string): string {
  const candidates = [
    path.join(__dirname, 'fixtures', name),
    path.join(__dirname, '..', '..', 'test', 'fixtures', name),
    path.join(__dirname, '..', 'fixtures', name),
  ];
  for (const c of candidates) {
    if (fs.existsSync(c)) {
      return c;
    }
  }
  return candidates[0];
}

  it('maps real report findings to diagnostics grouped by URI', () => {
    const fixturePath = getFixturePath('sample_report.json');
    const raw = fs.readFileSync(fixturePath, 'utf8');
    const report = parseStackvetOutput(raw);

    const workspaceRoot = 'C:\\workspace\\test-project';
    const result = mapFindingsToDiagnostics(report.diagnostics, workspaceRoot);

    // Finding with <unknown> location should not be mapped to a file
    assert.strictEqual(result.unmappedFindings.length, 1);
    assert.strictEqual(result.unmappedFindings[0].rule_id, 'DOC-001');

    // Mapped diagnostics
    assert.ok(result.diagnosticsByUri.size > 0);

    // Check relative path resolution against workspaceRoot
    let reqUriFound = false;
    for (const [uriStr, diags] of result.diagnosticsByUri.entries()) {
      if (uriStr.includes('requirements.txt')) {
        reqUriFound = true;
        assert.strictEqual(diags.length, 1);
        const diag = diags[0];
        assert.strictEqual(diag.code, 'DEP-001');
        assert.strictEqual(diag.source, 'StackVet');
        assert.strictEqual(diag.severity, vscode.DiagnosticSeverity.Warning);
        assert.strictEqual(diag.range.start.line, 11);
        assert.ok(diag.message.includes('Remediation'));
      }
    }
    assert.ok(reqUriFound, 'requirements.txt should be resolved to absolute URI');

    // Check path with spaces
    let spaceUriFound = false;
    for (const [uriStr] of result.diagnosticsByUri.entries()) {
      if (uriStr.includes('path with spaces')) {
        spaceUriFound = true;
      }
    }
    assert.ok(spaceUriFound, 'Path with spaces should be preserved in URI');

    // Check path with Unicode
    let unicodeUriFound = false;
    for (const [uriStr] of result.diagnosticsByUri.entries()) {
      if (uriStr.includes('тест_unicode')) {
        unicodeUriFound = true;
      }
    }
    assert.ok(unicodeUriFound, 'Path with Unicode should be preserved in URI');
  });
});
