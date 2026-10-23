import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { mapFindingsToDiagnostics } from './diagnostics';
import { runStackvetCli } from './runner';
import { DiagnosticFinding } from './types';

let diagnosticCollection: vscode.DiagnosticCollection;
let outputChannel: vscode.OutputChannel;
let statusBarItem: vscode.StatusBarItem;
let isScanning = false;
const pendingScans = new Map<string, boolean>();
let autoScanDebounceTimer: NodeJS.Timeout | undefined;

/**
 * Performs a workspace or single-file scan and updates diagnostics and status.
 */
async function performScan(
  targetPath: string,
  extensionPath: string,
  isManual: boolean = false
): Promise<void> {
  if (isScanning) {
    // Queue a follow-up scan for this target path once the current scan finishes, preserving manual flag
    const existingManual = pendingScans.get(targetPath) ?? false;
    pendingScans.set(targetPath, isManual || existingManual);
    return;
  }

  isScanning = true;
  if (statusBarItem) {
    statusBarItem.text = '$(sync~spin) StackVet: Scanning...';
    statusBarItem.tooltip = `Scanning ${targetPath}`;
  }

  const config = vscode.workspace.getConfiguration('stackvet');
  const customExecutable = config.get<string>('executablePath');
  const triggerType = isManual ? 'Manual' : 'Automatic';

  outputChannel.appendLine(
    `[${new Date().toLocaleTimeString()}] [${triggerType}] Starting StackVet scan on: ${targetPath}`
  );

  const executeScan = async () => {
    try {
      const result = await runStackvetCli({
        executablePath: customExecutable,
        workspaceRoot: targetPath,
        extensionPath,
        onLog: (msg) => outputChannel.appendLine(`[Runner] ${msg}`),
      });

      // Clear stale diagnostics belonging to this target path
      const normalizedTarget = targetPath.replace(/\\/g, '/').toLowerCase();
      diagnosticCollection.forEach((uri) => {
        const uriPath = uri.fsPath.replace(/\\/g, '/').toLowerCase();
        if (uriPath === normalizedTarget || uriPath.startsWith(normalizedTarget + '/')) {
          diagnosticCollection.delete(uri);
        }
      });

      // Resolve effective base directory for relative diagnostic paths
      const wsFolder = vscode.workspace.getWorkspaceFolder(vscode.Uri.file(targetPath));
      const baseDir = resolveDiagnosticBaseDir(
        targetPath,
        wsFolder ? wsFolder.uri.fsPath : undefined,
        result.report.diagnostics
      );

      const { diagnosticsByUri, unmappedFindings } = mapFindingsToDiagnostics(
        result.report.diagnostics,
        baseDir
      );

      for (const [uriString, fileDiagnostics] of diagnosticsByUri.entries()) {
        diagnosticCollection.set(vscode.Uri.parse(uriString), fileDiagnostics);
      }

      // Calculate total findings across the workspace diagnostic collection
      let totalWorkspaceFindings = 0;
      diagnosticCollection.forEach((_, diags) => {
        totalWorkspaceFindings += diags.length;
      });

      const batchFindings = result.report.diagnostics.length;
      outputChannel.appendLine(
        `[${new Date().toLocaleTimeString()}] Scan complete for ${targetPath}. Findings: ${batchFindings} (Active issues: ${totalWorkspaceFindings}, Exit code: ${result.exitCode})`
      );

      if (statusBarItem) {
        if (totalWorkspaceFindings === 0) {
          statusBarItem.text = '$(check) StackVet: Clean';
          statusBarItem.tooltip = 'StackVet: No issues detected in active files. Click to re-scan.';
        } else {
          statusBarItem.text = `$(alert) StackVet: ${totalWorkspaceFindings}`;
          statusBarItem.tooltip = `StackVet found ${totalWorkspaceFindings} issue(s) in active files. Click to re-scan.`;
        }
      }

      if (unmappedFindings.length > 0) {
        outputChannel.appendLine(
          `Notice: ${unmappedFindings.length} finding(s) have no specific source file:`
        );
        for (const uf of unmappedFindings) {
          outputChannel.appendLine(`  - [${uf.rule_id}] ${uf.severity}: ${uf.message}`);
        }
      }

      if (result.stderr.trim()) {
        outputChannel.appendLine(`CLI output notices:\n${result.stderr.trim()}`);
      }

      if (isManual) {
        if (totalWorkspaceFindings === 0) {
          vscode.window.showInformationMessage('StackVet: Scan complete. No issues found.');
        } else {
          vscode.window.showInformationMessage(`StackVet: Scan complete. Found ${totalWorkspaceFindings} issue(s).`);
        }
      }
    } catch (err: any) {
      const msg = err instanceof Error ? err.message : String(err);
      outputChannel.appendLine(`[${new Date().toLocaleTimeString()}] Scan failed: ${msg}`);
      if (statusBarItem) {
        statusBarItem.text = '$(error) StackVet: Error';
        statusBarItem.tooltip = `StackVet scan error: ${msg}. Click to retry.`;
      }
      if (isManual) {
        vscode.window.showErrorMessage(`StackVet: ${msg}`);
      }
    } finally {
      isScanning = false;

      // Drain queued scans in order, preserving manual scan intent
      if (pendingScans.size > 0) {
        const nextEntry = pendingScans.entries().next().value;
        if (nextEntry) {
          const [nextTarget, nextIsManual] = nextEntry;
          pendingScans.delete(nextTarget);
          performScan(nextTarget, extensionPath, nextIsManual);
        }
      }
    }
  };

  if (isManual) {
    await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: 'StackVet: Scanning workspace...',
        cancellable: false,
      },
      executeScan
    );
  } else {
    await executeScan();
  }
}

export function activate(context: vscode.ExtensionContext): void {
  diagnosticCollection = vscode.languages.createDiagnosticCollection('stackvet');
  outputChannel = vscode.window.createOutputChannel('StackVet');

  statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  statusBarItem.command = 'stackvet.scanWorkspace';
  statusBarItem.text = '$(shield) StackVet';
  statusBarItem.tooltip = 'Click to run StackVet scan';
  statusBarItem.show();

  context.subscriptions.push(diagnosticCollection);
  context.subscriptions.push(outputChannel);
  context.subscriptions.push(statusBarItem);

  // Helper to resolve workspace root for manual full workspace trigger
  const resolveTargetWorkspace = (): string | undefined => {
    const folders = vscode.workspace.workspaceFolders;
    if (folders && folders.length > 0) {
      const activeDoc = vscode.window.activeTextEditor?.document;
      if (activeDoc) {
        const match = vscode.workspace.getWorkspaceFolder(activeDoc.uri);
        if (match) {
          return match.uri.fsPath;
        }
      }
      return folders[0].uri.fsPath;
    }
    const activeDoc = vscode.window.activeTextEditor?.document;
    if (activeDoc && activeDoc.uri.scheme === 'file') {
      return path.dirname(activeDoc.uri.fsPath);
    }
    return undefined;
  };

  // Helper to trigger debounced auto-scanning
  const triggerAutoScan = (documentUri?: vscode.Uri, delayMs = 300) => {
    const config = vscode.workspace.getConfiguration('stackvet');
    if (!config.get<boolean>('autoScan', true)) {
      return;
    }

    const scanMode = config.get<string>('scanMode', 'openFilesOnly');

    if (autoScanDebounceTimer) {
      clearTimeout(autoScanDebounceTimer);
    }

    autoScanDebounceTimer = setTimeout(() => {
      const targetPaths: string[] = [];

      if (documentUri && documentUri.scheme === 'file') {
        if (scanMode === 'openFilesOnly') {
          // Pyright-style scoped scan: analyze only this specific file!
          targetPaths.push(documentUri.fsPath);
        } else {
          // Full workspace mode: analyze enclosing folder
          const folder = vscode.workspace.getWorkspaceFolder(documentUri);
          targetPaths.push(folder ? folder.uri.fsPath : path.dirname(documentUri.fsPath));
        }
      } else {
        // No specific document passed (e.g. startup / folder open)
        const visibleFiles = vscode.window.visibleTextEditors
          .map((e) => e.document)
          .filter((d) => d.uri.scheme === 'file')
          .map((d) => d.uri.fsPath);

        if (scanMode === 'openFilesOnly') {
          if (visibleFiles.length > 0) {
            targetPaths.push(...visibleFiles);
          } else {
            const activeDoc = vscode.window.activeTextEditor?.document;
            if (activeDoc && activeDoc.uri.scheme === 'file') {
              targetPaths.push(activeDoc.uri.fsPath);
            }
          }
        } else {
          const folders = vscode.workspace.workspaceFolders;
          if (folders && folders.length > 0) {
            for (const f of folders) {
              targetPaths.push(f.uri.fsPath);
            }
          } else if (vscode.window.activeTextEditor?.document.uri.scheme === 'file') {
            targetPaths.push(path.dirname(vscode.window.activeTextEditor.document.uri.fsPath));
          }
        }
      }

      const uniquePaths = Array.from(new Set(targetPaths));
      for (const target of uniquePaths) {
        performScan(target, context.extensionPath, false);
      }
    }, delayMs);
  };

  // 1. Manual Scan Command (Full Workspace Scan)
  const scanCommand = vscode.commands.registerCommand('stackvet.scanWorkspace', async () => {
    const targetRoot = resolveTargetWorkspace();
    if (!targetRoot) {
      vscode.window.showWarningMessage('StackVet: A workspace folder or file must be open to scan.');
      return;
    }
    await performScan(targetRoot, context.extensionPath, true);
  });
  context.subscriptions.push(scanCommand);

  // 2. Automatic Scan on Save (Rescans only the saved file in openFilesOnly mode)
  const onSaveDisposable = vscode.workspace.onDidSaveTextDocument((doc) => {
    const config = vscode.workspace.getConfiguration('stackvet');
    if (!config.get<boolean>('autoScanOnSave', true)) {
      return;
    }
    if (doc.uri.scheme === 'file') {
      triggerAutoScan(doc.uri, 200);
    }
  });
  context.subscriptions.push(onSaveDisposable);

  // 3. Automatic Scan on Document Open (Rescans only the opened file in openFilesOnly mode)
  const onOpenDisposable = vscode.workspace.onDidOpenTextDocument((doc) => {
    if (doc.uri.scheme === 'file') {
      triggerAutoScan(doc.uri, 300);
    }
  });
  context.subscriptions.push(onOpenDisposable);

  // 4. Automatic Scan on Active Editor Switch
  const onEditorChangeDisposable = vscode.window.onDidChangeActiveTextEditor((editor) => {
    if (editor?.document.uri.scheme === 'file') {
      triggerAutoScan(editor.document.uri, 400);
    }
  });
  context.subscriptions.push(onEditorChangeDisposable);

  // 5. Automatic Scan on Workspace Folder Changes
  const onFoldersChangeDisposable = vscode.workspace.onDidChangeWorkspaceFolders(() => {
    triggerAutoScan(undefined, 200);
  });
  context.subscriptions.push(onFoldersChangeDisposable);

  // 6. Automatic Scan on Configuration Change
  const onConfigChangeDisposable = vscode.workspace.onDidChangeConfiguration((e) => {
    if (e.affectsConfiguration('stackvet')) {
      triggerAutoScan(undefined, 100);
    }
  });
  context.subscriptions.push(onConfigChangeDisposable);

  // 7. Invalidate marker cache on marker file creation / deletion
  try {
    const markerPattern = `**/{${PROJECT_MARKERS.join(',')}}`;
    const markerWatcher = vscode.workspace.createFileSystemWatcher(markerPattern);
    markerWatcher.onDidCreate(() => clearMarkerCache());
    markerWatcher.onDidDelete(() => clearMarkerCache());
    context.subscriptions.push(markerWatcher);
  } catch {
    // Ignore in environments without workspace watcher
  }

  // 8. Initial Automatic Scan on Activation
  const config = vscode.workspace.getConfiguration('stackvet');
  if (config.get<boolean>('autoScan', true) && config.get<boolean>('autoScanOnOpen', true)) {
    triggerAutoScan(undefined, 300);
  }
}

export function deactivate(): void {
  if (autoScanDebounceTimer) {
    clearTimeout(autoScanDebounceTimer);
    autoScanDebounceTimer = undefined;
  }
  if (diagnosticCollection) {
    diagnosticCollection.clear();
    diagnosticCollection.dispose();
  }
  if (outputChannel) {
    outputChannel.dispose();
  }
  if (statusBarItem) {
    statusBarItem.dispose();
  }
  clearMarkerCache();
}

interface MarkerCacheEntry {
  root: string | null;
  mtimeMs: number;
}

const markerCache = new Map<string, MarkerCacheEntry>();

export function clearMarkerCache(): void {
  markerCache.clear();
}

export const PROJECT_MARKERS = [
  'pyproject.toml',
  'package.json',
  'Cargo.toml',
  'setup.py',
  'setup.cfg',
  'requirements.txt',
  'stackvet.toml',
  '.stackvet.toml',
  '.git',
];

/**
 * Walks up directory hierarchy looking for project markers, caching results per directory
 * and invalidating cache when directory mtime changes (e.g. when markers are added/removed).
 */
export function findNestedProjectRoot(
  targetDir: string,
  stopDir?: string,
  markers: string[] = PROJECT_MARKERS
): string | undefined {
  let currentDir = path.resolve(targetDir);
  const resolvedStopDir = stopDir ? path.resolve(stopDir).toLowerCase() : undefined;

  while (currentDir) {
    let currentMtime = 0;
    try {
      currentMtime = fs.statSync(currentDir).mtimeMs;
    } catch {
      break;
    }

    const cached = markerCache.get(currentDir);
    if (cached !== undefined && cached.mtimeMs === currentMtime) {
      if (cached.root !== null) {
        return cached.root;
      }
    } else {
      let found = false;
      for (const marker of markers) {
        try {
          if (fs.existsSync(path.join(currentDir, marker))) {
            found = true;
            break;
          }
        } catch {
          // Ignore filesystem errors
        }
      }
      if (found) {
        markerCache.set(currentDir, { root: currentDir, mtimeMs: currentMtime });
        return currentDir;
      }
      markerCache.set(currentDir, { root: null, mtimeMs: currentMtime });
    }

    if (resolvedStopDir && currentDir.toLowerCase() === resolvedStopDir) {
      break;
    }
    const parent = path.dirname(currentDir);
    if (parent === currentDir) {
      break;
    }
    currentDir = parent;
  }
  return undefined;
}

/**
 * Resolves the effective base directory for relative diagnostic paths.
 * When target is a file, checks whether relative findings in the report correspond
 * to the scanned target, a sibling file, or a manifest under the nested project root.
 */
export function resolveDiagnosticBaseDir(
  targetPath: string,
  workspaceFolderPath: string | undefined,
  diagnostics: DiagnosticFinding[],
  isFileTarget?: boolean
): string {
  const isFile =
    isFileTarget !== undefined
      ? isFileTarget
      : (() => {
          try {
            return fs.existsSync(targetPath) && !fs.statSync(targetPath).isDirectory();
          } catch {
            return false;
          }
        })();

  let baseDir = workspaceFolderPath || (isFile ? path.dirname(targetPath) : targetPath);

  if (isFile) {
    const nestedRoot = findNestedProjectRoot(path.dirname(targetPath), workspaceFolderPath);
    if (nestedRoot) {
      const normalizedTarget = path.resolve(targetPath).replace(/\\/g, '/').toLowerCase();
      const normalizedNestedRoot = path.resolve(nestedRoot).replace(/\\/g, '/').toLowerCase();

      const hasRelativeFindingInNestedRoot = diagnostics.some((d) => {
        const fp = d.location?.file_path;
        if (fp && !path.isAbsolute(fp)) {
          const resolvedCandidate = path.resolve(nestedRoot, fp);
          const normalizedCandidate = resolvedCandidate.replace(/\\/g, '/').toLowerCase();
          if (normalizedCandidate === normalizedTarget) {
            return true;
          }
          if (normalizedCandidate.startsWith(normalizedNestedRoot + '/')) {
            try {
              return fs.existsSync(resolvedCandidate);
            } catch {
              return false;
            }
          }
        }
        return false;
      });

      if (hasRelativeFindingInNestedRoot) {
        baseDir = nestedRoot;
      }
    }
  }

  return baseDir;
}


