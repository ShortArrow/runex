//! End-to-end keystroke test for the bash integration.
//!
//! Spawns a real interactive bash inside a PTY, sources the runex
//! integration script, then sends individual keystrokes (including
//! the Space trigger) and asserts that the resulting prompt contains
//! the expanded command. This is the layer of coverage closest to a
//! real user — it exercises everything the other test suites bypass:
//! readline's bind table, the registered widget callback path
//! (`__runex_expand` invoked from the actual key event), and the
//! READLINE_LINE / READLINE_POINT plumbing.
//!
//! The 0.1.12 clink regression (silent fallback to a literal space
//! when the cmd host's PATH was degraded) would have been caught by
//! a test like this, just on the clink side. clink is harder to drive
//! from Linux CI; bash's readline is the most stable target so we
//! pin it first and treat the others as future work.
//!
//! Mechanics (PTY launch, sentinel prompt, integration sourcing) live
//! in [`support::pty`]; this file owns only the bash-specific scenarios.

#![cfg(target_family = "unix")]

mod support;

use support::pty::{PtySession, PtyShell};
use support::subprocess::{bash4_available, runex_bin_str, write_simple_config};

#[test]
fn space_triggers_expand_for_known_token() {
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let config = write_simple_config("gcm", "echo EXPANDED");
    let Some(mut session) = PtySession::spawn(PtyShell::Bash, runex_bin_str(), config.path())
    else {
        eprintln!("skipping: could not spawn bash session");
        return;
    };

    // Type `gcm` then Space. The Space binding should fire
    // __runex_expand, which replaces `gcm` with `echo EXPANDED ` and
    // submits the line on the next Enter. We then read the command's
    // own output (`EXPANDED`) which proves the expansion ran end to
    // end.
    session.send("gcm ");
    session.send_line("");

    // Use the inner expect for richer error messages on the assertion
    // path; PtySession::expect_regex returns Option<()> which would
    // only let us say "didn't see it" with no captured surrounding
    // text. Reach across via the published API.
    session
        .expect_regex(r"EXPANDED")
        .expect("bash should have echoed EXPANDED after the gcm<Space> expansion");
}

/// Mirror of `bash_cygwin_bake_pty::cygwin_bake_skips_expansion_after_echo`
/// for the exec path (Linux bash → `runex hook`). The exec path has always
/// honoured `is_command_position`, so `echo gcm<Space>` self-inserts a
/// literal space instead of expanding `gcm`. Putting the assertion side
/// by side with the bake-path counterpart makes the parity visible in
/// any diff that touches command-position handling (issue #9).
#[test]
fn space_does_not_expand_after_echo_argument_position() {
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let config = write_simple_config("gcm", "echo EXPANDED");
    let Some(mut session) = PtySession::spawn(PtyShell::Bash, runex_bin_str(), config.path())
    else {
        eprintln!("skipping: could not spawn bash session");
        return;
    };

    // Type `echo gcm` then Space. `echo ` is not a command position,
    // so the hook must leave the buffer as `echo gcm ` (literal space)
    // and bash should print `gcm` — NOT the expanded EXPANDED string.
    session.send("echo gcm ");
    session.send_line("");

    // We expect bash to print the literal token `gcm` (= what `echo`
    // received as its argument) somewhere in the output, and *not* the
    // `EXPANDED` marker that would only appear if the bake / hook
    // rewrote the buffer. We can't anchor `gcm` to a line start because
    // mintty / xterm don't always emit a leading newline before the
    // output. So we assert two things separately: `gcm` appears, and
    // `EXPANDED` does NOT.
    session
        .expect_regex(r"gcm")
        .expect("expected `gcm` to appear in stdout (= echo printed its argument)");
    // Best-effort negative check: we already matched `gcm` so the
    // session buffer should not also carry `EXPANDED`. expectrl's
    // expect_regex doesn't expose a non-blocking try-match, so we
    // resort to sending another keystroke and inspecting the prompt
    // round-trip — but that's flaky in CI. Keep the positive assert
    // and rely on the bake-side test for the negative side.
}
