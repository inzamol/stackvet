import { ChildProcess, spawn } from 'child_process';
import * as fs from 'fs';
import * as path from 'path';
import { parseStackvetOutput } from './parser';
import { StackvetReport } from './types';

export interface CancellationTokenLike {
  readonly isCancellationRequested: boolean;
  onCancellationRequested(listener: () => void): { dispose(): void };
}

export interface RunOptions {
  executablePath?: string;
  args?: string[];
  workspaceRoot: string;
  timeoutMs?: number;
  extensionPath?: string;
  allowLiveFallback?: boolean;
  cancellationToken?: CancellationTokenLike;
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

function getExtensionVersion(extensionPath?: string): string {
  const candidatePaths = [
    extensionPath ? path.join(extensionPath, 'package.json') : null,
    path.join(__dirname, '..', '..', 'package.json'),
    path.join(__dirname, '..', 'package.json'),
  ].filter((p): p is string => Boolean(p));

  for (const candidate of candidatePaths) {
    try {
      if (fs.existsSync(candidate)) {
        const pkg = JSON.parse(fs.readFileSync(candidate, 'utf8'));
        if (typeof pkg.version === 'string' && pkg.version.trim()) {
          return pkg.version.trim();
        }
      }
    } catch {
      // ignore
    }
  }
  return '0.2.1';
}

/**
 * Resolves the candidate execution targets in order of preference:
 * 1. User setting (customPath) if provided and non-empty.
 * 2. Bundled binary inside the extension's bin/ directory if present.
 * 3. Default 'stackvet' resolved from system PATH.
 * 4. (Optional opt-in) Live fallback via npx (@inzamol/stackvet@<version>).
 * 5. (Optional opt-in) Live fallback via pipx (stackvet==<version>).
 */
export function resolveExecutionTargets(
  customPath?: string,
  extensionPath?: string,
  workspaceRoot?: string,
  customArgs?: string[],
  allowLiveFallback: boolean = false
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

  // 4 & 5. Live fallbacks (opt-in only)
  if (allowLiveFallback) {
    const version = getExtensionVersion(extensionPath);
    const npxCmd = process.platform === 'win32' ? 'npx.cmd' : 'npx';
    targets.push({
      name: `Live npx (@inzamol/stackvet@${version})`,
      command: npxCmd,
      args: ['-y', '-p', `@inzamol/stackvet@${version}`, 'stackvet', '--format', 'json', 'check', ws],
    });

    const pipxCmd = process.platform === 'win32' ? 'pipx.exe' : 'pipx';
    targets.push({
      name: `Live pipx (stackvet==${version})`,
      command: pipxCmd,
      args: ['run', `stackvet==${version}`, '--format', 'json', 'check', ws],
    });
  }

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
          cleanup();
          reject(new Error(`StackVet execution timed out after ${options.timeoutMs}ms.`));
        }
      }, options.timeoutMs);
    }

    let cancellationSub: { dispose(): void } | undefined;
    if (options.cancellationToken) {
      if (options.cancellationToken.isCancellationRequested) {
        return reject(new Error('StackVet scan was cancelled.'));
      }
      cancellationSub = options.cancellationToken.onCancellationRequested(() => {
        if (!isSettled) {
          isSettled = true;
          try {
            if (child) {
              child.kill();
            }
          } catch {
            // ignore
          }
          cleanup();
          reject(new Error('StackVet scan was cancelled.'));
        }
      });
    }

    const cleanup = () => {
      if (timer) {
        clearTimeout(timer);
        timer = undefined;
      }
      if (cancellationSub) {
        cancellationSub.dispose();
        cancellationSub = undefined;
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

    const isWindows = process.platform === 'win32';
    const isBatchShim =
      isWindows &&
      (command.toLowerCase().endsWith('.cmd') || command.toLowerCase().endsWith('.bat'));

    const spawnCommand = isBatchShim ? (process.env.ComSpec || 'cmd.exe') : command;
    const spawnArgs = isBatchShim
      ? ['/d', '/s', '/c', command, ...effectiveArgs]
      : effectiveArgs;

    try {
      child = spawn(spawnCommand, spawnArgs, {
        cwd: effectiveCwd,
        shell: false,
        windowsHide: true,
      });
    } catch (err: any) {
      cleanup();
      const isNotFound = err?.code === 'ENOENT' || err?.code === 'EINVAL';
      const pErr: ProcessError = new Error(
        isNotFound
          ? `Executable '${command}' was not found or could not be invoked.`
          : `Failed to spawn StackVet process: ${err?.message || String(err)}`
      );
      pErr.isNotFound = isNotFound;
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

      const isNotFound = err.code === 'ENOENT' || err.code === 'EINVAL';
      const pErr: ProcessError = new Error(
        isNotFound
          ? `Executable '${command}' was not found or could not be invoked.`
          : `Failed to run '${command}': ${err.message}`
      );
      pErr.isNotFound = isNotFound;
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
 * Tries local/bundled binary first, and falls back to live execution via npx or pipx if allowed.
 */
export async function runStackvetCli(options: RunOptions): Promise<RunResult> {
  const targets = resolveExecutionTargets(
    options.executablePath,
    options.extensionPath,
    options.workspaceRoot,
    options.args,
    options.allowLiveFallback
  );
  let lastError: Error | undefined;

  for (let i = 0; i < targets.length; i++) {
    const target = targets[i];
    try {
      if (i > 0 && options.onLog) {
        options.onLog(`StackVet executable not found locally. Falling back to execution via ${target.name}...`);
      }
      const result = await executeProcess(target.command, target.args, options);
      result.usedTarget = target.name;
      return result;
    } catch (err: any) {
      lastError = err;
      // If error is notFound (ENOENT or EINVAL), proceed to next target
      if (err.isNotFound) {
        continue;
      }
      // If command was found and exited with error (or cancelled/timeout), do not swallow it
      throw err;
    }
  }

  if (options.executablePath && options.executablePath.trim().length > 0) {
    throw lastError || new Error(`Configured executable '${options.executablePath}' could not be executed.`);
  }

  const guidance = options.allowLiveFallback
    ? "StackVet executable was not found locally, and live execution via npx and pipx failed. Please install StackVet ('npm install -g @inzamol/stackvet' or 'pip install stackvet') or configure 'stackvet.executablePath' in VS Code settings."
    : "StackVet executable was not found. Please install StackVet ('npm install -g @inzamol/stackvet' or 'pip install stackvet'), configure 'stackvet.executablePath' in VS Code settings, or enable 'stackvet.allowLiveFallback'.";

  throw new Error(guidance);
}
