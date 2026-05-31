//! `runex hook` — per-keystroke decision: insert space, expand, or
//! short-circuit on oversize input.
//!
//! Writes the shell-specific eval text to stdout on success. On
//! config-load failure we silently emit a plain InsertSpace (so the
//! shell wrapper inserts a literal space on the user's behalf) —
//! runex never breaks the user's terminal even with a malformed
//! config they haven't fixed yet.

use std::path::Path;
use std::str::FromStr;

use crate::domain::shell::Shell;

use crate::{AppContext, CmdOutcome, CmdResult, MAX_HOOK_LINE_BYTES};

pub(crate) fn handle(
    shell_str: &str,
    line: &str,
    cursor: usize,
    paste_pending: bool,
    config_override: Option<&Path>,
    path_prepend: Option<&Path>,
) -> CmdResult {
    let shell = Shell::from_str(shell_str).map_err(|e| format!("{}", e))?;

    // Shells pass `--cursor` in their own unit: pwsh uses UTF-16 code
    // units (.NET / PSReadLine native), every other shell uses a char
    // (Unicode scalar value) count. The domain layer expects a byte
    // offset. Convert once here at the cmd/app boundary so every
    // downstream path sees a consistent byte cursor (issue #6).
    let cursor = crate::app::hook::shell_cursor_to_byte(shell, line, cursor);

    // Per-keystroke cost guard. An oversize --line short-circuits to
    // a literal-space InsertSpace before any expansion logic runs.
    if line.len() > MAX_HOOK_LINE_BYTES {
        let action = crate::app::hook::insert_space_action(line, cursor);
        println!("{}", crate::app::hook::render(shell, &action));
        return Ok(CmdOutcome::Ok);
    }

    // If the user pasted a block, the pwsh wrapper sets this flag so we skip
    // expansion entirely and behave like a normal space keypress.
    if paste_pending {
        let action = crate::app::hook::insert_space_action(line, cursor);
        println!("{}", crate::app::hook::render(shell, &action));
        return Ok(CmdOutcome::Ok);
    }

    // Config load failures are treated as "no expansion" — we still return the
    // InsertSpace action so the wrapper inserts a literal space on behalf of
    // the user. Pass the shell explicitly via shell_flag so the fingerprint
    // matches the shell the keystroke is for, not whatever `resolve_shell`
    // would default to.
    let ctx = AppContext::build_optional(config_override, Some(shell_str), path_prepend, true);
    let Some(config) = ctx.config else {
        // No valid config: emit a plain InsertSpace and return. This avoids
        // making every keypress a no-op (which would swallow the trigger key)
        // when a user has a malformed config they haven't fixed yet.
        let action = crate::app::hook::insert_space_action(line, cursor);
        println!("{}", crate::app::hook::render(shell, &action));
        return Ok(CmdOutcome::Ok);
    };

    let action = crate::app::hook::run(&config, shell, line, cursor, ctx.command_exists);
    println!("{}", crate::app::hook::render(shell, &action));
    Ok(CmdOutcome::Ok)
}
