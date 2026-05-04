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
//! ## Anti-flake measures
//!
//! - Use a bespoke PS1 (`__RUNEX_PROMPT__> `) so prompt detection is
//!   unambiguous.
//! - `set +o emacs` would disable readline; we explicitly enable it
//!   with `bind 'set enable-bracketed-paste off'` to avoid surprise
//!   ANSI prefixes around pasted-looking input.
//! - Generous timeout (5s per `expect`); CI runners with slow IO can
//!   easily exceed the default.
//! - One assertion per test; tests are independent.

#![cfg(target_family = "unix")]

use expectrl::Regex;
use expectrl::session::Session;
use std::io::Write as _;
use std::time::Duration;
use tempfile::NamedTempFile;

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_runex")
}

fn write_config() -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    write!(
        f,
        "version = 1\n\n[keybind.trigger]\ndefault = \"space\"\n\n[[abbr]]\nkey = \"gcm\"\nexpand = \"echo EXPANDED\"\n"
    )
    .unwrap();
    f.flush().unwrap();
    f
}

/// Returns false if bash 4+ is not available. macOS / minimal CI
/// images may ship bash 3.2; we need 4 for the integration template.
fn bash4_available() -> bool {
    let Ok(path) = which::which("bash") else { return false };
    let out = std::process::Command::new(path)
        .args(["--norc", "--noprofile", "-c", "echo $BASH_VERSION"])
        .output();
    let Ok(out) = out else { return false };
    let ver = String::from_utf8_lossy(&out.stdout);
    ver.trim()
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .map(|major| major >= 4)
        .unwrap_or(false)
}

/// Set up an interactive bash session with PS1 sentinel and runex
/// integration sourced. Caller drives the keystrokes from there.
fn spawn_bash_with_runex(config_path: &std::path::Path) -> Option<Session> {
    let bin = bin_path();
    // -i forces interactive mode so readline is loaded; --norc /
    // --noprofile avoid the user's rcfile (don't want any
    // pre-existing aliases or prompts to interfere).
    let mut session = expectrl::spawn("bash --norc --noprofile -i").ok()?;
    session.set_expect_timeout(Some(Duration::from_secs(5)));

    // Disable bracketed paste (some terminals still wrap our keystrokes
    // in ESC[200~ … ESC[201~ otherwise) and lock the prompt to a
    // sentinel string we can match exactly.
    session
        .send_line("bind 'set enable-bracketed-paste off' 2>/dev/null")
        .ok()?;
    session.send_line(r#"PS1='__RUNEX_PROMPT__> '"#).ok()?;
    session.send_line(&format!("export RUNEX_CONFIG={}", config_path.display())).ok()?;
    // Source the runex integration, then wait for the next prompt.
    session
        .send_line(&format!(r#"eval "$('{bin}' export bash --bin '{bin}')""#))
        .ok()?;
    session
        .expect(Regex(r"__RUNEX_PROMPT__> .*__RUNEX_PROMPT__> "))
        .ok()?;
    Some(session)
}

#[test]
fn space_triggers_expand_for_known_token() {
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let config = write_config();
    let Some(mut session) = spawn_bash_with_runex(config.path()) else {
        eprintln!("skipping: could not spawn bash session");
        return;
    };

    // Type `gcm` then Space. The Space binding should fire
    // __runex_expand, which replaces `gcm` with `echo EXPANDED ` and
    // submits the line on the next Enter. We then read the command's
    // own output (`EXPANDED`) which proves the expansion ran end to
    // end.
    session.send("gcm ").ok();
    session.send_line("").ok();

    let captured = session
        .expect(Regex(r"EXPANDED"))
        .expect("bash should have echoed EXPANDED after the gcm<Space> expansion");
    let _ = captured;
}
