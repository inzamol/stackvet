import assert from 'node:assert';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { describe, it } from 'node:test';
import { normalizeFilePath, parseStackvetOutput, sanitizeSpan } from '../src/parser';

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

describe('parser.ts', () => {
  it('parses valid CLI JSON fixture correctly', () => {
    const fixturePath = getFixturePath('sample_report.json');
    const raw = fs.readFileSync(fixturePath, 'utf8');
    const report = parseStackvetOutput(raw);

    assert.strictEqual(report.schema_version, '1.0.0');
    assert.strictEqual(report.health_score, 65);
    assert.strictEqual(report.grade, 'C (Needs Attention)');
    assert.strictEqual(report.diagnostics.length, 6);

    const first = report.diagnostics[0];
    assert.strictEqual(first.rule_id, 'DJG-002');
    assert.strictEqual(first.severity, 'Critical');
    assert.strictEqual(first.location.file_path, 'C:\\projects\\myapp\\settings.py');
    assert.deepStrictEqual(first.location.span, {
      start_line: 7,
      start_col: 1,
      end_line: 7,
      end_col: 35,
    });
    assert.strictEqual(
      first.suggested_remediation,
      'Load SECRET_KEY securely from environment variables.'
    );
  });

  it('normalizes Windows extended path prefixes (\\\\?\\)', () => {
    assert.strictEqual(
      normalizeFilePath('\\\\?\\C:\\Users\\user\\project\\app.py'),
      'C:\\Users\\user\\project\\app.py'
    );
    assert.strictEqual(normalizeFilePath('relative/path.py'), 'relative/path.py');
    assert.strictEqual(normalizeFilePath(''), '');
  });

  it('sanitizes valid and invalid text spans defensively', () => {
    // Normal valid span
    assert.deepStrictEqual(
      sanitizeSpan({ start_line: 10, start_col: 5, end_line: 10, end_col: 20 }),
      { start_line: 10, start_col: 5, end_line: 10, end_col: 20 }
    );

    // Negative or zero lines/cols return null
    assert.strictEqual(
      sanitizeSpan({ start_line: 0, start_col: 1, end_line: 1, end_col: 1 }),
      null
    );
    assert.strictEqual(
      sanitizeSpan({ start_line: 5, start_col: -1, end_line: 5, end_col: 10 }),
      null
    );

    // end_line < start_line gets clamped
    assert.deepStrictEqual(
      sanitizeSpan({ start_line: 10, start_col: 5, end_line: 8, end_col: 20 }),
      { start_line: 10, start_col: 5, end_line: 10, end_col: 5 }
    );

    // null / non-object returns null
    assert.strictEqual(sanitizeSpan(null), null);
    assert.strictEqual(sanitizeSpan(undefined), null);
    assert.strictEqual(sanitizeSpan('not-a-span'), null);
  });

  it('preserves paths with spaces and Unicode characters', () => {
    const fixturePath = getFixturePath('sample_report.json');
    const raw = fs.readFileSync(fixturePath, 'utf8');
    const report = parseStackvetOutput(raw);

    const spaceFinding = report.diagnostics.find((d) => d.rule_id === 'NDEP-004');
    assert.ok(spaceFinding);
    assert.strictEqual(
      spaceFinding.location.file_path,
      'path with spaces/nested folder/package.json'
    );

    const unicodeFinding = report.diagnostics.find((d) => d.rule_id === 'FAP-002');
    assert.ok(unicodeFinding);
    assert.strictEqual(unicodeFinding.location.file_path, 'src/тест_unicode/api.py');
  });

  it('throws descriptive error on empty or whitespace output', () => {
    assert.throws(
      () => parseStackvetOutput(''),
      /CLI returned empty output/
    );
    assert.throws(
      () => parseStackvetOutput('   \n  \t  '),
      /CLI returned empty output/
    );
  });

  it('throws descriptive error on malformed JSON', () => {
    assert.throws(
      () => parseStackvetOutput('{ "invalid": json without quotes }'),
      /Failed to parse StackVet output as JSON/
    );
  });

  it('throws error when diagnostics array is missing', () => {
    assert.throws(
      () => parseStackvetOutput('{"schema_version": "1.0.0"}'),
      /missing 'diagnostics' array/
    );
  });
});
