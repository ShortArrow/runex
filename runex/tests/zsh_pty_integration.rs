//! End-to-end keystroke test for the zsh integration.
//!
//! Mirrors `bash_pty_integration.rs`: spawns an interactive zsh in
//! a PTY through the shared [`support::pty`] harness, sources the
//! runex integration script, then sends a token + Space and asserts
//! the expanded command actually runs.
//!
//! zsh-only differences from the bash counterpart:
//! * `zsh -f` (no rcfiles) instead of `--norc --noprofile`.
//! * The harness installs `PROMPT='__RUNEX_PROMPT__> '` instead of
//!   bash's `PS1`. zsh would otherwise emit a multi-line default
//!   prompt that the regex would have to dance around.
//!
//! Skip-on-missing: every test bails silently when `zsh` isn't on
//! `$PATH` so Linux runners without it (and Windows generally)
//! don't fail.

#![cfg(target_family = "unix")]

mod support;

use support::pty::{PtySession, PtyShell};
use support::subprocess::{runex_bin_str, shell_available, write_simple_config};

#[test]
fn space_triggers_expand_for_known_token() {
    if !shell_available("zsh") {
        eprintln!("skipping: zsh not available");
        return;
    }
    let config = write_simple_config("gcm", "echo EXPANDED");
    let Some(mut session) = PtySession::spawn(PtyShell::Zsh, runex_bin_str(), config.path())
    else {
        eprintln!("skipping: could not spawn zsh session");
        return;
    };

    // Same shape as the bash test: type the abbr, then Space (the
    // configured trigger). The zle widget bound by the integration
    // script should rewrite the buffer to `echo EXPANDED ` and then
    // Enter submits the line.
    session.send("gcm ");
    session.send_line("");

    session
        .expect_regex(r"EXPANDED")
        .expect("zsh should have echoed EXPANDED after the gcm<Space> expansion");
}
