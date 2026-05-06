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
