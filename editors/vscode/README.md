# StackVet for Visual Studio Code

The official Visual Studio Code extension for [StackVet](https://github.com/inzamol/stackvet) — the Universal Project Health & Static Diagnostics Engine.

This extension brings StackVet's static analysis, dependency health, framework diagnostics, and security findings directly into your VS Code editor and Problems panel.

---

## Features

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

This extension provides the following setting:

| Setting | Type | Default | Description |
|---|---|---|---|
| `stackvet.executablePath` | `string` | `""` | Custom path to the `stackvet` executable. If empty, `stackvet` is resolved from system `PATH`. |

---

## Usage

1. Open a workspace folder in Visual Studio Code.
2. Open the Command Palette (`Ctrl+Shift+P` on Windows/Linux, `Cmd+Shift+P` on macOS).
3. Type and select **`StackVet: Scan Workspace`**.
4. The scan will execute and report findings in the **Problems** panel (`Ctrl+Shift+M` / `Cmd+Shift+M`).
5. Click on any finding in the Problems panel to jump to the offending file, line, and column.

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
