# StackVet for Visual Studio Code

The official Visual Studio Code extension for [StackVet](https://github.com/inzamol/stackvet) — the Universal Project Health & Static Diagnostics Engine.

This extension brings StackVet's static analysis, dependency health, framework diagnostics, and security findings directly into your VS Code editor and Problems panel.

---

## Features

- **Automatic Background Scanning**: Scans active files or the entire workspace on open and save with configurable debouncing.
- **Command Palette Integration**: Run `StackVet: Scan Workspace` on demand to audit the current workspace.
- **Problems Panel Diagnostics**: View rule violations with exact rule IDs (e.g. `DEP-001`, `DJG-002`, `FAP-002`), severities, and source code navigation.
- **Inline Remediation Guidance**: Actionable remediation instructions supplied by StackVet are displayed directly on hover and in problem details.
- **Privacy & Security First**: Operates 100% locally on your machine. No source code is uploaded, transmitted, or shared.
- **Zero Duplication**: Leverages your native or npm-installed StackVet CLI directly as the single source of truth.

---

## Prerequisites

The extension resolves the StackVet engine automatically:

1. **User Setting**: Custom path configured via `stackvet.executablePath`.
2. **Bundled Native Binary**: Fast, offline native binary bundled directly inside the extension.
3. **System PATH**: Global `stackvet` command installed via npm, pip, or cargo.
4. **Live `npx` Fallback**: If no local binary is found, automatically runs the live version via `npx -y -p @inzamol/stackvet stackvet`.
5. **Live `pipx` Fallback**: If Node.js / npx is unavailable, automatically runs via `pipx run stackvet`.

You can also install StackVet globally via:
- **npm**: `npm install -g @inzamol/stackvet`
- **pip**: `pip install stackvet`
- **Cargo**: `cargo install --path crates/stackvet-cli`

---

## Configuration

This extension provides the following settings:

| Setting | Type | Default | Description |
|---|---|---|---|
| `stackvet.executablePath` | `string` | `""` | Custom path to the `stackvet` executable. If empty, `stackvet` is resolved from bundled binary or system `PATH`. |
| `stackvet.autoScan` | `boolean` | `true` | Automatically check files and workspace for issues on open, change, and save. |
| `stackvet.scanMode` | `string` | `"openFilesOnly"` | Analysis scope: `'openFilesOnly'` (scans active/saved file for instant feedback) or `'workspace'` (scans entire repository). |
| `stackvet.autoScanOnSave` | `boolean` | `true` | Automatically run StackVet scan whenever a file is saved. |
| `stackvet.autoScanOnOpen` | `boolean` | `true` | Automatically run StackVet scan when a workspace is opened. |

---

## Usage

1. Open a workspace folder or project file in Visual Studio Code.
2. **Automatic Diagnostics**: When `stackvet.autoScan` is enabled (default), diagnostics are automatically updated as you open and save files.
3. **Manual Workspace Scan**: Open the Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`), type and select **`StackVet: Scan Workspace`**.
4. Findings appear immediately in the **Problems** panel (`Ctrl+Shift+M` / `Cmd+Shift+M`) and in the Status Bar.
5. Click on any finding in the Problems panel to jump directly to the offending file, line, and column.

---

## CLI Integration Contract

The extension invokes the StackVet CLI without shell interpolation using:
```bash
stackvet --format json check <workspaceRoot>
```

- **Output Stream**: Parses the JSON diagnostic report from `stdout`.
- **Exit Codes**:
  - `0`: Scan clean (no findings).
  - `1`: Findings violation (findings detected; valid JSON report emitted on `stdout`).
  - `2`: Configuration error (details logged to Output Channel).
  - `3`: Execution error (details logged to Output Channel).
  - `4`: Incomplete analysis (unreadable or malformed files encountered).

Diagnostic severities are mapped as follows:
- `Critical` / `Fatal` / `Error` &rarr; VS Code `Error`
- `Warning` &rarr; VS Code `Warning`
- `Info` / `Information` &rarr; VS Code `Information`
- `Hint` &rarr; VS Code `Hint`

---

## Development & Testing

### Build
```bash
cd editors/vscode
bun install # or npm install
bun run compile
```

### Run Tests
```bash
bun run test # or npm test
```

Unit tests run against realistic StackVet CLI report fixtures and optionally against the local native binary.

---

## License

MIT License.
