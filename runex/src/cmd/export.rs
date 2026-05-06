//! `runex export <shell> [--bin NAME]` — emit the integration script
//! the shell wrapper sources at startup.
//!
//! `validate_bin` lives here (not in `util/`) because its policy is
//! export-specific: only `--bin` ever flows through it, and the
//! "printable ASCII only" guard exists to stop Unicode homoglyphs
//! from being baked into a generated shell script.

use std::path::Path;

use crate::domain::shell::Shell;

use crate::{resolve_config, resolve_config_opt, CmdOutcome, CmdResult, MAX_BIN_LEN};

/// Validate the `--bin` argument for `export`.
///
/// Rejects values that are empty, whitespace-only, too long, contain
/// control characters, or contain non-printable-ASCII characters.
/// Only printable ASCII is allowed to prevent Unicode homoglyphs and
/// bidirectional overrides from being silently embedded in generated
/// shell scripts.
///
/// Returns the error message to surface to the user on validation
/// failure; `Ok(())` when the value passes. Caller is responsible
/// for `eprintln!`ing the message and returning
/// `CmdOutcome::ExitCode(1)` — keeping `validate_bin` itself I/O-free
/// makes it a pure function the unit tests can drive directly.
pub(crate) fn validate_bin(bin: &str) -> Result<(), String> {
    if bin.trim().is_empty() {
        return Err("--bin must not be empty or whitespace-only".into());
    }
    if bin.len() > MAX_BIN_LEN {
        return Err(format!(
            "--bin is too long ({} bytes); maximum is {MAX_BIN_LEN}",
            bin.len()
        ));
    }
    if bin.chars().any(|c| c.is_ascii_control() || c == '\u{0085}' || c == '\u{2028}' || c == '\u{2029}') {
        return Err("--bin contains an invalid control character".into());
    }
    if bin.chars().any(|c| !c.is_ascii() || !c.is_ascii_graphic()) {
        return Err("--bin must contain only printable ASCII characters".into());
    }
    Ok(())
}

pub(crate) fn handle(shell: String, bin: String, config_flag: Option<&Path>) -> CmdResult {
    if let Err(msg) = validate_bin(&bin) {
        eprintln!("error: {msg}");
        return Ok(CmdOutcome::ExitCode(1));
    }
    let s: Shell = shell.parse().map_err(|e: crate::domain::shell::ShellParseError| {
        Box::<dyn std::error::Error>::from(e.to_string())
    })?;
    let config = if config_flag.is_some() {
        let (_path, cfg) = resolve_config(config_flag)?;
        Some(cfg)
    } else {
        let (_path, cfg, _err) = resolve_config_opt(None);
        cfg
    };
    // For clink, default-bin must resolve to an absolute path.
    //
    // Why: clink invokes runex via Lua's `io.popen` which spawns a fresh
    // cmd.exe child. That child inherits whatever PATH the clink-injected
    // host process happens to have, and on real machines that PATH is
    // sometimes degraded (e.g. system-only, with the User scope from
    // HKCU not yet merged in). A bare `runex` command in the lua script
    // would then fail to resolve. Embedding the absolute path of the
    // currently-running executable (which is by definition reachable —
    // we ourselves were just launched from it) sidesteps the entire
    // PATH-inheritance question for clink.
    //
    // Other shells (bash/zsh/pwsh/nu) rely on PATH-resolved bare names
    // because they're invoked from rcfiles where PATH is already correct,
    // and because users can plausibly want to override which `runex`
    // gets used. Only clink gets the absolute-path treatment.
    let effective_bin = if matches!(s, Shell::Clink) && bin == "runex" {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or(bin)
    } else {
        bin
    };
    print!("{}", crate::app::shell_export::export_script(s, &effective_bin, config.as_ref()));
    Ok(CmdOutcome::Ok)
}
