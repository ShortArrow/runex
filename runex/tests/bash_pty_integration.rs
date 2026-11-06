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
    let mut session = PtySession::spawn(PtyShell::Bash, runex_bin_str(), config.path())
        .expect("the shell is installed, so a PTY session that fails to bootstrap is a real failure, not a skip");

    // Type `gcm` then Space. The Space binding should fire
    // __runex_expand, which replaces `gcm` with `echo EXPANDED ` and
    // submits the line on the next Enter. We then read the command's
    // own output (`EXPANDED`) which proves the expansion ran end to
    // end.
    session.send("gcm ");
    session.send_line("");

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
/// The line ends in `'RX''DONE'`, which prints `RXDONE` only when the
/// line runs, so the checks read the executed output.
#[test]
fn space_does_not_expand_after_echo_argument_position() {
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let config = write_simple_config("gcm", "echo EXPANDED");
    let mut session = PtySession::spawn(PtyShell::Bash, runex_bin_str(), config.path())
        .expect("the shell is installed, so a PTY session that fails to bootstrap is a real failure, not a skip");

    session.send("echo gcm ");
    session.send_line("'RX''DONE'");

    session
        .expect_regex("RXDONE")
        .expect("the line ran, so its two quoted halves printed joined");
    assert!(
        !session.saw("EXPANDED"),
        "`gcm` after `echo ` is an argument, so the Space must not expand it"
    );
    assert!(
        session.saw("gcm RXDONE"),
        "`echo` should have received `gcm` as typed"
    );
}

/// Issue #50: a line that starts with `-` reaches `runex hook` as the
/// value of `--line`, not as a flag, so a rule whose key is `-n` expands.
#[test]
fn space_expands_a_line_that_starts_with_a_dash() {
    if !bash4_available() {
        eprintln!("skipping: bash not available");
        return;
    }
    let config = write_simple_config("-n", "echo EXPANDED");
    let mut session = PtySession::spawn(PtyShell::Bash, runex_bin_str(), config.path())
        .expect("the shell is installed, so a PTY session that fails to bootstrap is a real failure, not a skip");

    session.send("-n ");
    session.send_line("");

    session
        .expect_regex(r"EXPANDED")
        .expect("bash should have expanded -n<Space> to `echo EXPANDED`");
}
