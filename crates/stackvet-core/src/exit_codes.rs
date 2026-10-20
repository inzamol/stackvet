/// Standardized process exit codes for StackVet automation and CI/CD pipelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    /// Command completed successfully with no violations.
    Success = 0,
    /// Findings detected that meet or exceed configured thresholds (or baseline regressions).
    FindingsViolation = 1,
    /// Configuration syntax error, invalid rule ID, or unsupported severity override.
    ConfigurationError = 2,
    /// Execution or operational I/O error (path not found, missing baseline file, read error).
    ExecutionError = 3,
    /// Incomplete analysis due to malformed files, unreadable files, or resource limit exhaustion.
    IncompleteAnalysis = 4,
}

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    pub fn exit(self) -> ! {
        std::process::exit(self.as_i32())
    }
}
