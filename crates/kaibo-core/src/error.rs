//! Shared exit-code and error contract.
//!
//! Errors are instructions: every error message should name the exact next
//! command the user needs to run, where one exists, instead of only
//! describing what went wrong. See [`crate::config::ConfigError::NoHomeDir`]
//! for an example that does this.

/// Process exit codes, shared by every verb.
///
/// This enum is the single source of truth for the mapping; the binary is
/// the only place a value here turns into an actual process exit. Core only
/// ever hands one back as data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// Success.
    Success = 0,
    /// A bug in kaibo itself, not something the user's input or environment caused.
    Internal = 1,
    /// Bad input: a malformed flag, a broken config file, a missing
    /// prerequisite the user can fix.
    Usage = 2,
    /// A verb ran cleanly but found nothing useful. The gap signal.
    NoHits = 3,
    /// The local corpus clone or index is missing, unsynced, or stale beyond
    /// the configured threshold.
    Stale = 4,
}

impl ExitCode {
    /// The raw process exit status for this code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// An error that knows which process exit code it maps to.
///
/// Every error type surfaced across the process boundary (config, corpus,
/// verb results) implements this so the binary can map error to exit code in
/// one place, without matching on error variants itself.
pub trait ExitCoded {
    fn exit_code(&self) -> ExitCode;
}
