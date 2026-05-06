//! `runex add <key> <expand>` and `runex remove <key>` — direct edits
//! to the config TOML, delegated entirely to runex-core.
//!
//! These are the two handlers the codex review flagged as inline-in-
//! dispatch. Pulling them out behind a named function makes the
//! dispatch arm a one-liner like every other arm and keeps the file
//! reading top-to-bottom.

use std::path::Path;

use crate::domain::sanitize::sanitize_for_display;

use crate::{CmdOutcome, CmdResult};

pub(crate) fn handle_add(
    config_path: &Path,
    key: &str,
    expand: &str,
    when_command_exists: Option<&[String]>,
) -> CmdResult {
    crate::app::config::append_abbr_to_file(config_path, key, expand, when_command_exists)?;
    println!(
        "Added: {} -> {}",
        sanitize_for_display(key),
        sanitize_for_display(expand)
    );
    Ok(CmdOutcome::Ok)
}

pub(crate) fn handle_remove(config_path: &Path, key: &str) -> CmdResult {
    let removed = crate::app::config::remove_abbr_from_file(config_path, key)?;
    if removed > 0 {
        println!(
            "Removed {} rule(s) for '{}'",
            removed,
            sanitize_for_display(key)
        );
    } else {
        println!("No rule found for '{}'", sanitize_for_display(key));
    }
    Ok(CmdOutcome::Ok)
}
