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
    return new Uri('file', uriStr);
  }

  constructor(public readonly scheme: string, public readonly fsPath: string) {}

  toString(): string {
    return `${this.scheme}://${this.fsPath.replace(/\\/g, '/')}`;
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
