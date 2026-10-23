pub static NODE_BUILTINS: &[&str] = &[
    "assert",
    "assert/strict",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "dns/promises",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "inspector/promises",
    "module",
    "net",
    "os",
    "path",
    "path/posix",
    "path/win32",
    "navigator",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "readline/promises",
    "repl",
    "sea",
    "sqlite",
    "stream",
    "stream/promises",
    "stream/consumers",
    "stream/web",
    "string_decoder",
    "sys",
    "test",
    "test/reporters",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "util/types",
    "v8",
    "vm",
    "wasi",
    "websocket",
    "worker_threads",
    "zlib",
];

pub fn is_node_builtin(specifier: &str) -> bool {
    // 1. All 'node:' prefixed specifiers are reserved for Node.js core modules
    if specifier.starts_with("node:") {
        return true;
    }
    // 2. Bun runtime built-ins
    if specifier == "bun" || specifier.starts_with("bun:") {
        return true;
    }
    // 3. Deno runtime built-ins
    if specifier == "deno" || specifier.starts_with("deno:") {
        return true;
    }
    // 4. Host/platform runtime ambient APIs (injected by host environment, never in npm dependencies)
    if specifier == "vscode" || specifier == "electron" || specifier == "original-fs" {
        return true;
    }
    let spec = specifier.strip_prefix("node:").unwrap_or(specifier);
    NODE_BUILTINS.contains(&spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_builtins() {
        assert!(is_node_builtin("fs"));
        assert!(is_node_builtin("fs/promises"));
        assert!(is_node_builtin("node:fs"));
        assert!(is_node_builtin("node:fs/promises"));
        assert!(is_node_builtin("test"));
        assert!(is_node_builtin("test/reporters"));
        assert!(is_node_builtin("node:test"));
        assert!(is_node_builtin("node:test/reporters"));
        assert!(is_node_builtin("node:sqlite"));
        assert!(is_node_builtin("node:sea"));
    }

    #[test]
    fn test_runtime_and_platform_builtins() {
        assert!(is_node_builtin("vscode"));
        assert!(is_node_builtin("electron"));
        assert!(is_node_builtin("bun"));
        assert!(is_node_builtin("bun:test"));
        assert!(is_node_builtin("bun:sqlite"));
        assert!(is_node_builtin("deno"));
        assert!(is_node_builtin("deno:path"));
    }

    #[test]
    fn test_third_party_packages_not_builtins() {
        assert!(!is_node_builtin("react"));
        assert!(!is_node_builtin("axios"));
        assert!(!is_node_builtin("lodash"));
        assert!(!is_node_builtin("@types/node"));
    }
}
