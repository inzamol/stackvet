/**
 * Types representing StackVet CLI JSON output schema.
 * Source of truth: crates/stackvet-reporter and crates/stackvet-core.
 */

export interface TextSpan {
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
}

export interface DiagnosticLocation {
  file_path: string;
  span?: TextSpan | null;
  snippet?: string | null;
  message?: string | null;
}

export type DiagnosticSeverityLevel =
  | 'Critical'
  | 'Error'
  | 'Warning'
  | 'Info'
  | 'Hint'
  | string;

export interface FixAction {
  description: string;
  file_path: string;
  replacement: string;
  span?: TextSpan | null;
}

export interface DiagnosticFinding {
  rule_id: string;
  message: string;
  severity: DiagnosticSeverityLevel;
  location: DiagnosticLocation;
  explanation?: string;
  suggested_remediation?: string;
  fix?: FixAction | null;
  language?: string;
  confidence?: string;
  related_locations?: DiagnosticLocation[];
}

export interface ReportSummary {
  total_issues: number;
  critical_count: number;
  error_count: number;
  warning_count: number;
  info_count: number;
  hint_count: number;
  fixable_count: number;
  is_complete?: boolean;
  incomplete_reasons?: string[];
}

export interface StackvetReport {
  schema_version?: string;
  health_score?: number;
  grade?: string;
  summary?: ReportSummary;
  diagnostics: DiagnosticFinding[];
}
