//! `runex doctor` — environment health check.
//!
//! Surfaces config errors rather than aborting on them, so this is
//! the canonical caller of `AppContext::build_optional`. The
//! `build_doctor_env_info` helper composes the doctor-only env info
//! struct (Windows effective-search-path breakdown, the canonical
//! clink-export-for-drift-check, and the per-shell rcfile marker
//! selection); it lives here because every field is doctor-specific
//! and `util/` would be the wrong home for it.

use std::path::Path;

use crate::app::doctor;
use crate::domain::model::Config;
use crate::domain::shell::Shell;

use crate::format::format_check_line;
use crate::shell_alias::add_shell_alias_conflicts;
use crate::{AppContext, CmdOutcome, CmdResult, OptionalContext, Spinner};

/// Compose the [`doctor::DoctorEnvInfo`] that the `doctor` subcommand
/// passes alongside the config checks. Today this only sets the
/// Windows effective-search-path breakdown; on other platforms only
/// the integration-check fields apply.
pub fn build_doctor_env_info(config: Option<&Config>) -> doctor::DoctorEnvInfo {
    let mut info = doctor::DoctorEnvInfo::default();

    #[cfg(windows)]
    {
        let p = crate::win_path::effective_search_path();
        info.effective_search_path = Some(doctor::EffectiveSearchPathSummary {
            from_process: p.from_process,
            from_user_registry: p.from_user_registry,
            from_system_registry: p.from_system_registry,
        });
    }

    // Render the canonical clink export so doctor can detect drift on
    // disk. Use the absolute path of our own executable as the bin
    // (matching `handle_export`'s clink full-path fallback) so a fresh
    // `runex doctor` after upgrade matches what `runex init clink`
    // would write today.
    let clink_bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "runex".to_string());
    info.clink_export_for_drift_check = Some(crate::app::shell_export::export_script(
        Shell::Clink,
        &clink_bin,
        config,
    ));

    // We always want to know whether the user ran `runex init <shell>`
    // for each rcfile-bearing shell. doctor itself decides whether to
    // emit each row based on rcfile existence (a missing rcfile means
    // "user doesn't use that shell" and the check is skipped silently).
    info.check_rcfile_markers = doctor::RcfileMarkerSelection::all();

    info
}

pub fn handle(
    config_flag: Option<&Path>,
    path_prepend: Option<&Path>,
    no_shell_aliases: bool,
    verbose: bool,
    strict: bool,
    json: bool,
) -> CmdResult {
    // Doctor surfaces config errors rather than aborting on them, so
    // we use the graceful builder. precache_enabled = false because
    // doctor must always check live to surface stale-cache issues.
    let ctx = AppContext::build_optional(config_flag, None, path_prepend, false);
    let OptionalContext {
        config_path,
        config,
        parse_error,
        command_exists,
        ..
    } = ctx;
    let spinner = Spinner::start("Checking environment...");
    // Build informational env-info that doctor renders alongside the
    // config checks: Windows effective_search_path breakdown (see
    // `runex/src/win_path.rs`), per-shell rcfile marker checks, and a
    // clink-lua drift check. The current config is forwarded so the
    // generated clink export reflects the user's keybinds & abbrs.
    let env_info = build_doctor_env_info(config.as_ref());
    let mut result = doctor::diagnose(
        &config_path,
        config.as_ref(),
        parse_error.as_deref(),
        &env_info,
        &command_exists,
    );
    if !no_shell_aliases {
        add_shell_alias_conflicts(&mut result, config.as_ref());
    }
    // Read config source once (O_NOFOLLOW, size-capped) and share across checks.
    let source = crate::app::config::read_config_source(&config_path).ok();

    // Always: report every rule rejected by per-field validation so users know
    // *all* the invalid fields, not just the first one that tripped parse_config.
    if let Some(src) = source.as_deref() {
        result.checks.extend(doctor::check_rejected_rules(src));
    }

    if strict {
        if let Some(src) = source.as_deref() {
            result.checks.extend(doctor::check_unknown_fields(src));
            result.checks.extend(doctor::check_precache_deprecation(src));
        }
        // Check for unreachable duplicate rules
        if let Some(cfg) = config.as_ref() {
            result.checks.extend(doctor::check_unreachable_duplicates(cfg));
        }
    }
    spinner.stop();

    if json {
        println!("{}", serde_json::to_string_pretty(&result.checks)?);
    } else {
        for check in &result.checks {
            println!("{}", format_check_line(check, verbose));
        }
    }

    if !result.is_healthy() {
        return Ok(CmdOutcome::ExitCode(1));
    }
    Ok(CmdOutcome::Ok)
}
