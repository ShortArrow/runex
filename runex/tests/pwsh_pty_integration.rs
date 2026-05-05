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

    // Same shape as the bash / zsh tests. PSReadLine binds the
    // trigger key inside the integration script's
    // `Set-PSReadLineKeyHandler -Chord Spacebar` call; pressing
    // Space should fire the runex hook handler and rewrite the
    // buffer.
    session.send("gcm ");
    session.send_line("");

    session
        .expect_regex(r"EXPANDED")
        .expect("pwsh should have echoed EXPANDED after the gcm<Space> expansion");
}
