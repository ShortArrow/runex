//! Use-case wrapper for the `hook` subcommand — the per-keystroke
//! dispatch that decides "insert a literal space", "expand the token
//! to the left of the cursor", or "no-op".
//!
//! Phase D D4a routes `cmd::hook` through this module so the cmd
//! layer doesn't import `crate::domain::hook` directly. See
//! `app::expand` for the rationale.
//!
//! The `HookAction` enum and the rendering function are re-exported
//! at `pub(crate)` so cmd code constructs the InsertSpace short-
//! circuit (oversized line, paste-pending, etc.) using a stable
//! re-export rather than a deep `crate::domain::hook::HookAction`
//! path.

use crate::domain::model::Config;
use crate::domain::shell::Shell;

pub(crate) use crate::domain::hook::HookAction;

/// Run the per-keystroke hook decision. The closure dispatches to
/// `domain::hook::hook` unchanged — the wrapper exists for layering,
/// not for behaviour.
pub(crate) fn run<F>(
    config: &Config,
    shell: Shell,
    line: &str,
    cursor: usize,
    command_exists: F,
) -> HookAction
where
    F: Fn(&str) -> bool,
{
    crate::domain::hook::hook(config, shell, line, cursor, command_exists)
}

/// Render the chosen [`HookAction`] as the shell-specific eval text
/// the wrapper script will pipe into the live shell.
pub(crate) fn render(shell: Shell, action: &HookAction) -> String {
    crate::domain::hook::render_action(shell, action)
}
