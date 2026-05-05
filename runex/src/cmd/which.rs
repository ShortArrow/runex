//! `runex which <token>` — show the rule that would match the token.

use runex_core::expand;
use runex_core::model::Config;
use runex_core::shell::Shell;

use crate::format::{format_which_result, which_result_to_json};
use crate::{CmdOutcome, CmdResult, MAX_TOKEN_BYTES};

pub fn handle(
    token: String,
    config: &Config,
    shell: Shell,
    command_exists: &dyn Fn(&str) -> bool,
    json: bool,
    why: bool,
) -> CmdResult {
    if token.len() > MAX_TOKEN_BYTES {
        eprintln!(
            "error: token is too long ({} bytes); maximum is {MAX_TOKEN_BYTES}",
            token.len()
        );
        return Ok(CmdOutcome::ExitCode(1));
    }
    let result = expand::which_abbr(config, &token, shell, command_exists);
    if json {
        println!("{}", serde_json::to_string_pretty(&which_result_to_json(&result))?);
    } else {
        println!("{}", format_which_result(&result, why));
    }
    Ok(CmdOutcome::Ok)
}
