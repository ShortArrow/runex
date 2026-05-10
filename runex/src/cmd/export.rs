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

/// `runex export <shell>` handler.
///
/// As of Phase G (0.1.15), `--bin` is an `Option<String>`:
///
/// * `None` (= flag omitted) → bake `current_exe()` into the
///   generated hook so per-keystroke invocations don't pay PATH
///   resolution. This is the recommended default.
/// * `Some(s)` → use `s` verbatim. `--bin runex` keeps the legacy
///   bare-name behaviour for power users hand-managing dotfiles
///   that source the same exported script across multiple
///   machines with different installations.
///
/// See [`docs/decisions/0001-static-integration-cache.md`] for the
/// design rationale (why `current_exe()` over rcfile-baked
/// absolute paths or doctor-WARN-only).
pub(crate) fn handle(shell: String, bin: Option<String>, config_flag: Option<&Path>) -> CmdResult {
    // Phase G: --bin is Option<String> as of 0.1.15. None means
    // "use current_exe()", which bakes an absolute path into the
    // generated script and lets per-keystroke `runex hook`
    // invocations skip the PATH lookup. On WSL with a `mise` shim
    // ahead of ~/.cargo/bin/runex, that lookup costs ~470 ms per
    // invocation (mise startup overhead through the shim wrapper).
    //
    // Some(s) preserves the previous bare-name behaviour for power
    // users hand-managing dotfiles that need to source the same
    // exported script across multiple machines with different
    // installations. Most callers should leave --bin off.
    //
    // current_exe() returns the post-exec real binary even when the
    // process was launched via a `mise` shim (Linux: /proc/self/exe;
    // Windows: GetModuleFileNameW), so the baked path bypasses the
    // shim by construction.
    let effective_bin = bin.unwrap_or_else(|| crate::util::path::current_exe_or_default("runex"));
    if let Err(msg) = validate_bin(&effective_bin) {
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

    // Phase G: prepend the integration-cache header so the same byte
    // stream serves both `runex export <shell>` (stdout) and `runex
    // init <shell>` (cache file). Doctor (G6) parses the header to
    // detect drift / version mismatch / missing baked-bin path; having
    // the same format on both write paths keeps the parser simple.
    //
    // Header is omitted for clink because clink's lua install path
    // and freshness probe already use a different format (the lua
    // file is byte-compared against export output in
    // `infra::integration_check::check_clink_lua_freshness`).
    let header = if matches!(s, Shell::Clink) {
        String::new()
    } else {
        let cp = crate::infra::integration_cache::comment_prefix_for(s);
        crate::infra::integration_cache::cache_header(cp, &effective_bin)
    };
    let body = crate::app::shell_export::export_script(s, &effective_bin, config.as_ref());
    print!("{header}{body}");
    Ok(CmdOutcome::Ok)
}
