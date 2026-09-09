//! End-to-end keystroke test for the pwsh integration.
//!
//! Mirrors `bash_pty_integration.rs` for PowerShell. Uses the
//! shared [`support::pty`] harness, which knows about the
//! `function prompt { '...' }` syntax pwsh wants and how to
//! source the integration via `Invoke-Expression (& runex
//! export pwsh | Out-String)`.
//!
//! Why Linux-only: expectrl 0.7's Windows ConPTY backend is
//! flagged unstable in our `Cargo.toml`. The pwsh that this test
//! drives is the same `pwsh` binary that ships on Linux (Microsoft
//! distributes it for every major distro), so we get end-to-end
//! coverage of the integration on the platform expectrl handles
//! reliably and let the existing `pwsh_integration.rs` subprocess
//! test cover the Windows side.
//!
//! Skip-on-missing: every test bails silently when `pwsh` isn't on
//! `$PATH`.

#![cfg(target_family = "unix")]

mod support;

use support::pty::{PtySession, PtyShell};
use support::subprocess::{runex_bin_str, shell_available, write_simple_config};

#[test]
fn space_triggers_expand_for_known_token() {
    if !shell_available("pwsh") {
        eprintln!("skipping: pwsh not available");
        return;
    }
    let config = write_simple_config("gcm", "echo EXPANDED");
    let Some(mut session) = PtySession::spawn(PtyShell::Pwsh, runex_bin_str(), config.path())
    else {
        eprintln!("skipping: could not spawn pwsh session");
        return;
    };

    // Type the token, let PSReadLine render it, THEN press Space as a
    // separate keystroke so the trigger handler fires on its own key
    // event (sending "gcm " in one write makes PSReadLine treat the
    // space as an ordinary self-insert). The handler replaces the
    // buffer with `echo EXPANDED `; Enter then submits it.
    // Type one char at a time and wait for each to render, so the
    // buffer is settled at `gcm` before Space arrives — otherwise the
    // trigger key can reach PSReadLine while it is still processing the
    // token and be handled as a plain self-insert.
    session.send("g").expect("send g");
    session.expect_regex("g").expect("echo g");
    session.send("c").expect("send c");
    session.expect_regex("gc").expect("echo gc");
    session.send("m").expect("send m");
    session.expect_regex("gcm").expect("echo gcm");
    session.send(" ").expect("send space");
    // The rewritten buffer renders as `echo EXPANDED` with syntax
    // coloring between the two words, so match the single word
    // `EXPANDED` — it appears only because the abbreviation expanded
    // (bootstrap output was cleared before this point).
    session
        .expect_regex("EXPANDED")
        .expect("pwsh Space should expand gcm so EXPANDED appears in the buffer");
    session.enter().expect("submit the line");
    // After submission the command runs and prints EXPANDED on its own
    // line. Wait for a SECOND occurrence (the first was the buffer
    // render above) to prove the expanded command actually executed.
    session
        .expect_regex_nth("EXPANDED", 2)
        .expect("pwsh should have echoed EXPANDED after submitting the expanded line");
}
