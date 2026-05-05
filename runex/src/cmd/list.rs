//! `runex list` — print every abbreviation in the loaded config.

use runex_core::expand;
use runex_core::model::Config;
use runex_core::sanitize::sanitize_for_display;
use runex_core::shell::Shell;

use crate::{CmdOutcome, CmdResult};

pub fn handle(config: &Config, shell: Option<Shell>, json: bool) -> CmdResult {
    if json {
        println!("{}", serde_json::to_string_pretty(&config.abbr)?);
    } else {
        for (key, exp) in expand::list(config, shell) {
            println!("{}\t{}", sanitize_for_display(key), sanitize_for_display(&exp));
        }
    }
    Ok(CmdOutcome::Ok)
}
