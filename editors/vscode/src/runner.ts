import { ChildProcess, spawn } from 'child_process';
import * as fs from 'fs';
import * as path from 'path';
import { parseStackvetOutput } from './parser';
import { StackvetReport } from './types';

export interface RunOptions {
  executablePath?: string;
  args?: string[];
  workspaceRoot: string;
  timeoutMs?: number;
  extensionPath?: string;
  onLog?: (message: string) => void;
}

export interface RunResult {
  report: StackvetReport;
  stdout: string;
  stderr: string;
  exitCode: number;
  usedTarget?: string;
}

export interface ExecutionTarget {
  name: string;
  command: string;
  args: string[];
}

/**
 * Resolves the candidate execution targets in order of preference:
 * 1. User setting (customPath) if provided and non-empty.
 * 2. Bundled binary inside the extension's bin/ directory if present.
 * 3. Default 'stackvet' resolved from system PATH.
 * 4. Live fallback via npx (@inzamol/stackvet).
 * 5. Live fallback via pipx (stackvet).
 */
export function resolveExecutionTargets(
  customPath?: string,
  extensionPath?: string,
  workspaceRoot?: string,
  customArgs?: string[]
): ExecutionTarget[] {
  const ws = workspaceRoot || '.';
  const targets: ExecutionTarget[] = [];

  // 1. Explicit user-configured path
  if (customPath && customPath.trim().length > 0) {
    targets.push({
      name: 'Custom Executable',
      command: customPath.trim(),
      args: customArgs || ['--format', 'json', 'check', ws],
    });
    return targets;
  }

  const binName = process.platform === 'win32' ? 'stackvet.exe' : 'stackvet';

  // 2. Bundled binary inside extension directory
  if (extensionPath) {
    const candidate = path.join(extensionPath, 'bin', binName);
    if (fs.existsSync(candidate)) {
      targets.push({
        name: 'Bundled Binary',
        command: candidate,
        args: ['--format', 'json', 'check', ws],
      });
    }
  }

  const relativeBundled = path.join(__dirname, '..', '..', 'bin', binName);
  if (fs.existsSync(relativeBundled)) {
    if (!targets.some((t) => t.command === relativeBundled)) {
      targets.push({
        name: 'Bundled Binary',
        command: relativeBundled,
        args: ['--format', 'json', 'check', ws],
      });
    }
  }

  // 3. System PATH binary
  targets.push({
    name: 'System PATH (stackvet)',
    command: 'stackvet',
    args: ['--format', 'json', 'check', ws],
  });

  // 4. Live fallback via npx (@inzamol/stackvet)
  const npxCmd = process.platform === 'win32' ? 'npx.cmd' : 'npx';
  targets.push({
    name: 'Live npx (@inzamol/stackvet)',
    command: npxCmd,
    args: ['-y', '-p', '@inzamol/stackvet', 'stackvet', '--format', 'json', 'check', ws],
  });

  // 5. Live fallback via pipx (stackvet)
  const pipxCmd = process.platform === 'win32' ? 'pipx.exe' : 'pipx';
  targets.push({
    name: 'Live pipx (stackvet)',
    command: pipxCmd,
    args: ['run', 'stackvet', '--format', 'json', 'check', ws],
  });

  return targets;
}

/**
 * Resolves the primary StackVet executable command/path.
 */
export function resolveExecutable(customPath?: string, extensionPath?: string): string {
  const targets = resolveExecutionTargets(customPath, extensionPath);
  return targets[0]?.command || 'stackvet';
}

interface ProcessError extends Error {
  isNotFound?: boolean;
}

function executeProcess(
  command: string,
  args: string[],
  options: RunOptions
): Promise<RunResult> {
  return new Promise((resolve, reject) => {
    let stdoutData = '';
    let stderrData = '';
    let isSettled = false;
    let child: ChildProcess;

    let timer: NodeJS.Timeout | undefined;
    if (options.timeoutMs && options.timeoutMs > 0) {
      timer = setTimeout(() => {
        if (!isSettled) {
          isSettled = true;
          try {
            if (child) {
              child.kill();
            }
          } catch {
            // ignore
          }
          reject(new Error(`StackVet execution timed out after ${options.timeoutMs}ms.`));
        }
      }, options.timeoutMs);
    }

    const cleanup = () => {
      if (timer) {
        clearTimeout(timer);
        timer = undefined;
      }
    };

    let effectiveCwd = options.workspaceRoot;
    let effectiveArgs = [...args];
    try {
      if (options.workspaceRoot && fs.existsSync(options.workspaceRoot)) {
        const stat = fs.statSync(options.workspaceRoot);
        if (!stat.isDirectory()) {
          const resolvedTarget = path.resolve(options.workspaceRoot);
          effectiveArgs = effectiveArgs.map((arg) =>
            arg === options.workspaceRoot ? resolvedTarget : arg
          );
          effectiveCwd = path.dirname(resolvedTarget);
        }
      }
    } catch {
      // Path inspection failed, keep options.workspaceRoot and original args
    }

    try {
      child = spawn(command, effectiveArgs, {
        cwd: effectiveCwd,
        shell: false,
        windowsHide: true,
      });
    } catch (err: any) {
      cleanup();
      const pErr: ProcessError = new Error(`Failed to spawn StackVet process: ${err?.message || String(err)}`);
      pErr.isNotFound = err?.code === 'ENOENT';
      return reject(pErr);
    }

    if (child.stdout) {
      child.stdout.on('data', (chunk: Buffer | string) => {
        stdoutData += chunk.toString();
      });
    }

    if (child.stderr) {
      child.stderr.on('data', (chunk: Buffer | string) => {
        stderrData += chunk.toString();
      });
    }

    child.on('error', (err: NodeJS.ErrnoException) => {
      if (isSettled) {
        return;
      }
      isSettled = true;
      cleanup();

      const pErr: ProcessError = new Error(
        err.code === 'ENOENT'
          ? `Executable '${command}' was not found.`
          : `Failed to run '${command}': ${err.message}`
      );
      pErr.isNotFound = err.code === 'ENOENT';
      reject(pErr);
    });

    child.on('close', (code, signal) => {
      if (isSettled) {
        return;
      }
      isSettled = true;
      cleanup();

      const exitCode = code !== null ? code : (signal ? 128 : 0);

      // Exit codes 0 (Success) and 1 (FindingsViolation) emit standard JSON report on stdout.
      if (exitCode === 0 || exitCode === 1) {
        try {
          const report = parseStackvetOutput(stdoutData);
          return resolve({
            report,
            stdout: stdoutData,
            stderr: stderrData,
            exitCode,
          });
        } catch (parseErr) {
          const stderrSummary = stderrData.trim() ? ` Stderr: ${stderrData.trim()}` : '';
          return reject(
            new Error(
              `StackVet completed with exit code ${exitCode}, but output could not be parsed: ${
                parseErr instanceof Error ? parseErr.message : String(parseErr)
              }.${stderrSummary}`
            )
          );
        }
      }

      // If exit code is not 0 or 1, check if stdout contains valid JSON (e.g. exit code 4 IncompleteAnalysis)
      if (stdoutData.trim().length > 0) {
        try {
          const report = parseStackvetOutput(stdoutData);
          return resolve({
            report,
            stdout: stdoutData,
            stderr: stderrData,
            exitCode,
          });
        } catch {
          // Fall through to error
        }
      }

      const stderrMsg = stderrData.trim();
      const errorDetail = stderrMsg
        ? `: ${stderrMsg} (exit code ${exitCode})`
        : ` (exit code ${exitCode})`;
      return reject(new Error(`StackVet failed${errorDetail}`));
    });
  });
}

/**
 * Executes the StackVet CLI against the target workspace in JSON mode.
 * Tries local/bundled binary first, and falls back to live execution via npx or pipx.
 */
export async function runStackvetCli(options: RunOptions): Promise<RunResult> {
  const targets = resolveExecutionTargets(
    options.executablePath,
    options.extensionPath,
    options.workspaceRoot,
    options.args
  );
  let lastError: Error | undefined;

  for (let i = 0; i < targets.length; i++) {
    const target = targets[i];
    try {
      if (i > 0 && options.onLog) {
        options.onLog(`StackVet executable not found locally. Falling back to live execution via ${target.name}...`);
      }
      const result = await executeProcess(target.command, target.args, options);
      result.usedTarget = target.name;
      return result;
    } catch (err: any) {
      lastError = err;
      // If error is ENOENT (command not found on system), proceed to next target
      if (err.isNotFound) {
        continue;
      }
      // If command was found and exited with error, do not swallow it
      throw err;
    }
  }

  throw (
    lastError ||
    new Error(
      "StackVet executable was not found locally, and live execution via npx and pipx failed. Please install StackVet ('npm install -g @inzamol/stackvet' or 'pip install stackvet') or configure 'stackvet.executablePath' in VS Code settings."
    )
  );
}
