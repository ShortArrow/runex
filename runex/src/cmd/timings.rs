//! `runex timings` — measure each phase of the expand path so users
//! can spot config-load / shell-resolve / per-key bottlenecks.
//!
//! Doesn't go through `AppContext` because the timer needs to bracket
//! each step of the runtime assembly individually, which is the whole
//! point of the command.

use std::path::Path;

use crate::app::expand;
use crate::domain::shell::Shell;

use crate::format;
use crate::util::path::make_command_exists;
use crate::util::shell::resolve_shell;
use crate::{compute_precache_fingerprint, resolve_config, CmdOutcome, CmdResult, MAX_TOKEN_BYTES};

pub fn handle(
    key: Option<String>,
    shell_str: Option<String>,
    config_flag: Option<&Path>,
    path_prepend: Option<&Path>,
    json: bool,
) -> CmdResult {
    use crate::domain::timings::{PhaseTimer, Timings};

    let mut timings = Timings::new();

    let t = PhaseTimer::start();
    let (config_path, config) = resolve_config(config_flag)?;
    timings.record_phase("config_load", t.elapsed());

    let t = PhaseTimer::start();
    let shell = resolve_shell(shell_str.as_deref())?.unwrap_or(Shell::Bash);
    timings.record_phase("shell_resolve", t.elapsed());

    let fp = compute_precache_fingerprint(&config_path, &format!("{shell:?}").to_lowercase());
    let command_exists = make_command_exists(path_prepend, Some(&fp));

    match key {
        Some(k) => {
            if k.len() > MAX_TOKEN_BYTES {
                eprintln!(
                    "error: key is too long ({} bytes); maximum is {MAX_TOKEN_BYTES}",
                    k.len()
                );
                return Ok(CmdOutcome::ExitCode(1));
            }
            expand::expand_timed(&config, &k, shell, &command_exists, &mut timings);
        }
        None => {
            // Time each unique abbr key
            let keys: Vec<String> = config.abbr.iter().map(|a| a.key.clone()).collect();
            let unique_keys: Vec<String> = {
                let mut seen = std::collections::HashSet::new();
                keys.into_iter().filter(|k| seen.insert(k.clone())).collect()
            };
            for key in &unique_keys {
                expand::expand_timed(&config, key, shell, &command_exists, &mut timings);
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&format::format_timings_json(&timings))?);
    } else {
        print!("{}", format::format_timings_table(&timings));
    }
    Ok(CmdOutcome::Ok)
}
