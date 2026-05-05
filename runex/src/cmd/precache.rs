//! `runex precache` — emit a shell `export` statement seeding the
//! `RUNEX_CMD_CACHE_V1` env var so subsequent `hook` invocations can
//! short-circuit `which::which` lookups.
//!
//! Three modes:
//! 1. `--list-commands` prints the bare comma-separated list of
//!    commands to probe (so the shell template can drive its own
//!    PATH-only / which-based precache).
//! 2. `--resolved <…>` consumes externally-computed results and
//!    converts them to the cache JSON.
//! 3. Default: probes via `make_command_exists(.., None)` (precache
//!    intentionally bypasses its own hint to avoid feedback loops).
//!
//! The handler doesn't go through `AppContext` because it needs to
//! make different early-return decisions *before* the fingerprint
//! is computed (mode 1 returns without ever computing it).

use std::path::Path;

use crate::domain::shell::Shell;

use crate::util::path::make_command_exists;
use crate::{compute_precache_fingerprint, resolve_config, CmdOutcome, CmdResult};

pub fn handle(
    shell: String,
    list_commands: bool,
    resolved: Option<String>,
    config_flag: Option<&Path>,
    path_prepend: Option<&Path>,
) -> CmdResult {
    use crate::app::precache;

    let s: Shell = shell.parse().map_err(|e: crate::domain::shell::ShellParseError| {
        Box::<dyn std::error::Error>::from(e.to_string())
    })?;
    let shell_name = format!("{s:?}").to_lowercase();

    let (config_path, config) = resolve_config(config_flag)?;

    // Mode 1: print comma-separated list of commands to check externally.
    // When path_only is true, output nothing so the shell template falls
    // back to which-based precache.
    if list_commands {
        if !config.precache.path_only {
            let cmds = precache::collect_unique_commands(&config);
            print!("{}", cmds.join(","));
        }
        return Ok(CmdOutcome::Ok);
    }

    let fp = compute_precache_fingerprint(&config_path, &shell_name);

    // Mode 2: use externally resolved results instead of which::which()
    if let Some(resolved_str) = resolved {
        let cache = precache::build_cache_from_resolved(&config, &fp, &resolved_str);
        let json = precache::cache_to_json(&cache);
        println!("{}", precache::export_statement(&shell_name, &json));
        return Ok(CmdOutcome::Ok);
    }

    // Default: use which::which() for command existence checks
    let command_exists = make_command_exists(path_prepend, None);
    let cache = precache::build_cache(&config, &fp, &command_exists);
    let json = precache::cache_to_json(&cache);

    println!("{}", precache::export_statement(&shell_name, &json));
    Ok(CmdOutcome::Ok)
}
