import Module from 'module';

export enum DiagnosticSeverity {
  Error = 0,
  Warning = 1,
  Information = 2,
  Hint = 3,
}

export class Position {
  constructor(public readonly line: number, public readonly character: number) {}
}

export class Range {
  constructor(public readonly start: Position, public readonly end: Position) {}
}

export class Diagnostic {
  public source?: string;
  public code?: string | number | { value: string | number; target: any };

  constructor(
    public range: Range,
    public message: string,
    public severity: DiagnosticSeverity = DiagnosticSeverity.Error
  ) {}
}

export class Uri {
  static file(fsPath: string): Uri {
    return new Uri('file', fsPath);
  }

  static parse(uriStr: string): Uri {
    let clean = uriStr;
    if (clean.startsWith('file://')) {
      clean = clean.slice(7);
      if (clean.startsWith('/') && process.platform === 'win32' && /^[a-zA-Z]:/.test(clean.slice(1))) {
        clean = clean.slice(1);
      }
      try {
        clean = decodeURI(clean);
      } catch {}
    }
    return new Uri('file', clean);
  }

  constructor(public readonly scheme: string, public readonly fsPath: string) {}

  toString(): string {
    let norm = this.fsPath.replace(/\\/g, '/');
    if (!norm.startsWith('/')) {
      norm = '/' + norm;
    }
    return `${this.scheme}://${encodeURI(norm)}`;
  }
}

export const mockVscode = {
  DiagnosticSeverity,
  Position,
  Range,
  Diagnostic,
  Uri,
};

// Register in Node module cache so require('vscode') resolves to mockVscode
const originalLoad = (Module as any)._load;
(Module as any)._load = function (request: string, parent: any, isMain: boolean) {
  if (request === 'vscode') {
    return mockVscode;
  }
  return originalLoad.apply(this, arguments);
};
