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
use support::subprocess::{runex_bin_str, shell_available, write_config_file, write_simple_config};

#[test]
fn space_triggers_expand_for_known_token() {
    if !shell_available("pwsh") {
        eprintln!("skipping: pwsh not available");
        return;
    }
    // The expansion builds EXPANDED from two halves at run time, so
    // the word can never appear in the buffer PSReadLine renders —
    // only the executed command can print it. That makes "EXPANDED
    // was seen" a proof of execution rather than of a redraw.
    let config = write_simple_config("gcm", "Write-Host ('EXPAN' + 'DED')");
    let mut session = PtySession::spawn(PtyShell::Pwsh, runex_bin_str(), config.path())
        .expect("the shell is installed, so a PTY session that fails to bootstrap is a real failure, not a skip");

    // Type the token, let PSReadLine render it, THEN press Space as a
    // separate keystroke so the trigger handler fires on its own key
    // event (sending "gcm " in one write makes PSReadLine treat the
    // space as an ordinary self-insert). The handler replaces the
    // buffer with the expansion; Enter then submits it.
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
    // The rewritten buffer renders `Write-Host (...)` with syntax
    // coloring between tokens, so match the single token `Write-Host`
    // — it appears only because the abbreviation expanded (bootstrap
    // output was cleared before this point).
    session
        .expect_regex("Write-Host")
        .expect("pwsh Space should replace gcm with the expansion in the buffer");
    session.enter().expect("submit the line");
    session
        .expect_regex("EXPANDED")
        .expect("pwsh should print EXPANDED after submitting the expanded line");
}

/// Negative control for the harness itself. With no abbreviation
/// configured, Space must insert a plain space and `gcm` must run as
/// what it is in PowerShell — the built-in alias of `Get-Command` —
/// whose listing proves the session executed the line. A harness that
/// silently stopped driving the shell would show neither the listing
/// nor an expansion, so this test fails loudly in that case instead of
/// passing vacuously.
#[test]
fn space_without_matching_abbr_inserts_a_plain_space() {
    if !shell_available("pwsh") {
        eprintln!("skipping: pwsh not available");
        return;
    }
    let config = write_config_file("version = 1\n");
    let mut session = PtySession::spawn(PtyShell::Pwsh, runex_bin_str(), config.path())
        .expect("the shell is installed, so a PTY session that fails to bootstrap is a real failure, not a skip");

    session.send("g").expect("send g");
    session.expect_regex("g").expect("echo g");
    session.send("c").expect("send c");
    session.expect_regex("gc").expect("echo gc");
    session.send("m").expect("send m");
    session.expect_regex("gcm").expect("echo gcm");
    session.send(" ").expect("send space");
    session.enter().expect("submit the line");
    session
        .expect_regex("Microsoft.PowerShell.Utility")
        .expect("`gcm` (Get-Command) should have listed cmdlets, proving the line ran");
    assert!(
        !session.saw("EXPANDED"),
        "no abbreviation is configured, so nothing may have expanded"
    );
}
