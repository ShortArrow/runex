//! `runex list` — print every abbreviation in the loaded config.

use crate::app::expand;
use crate::domain::model::Config;
use crate::domain::sanitize::sanitize_for_display;
use crate::domain::shell::Shell;

use crate::{CmdOutcome, CmdResult};

pub(crate) fn handle(
    config: &Config,
    shell: Option<Shell>,
    json: bool,
    filter: Option<&str>,
) -> CmdResult {
    if json {
        let filtered: Vec<&_> = config
            .abbr
            .iter()
            .filter(|a| filter.is_none_or(|f| a.key == f))
            .collect();
        println!("{}", serde_json::to_string_pretty(&filtered)?);
    } else {
        for (key, exp) in expand::list_pairs(config, shell, filter) {
            println!("{}\t{}", sanitize_for_display(key), sanitize_for_display(&exp));
        }
    }
    Ok(CmdOutcome::Ok)
}
