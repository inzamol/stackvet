import * as path from 'path';
import * as vscode from 'vscode';
import { DiagnosticFinding, TextSpan } from './types';

/**
 * Maps StackVet severity string to VS Code DiagnosticSeverity.
 */
export function mapSeverity(rawSeverity: string): vscode.DiagnosticSeverity {
  const s = (rawSeverity || '').trim().toLowerCase();
  switch (s) {
    case 'critical':
    case 'fatal':
    case 'error':
      return vscode.DiagnosticSeverity.Error;
    case 'warning':
    case 'warn':
      return vscode.DiagnosticSeverity.Warning;
    case 'info':
    case 'information':
      return vscode.DiagnosticSeverity.Information;
    case 'hint':
      return vscode.DiagnosticSeverity.Hint;
    default:
      return vscode.DiagnosticSeverity.Warning;
  }
}

/**
 * Converts a 1-based TextSpan to a 0-based VS Code Range.
 */
export function spanToRange(span?: TextSpan | null): vscode.Range {
  if (!span) {
    return new vscode.Range(new vscode.Position(0, 0), new vscode.Position(0, 0));
  }

  const startLine = Math.max(0, (span.start_line || 1) - 1);
  const startCol = Math.max(0, (span.start_col || 1) - 1);
  const endLine = Math.max(startLine, (span.end_line || span.start_line || 1) - 1);

  let endCol: number;
  if (span.end_col !== undefined && span.end_col !== null && span.end_col > 0) {
    const rawEndCol = span.end_col - 1;
    if (endLine === startLine) {
      endCol = Math.max(startCol, rawEndCol);
    } else {
      endCol = rawEndCol;
    }
  } else {
    endCol = startCol;
  }

  return new vscode.Range(new vscode.Position(startLine, startCol), new vscode.Position(endLine, endCol));
}

/**
 * Formats diagnostic message including remediation guidance if present.
 */
export function formatDiagnosticMessage(finding: DiagnosticFinding): string {
  let msg = finding.message || 'StackVet diagnostic finding';
  if (finding.suggested_remediation && finding.suggested_remediation.trim().length > 0) {
    msg += `\n\nRemediation: ${finding.suggested_remediation.trim()}`;
  }
  return msg;
}

export interface DiagnosticMappingResult {
  diagnosticsByUri: Map<string, vscode.Diagnostic[]>;
  unmappedFindings: DiagnosticFinding[];
}

/**
 * Maps a list of StackVet findings to VS Code Diagnostics grouped by document URI.
 */
export function mapFindingsToDiagnostics(
  findings: DiagnosticFinding[],
  workspaceRoot: string
): DiagnosticMappingResult {
  const diagnosticsByUri = new Map<string, vscode.Diagnostic[]>();
  const unmappedFindings: DiagnosticFinding[] = [];

  for (const finding of findings) {
    const rawPath = finding.location?.file_path;
    if (!rawPath || rawPath.trim() === '' || rawPath === '<unknown>') {
      // Findings without a real source file cannot be placed in a specific file editor
      unmappedFindings.push(finding);
      continue;
    }

    // Resolve path: if relative, resolve against workspaceRoot
    let absolutePath = rawPath;
    if (!path.isAbsolute(absolutePath)) {
      absolutePath = path.resolve(workspaceRoot, absolutePath);
    } else {
      absolutePath = path.normalize(absolutePath);
    }

    const uri = vscode.Uri.file(absolutePath);
    const range = spanToRange(finding.location?.span);
    const severity = mapSeverity(finding.severity);
    const message = formatDiagnosticMessage(finding);

    const diagnostic = new vscode.Diagnostic(range, message, severity);
    diagnostic.source = 'StackVet';
    diagnostic.code = finding.rule_id;

    const uriString = uri.toString();
    const existing = diagnosticsByUri.get(uriString);
    if (existing) {
      existing.push(diagnostic);
    } else {
      diagnosticsByUri.set(uriString, [diagnostic]);
    }
  }

  return {
    diagnosticsByUri,
    unmappedFindings,
  };
}
