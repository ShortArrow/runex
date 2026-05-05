//! End-to-end keystroke test for the nu integration.
//!
//! Spawns a real interactive nu inside a PTY, sources the runex
//! integration script (generated out-of-band into a tempfile because
//! nu's `source` resolves paths at parse time), then sends individual
//! keystrokes including the Space trigger and asserts that the
//! resulting line carries the expansion. This is the closest layer to
//! a user typing in nu — it exercises the reedline keymap binding
//! that runs `executehostcommand` and rewrites `commandline`.
//!
//! Mechanics (PTY launch, sentinel prompt, integration sourcing) live
//! in [`support::pty`]; this file owns only the nu-specific
//! scenarios.

#![cfg(target_family = "unix")]

mod support;

use support::pty::{PtySession, PtyShell};
use support::subprocess::{runex_bin_str, shell_available, write_simple_config};

#[test]
fn space_triggers_expand_for_known_token() {
    if !shell_available("nu") {
        eprintln!("skipping: nu not available");
        return;
    }
    let config = write_simple_config("gcm", "echo EXPANDED");
    let Some(mut session) = PtySession::spawn(PtyShell::Nu, runex_bin_str(), config.path()) else {
        eprintln!("skipping: could not spawn nu session");
        return;
    };

    // Type `gcm` then Space. The keymap binding should fire
    // executehostcommand which calls runex hook, rewrites commandline
    // to `echo EXPANDED `, and the next Enter submits the line. We
    // assert on the visible echo (`EXPANDED`) which proves the
    // expansion ran end to end inside reedline.
    session.send("gcm ");
    session.send_line("");

    session
        .expect_regex(r"EXPANDED")
        .expect("nu should have echoed EXPANDED after the gcm<Space> expansion");
}
