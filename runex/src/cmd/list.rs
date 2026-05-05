//! `runex list` — print every abbreviation in the loaded config.

use crate::domain::expand;
use crate::domain::model::Config;
use crate::domain::sanitize::sanitize_for_display;
use crate::domain::shell::Shell;

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
