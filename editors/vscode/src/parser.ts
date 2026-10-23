import { DiagnosticFinding, DiagnosticLocation, StackvetReport, TextSpan } from './types';

/**
 * Normalizes a file path returned by StackVet:
 * - Strips Windows extended-path prefix (`\\?\`)
 * - Converts backslashes to forward slashes for uniform handling
 */
export function normalizeFilePath(rawPath: string): string {
  if (!rawPath) {
    return '';
  }
  let p = rawPath.replace(/^\\\\\?\\/, '');
  return p;
}

/**
 * Validates whether a given value is a valid 1-based number.
 */
function isValidLineOrCol(val: unknown): val is number {
  return typeof val === 'number' && Number.isInteger(val) && val >= 1;
}

/**
 * Parses and validates a TextSpan, ensuring positive integers.
 */
export function sanitizeSpan(rawSpan: unknown): TextSpan | null {
  if (!rawSpan || typeof rawSpan !== 'object') {
    return null;
  }
  const s = rawSpan as Record<string, unknown>;
  if (!isValidLineOrCol(s.start_line) || !isValidLineOrCol(s.start_col)) {
    return null;
  }

  const startLine = s.start_line;
  const startCol = s.start_col;
  let endLine = isValidLineOrCol(s.end_line) ? s.end_line : startLine;
  let endCol = isValidLineOrCol(s.end_col) ? s.end_col : startCol;

  if (endLine < startLine) {
    endLine = startLine;
    endCol = startCol;
  } else if (endLine === startLine && endCol < startCol) {
    endCol = startCol;
  }

  return {
    start_line: startLine,
    start_col: startCol,
    end_line: endLine,
    end_col: endCol,
  };
}

/**
 * Sanitizes a single diagnostic finding object.
 */
export function sanitizeFinding(raw: unknown): DiagnosticFinding | null {
  if (!raw || typeof raw !== 'object') {
    return null;
  }
  const item = raw as Record<string, unknown>;

  const ruleId = typeof item.rule_id === 'string' && item.rule_id.trim().length > 0
    ? item.rule_id.trim()
    : 'UNKNOWN';

  const message = typeof item.message === 'string'
    ? item.message
    : 'No message provided';

  const severity = typeof item.severity === 'string'
    ? item.severity
    : 'Warning';

  let location: DiagnosticLocation = {
    file_path: '',
    span: null,
  };

  if (item.location && typeof item.location === 'object') {
    const loc = item.location as Record<string, unknown>;
    const rawFilePath = typeof loc.file_path === 'string' ? loc.file_path : '';
    location = {
      file_path: normalizeFilePath(rawFilePath),
      span: sanitizeSpan(loc.span),
      snippet: typeof loc.snippet === 'string' ? loc.snippet : null,
      message: typeof loc.message === 'string' ? loc.message : null,
    };
  }

  const finding: DiagnosticFinding = {
    rule_id: ruleId,
    message,
    severity,
    location,
    explanation: typeof item.explanation === 'string' ? item.explanation : undefined,
    suggested_remediation:
      typeof item.suggested_remediation === 'string' ? item.suggested_remediation : undefined,
    language: typeof item.language === 'string' ? item.language : undefined,
    confidence: typeof item.confidence === 'string' ? item.confidence : undefined,
  };

  return finding;
}

/**
 * Parses the CLI JSON output into a validated StackvetReport.
 * Throws an Error with a helpful message if parsing fails.
 */
export function parseStackvetOutput(rawOutput: string): StackvetReport {
  const trimmed = rawOutput.trim();
  if (!trimmed) {
    throw new Error('CLI returned empty output. Expected JSON report.');
  }

  let jsonStr = trimmed;
  const firstBrace = trimmed.indexOf('{');
  const lastBrace = trimmed.lastIndexOf('}');
  if (firstBrace !== -1 && lastBrace !== -1 && lastBrace > firstBrace) {
    jsonStr = trimmed.substring(firstBrace, lastBrace + 1);
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(jsonStr);
  } catch (err) {
    const preview = trimmed.length > 200 ? trimmed.substring(0, 200) + '...' : trimmed;
    throw new Error(
      `Failed to parse StackVet output as JSON: ${err instanceof Error ? err.message : String(err)}. Output preview: ${preview}`
    );
  }

  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Invalid StackVet report: expected a JSON object.');
  }

  const obj = parsed as Record<string, unknown>;
  if (!Array.isArray(obj.diagnostics)) {
    throw new Error("Invalid StackVet report: missing 'diagnostics' array.");
  }

  const sanitizedDiagnostics: DiagnosticFinding[] = [];
  for (const rawFinding of obj.diagnostics) {
    const sanitized = sanitizeFinding(rawFinding);
    if (sanitized) {
      sanitizedDiagnostics.push(sanitized);
    }
  }

  return {
    schema_version: typeof obj.schema_version === 'string' ? obj.schema_version : undefined,
    health_score: typeof obj.health_score === 'number' ? obj.health_score : undefined,
    grade: typeof obj.grade === 'string' ? obj.grade : undefined,
    summary: obj.summary && typeof obj.summary === 'object' ? (obj.summary as StackvetReport['summary']) : undefined,
    diagnostics: sanitizedDiagnostics,
  };
}
