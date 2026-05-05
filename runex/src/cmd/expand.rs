//! `runex expand --token …` — emit the expansion (or pass-through).
//! `--dry-run` prints diagnostic info instead of the final cast.

use runex_core::expand;
use runex_core::model::{Config, ExpandResult};
use runex_core::shell::Shell;

use crate::format::{format_dry_run_result, which_result_to_json};
use crate::{CmdOutcome, CmdResult, MAX_TOKEN_BYTES};

pub fn handle(
    token: String,
    config: &Config,
    shell: Shell,
    command_exists: &dyn Fn(&str) -> bool,
    json: bool,
    dry_run: bool,
) -> CmdResult {
    if token.len() > MAX_TOKEN_BYTES {
        eprintln!(
            "error: --token is too long ({} bytes); maximum is {MAX_TOKEN_BYTES}",
            token.len()
        );
        return Ok(CmdOutcome::ExitCode(1));
    }
    if dry_run {
        let result = expand::which_abbr(config, &token, shell, command_exists);
        if json {
            println!("{}", serde_json::to_string_pretty(&which_result_to_json(&result))?);
        } else {
            print!("{}", format_dry_run_result(&token, &result));
        }
    } else {
        let result = expand::expand(config, &token, shell, command_exists);
        if json {
            let v = match &result {
                ExpandResult::Expanded { text: s, .. } => serde_json::json!({
                    "result": "expanded",
                    "token": token,
                    "expansion": s,
                }),
                ExpandResult::PassThrough(s) => serde_json::json!({
                    "result": "pass_through",
                    "token": s,
                }),
            };
            println!("{}", serde_json::to_string_pretty(&v)?);
        } else {
            match result {
                ExpandResult::Expanded { text, cursor_offset } => {
                    if let Some(offset) = cursor_offset {
                        // Output text + unit separator + cursor offset for shell templates
                        print!("{text}\x1f{offset}");
                    } else {
                        print!("{text}");
                    }
                }
                ExpandResult::PassThrough(s) => print!("{s}"),
            }
        }
    }
    Ok(CmdOutcome::Ok)
}
