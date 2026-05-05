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

pub fn handle(
    shell_str: &str,
    line: &str,
    cursor: usize,
    paste_pending: bool,
    config_override: Option<&Path>,
    path_prepend: Option<&Path>,
) -> CmdResult {
    let shell = Shell::from_str(shell_str).map_err(|e| format!("{}", e))?;

    // Per-keystroke cost guard. An oversize --line short-circuits to
    // a literal-space InsertSpace before any expansion logic runs.
    if line.len() > MAX_HOOK_LINE_BYTES {
        let cursor_safe = cursor.min(line.len());
        let mut s = String::with_capacity(line.len() + 1);
        s.push_str(&line[..cursor_safe]);
        s.push(' ');
        s.push_str(&line[cursor_safe..]);
        let action = crate::domain::hook::HookAction::InsertSpace {
            line: s,
            cursor: cursor_safe + 1,
        };
        println!("{}", crate::domain::hook::render_action(shell, &action));
        return Ok(CmdOutcome::Ok);
    }

    // If the user pasted a block, the pwsh wrapper sets this flag so we skip
    // expansion entirely and behave like a normal space keypress.
    if paste_pending {
        let action = crate::domain::hook::HookAction::InsertSpace {
            line: {
                let mut s = String::with_capacity(line.len() + 1);
                let cursor = cursor.min(line.len());
                s.push_str(&line[..cursor]);
                s.push(' ');
                s.push_str(&line[cursor..]);
                s
            },
            cursor: cursor.min(line.len()) + 1,
        };
        println!("{}", crate::domain::hook::render_action(shell, &action));
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
        let cursor_safe = cursor.min(line.len());
        let mut s = String::with_capacity(line.len() + 1);
        s.push_str(&line[..cursor_safe]);
        s.push(' ');
        s.push_str(&line[cursor_safe..]);
        let action = crate::domain::hook::HookAction::InsertSpace { line: s, cursor: cursor_safe + 1 };
        println!("{}", crate::domain::hook::render_action(shell, &action));
        return Ok(CmdOutcome::Ok);
    };

    let action = crate::domain::hook::hook(&config, shell, line, cursor, ctx.command_exists);
    println!("{}", crate::domain::hook::render_action(shell, &action));
    Ok(CmdOutcome::Ok)
}
