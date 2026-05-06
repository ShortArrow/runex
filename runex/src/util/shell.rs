//! Shell detection and `--shell` flag resolution.
//!
//! Two functions, layered: `resolve_shell` is what the handlers
//! call; it forwards an explicit `--shell` value to `Shell::parse`
//! and falls back to `detect_shell` when the flag is absent.

use std::path::Path;

use crate::domain::shell::Shell;

/// Infer the current shell from environment variables.
///
/// On Unix, reads `$SHELL`. On Windows, the presence of `PSModulePath`
/// indicates a PowerShell parent process.
pub(crate) fn detect_shell() -> Option<Shell> {
    if let Ok(sh) = std::env::var("SHELL") {
        let base = Path::new(&sh)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if let Ok(s) = base.parse::<Shell>() {
            return Some(s);
        }
    }
    if std::env::var("PSModulePath").is_ok() {
        return Some(Shell::Pwsh);
    }
    None
}

/// Resolve shell from optional `--shell` flag, falling back to [`detect_shell`].
///
/// Returns `None` when no shell could be determined (both flag absent and detection failed).
pub(crate) fn resolve_shell(shell_flag: Option<&str>) -> Result<Option<Shell>, Box<dyn std::error::Error>> {
    if let Some(s) = shell_flag {
        let sh = s.parse::<Shell>().map_err(|e: crate::domain::shell::ShellParseError| {
            Box::<dyn std::error::Error>::from(e.to_string())
        })?;
        return Ok(Some(sh));
    }
    Ok(detect_shell())
}
