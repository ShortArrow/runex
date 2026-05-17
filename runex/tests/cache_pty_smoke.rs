//! End-to-end smoke test for the 0.1.15 static-cache hook path.
//!
//! The existing `bash_pty_integration.rs` bootstraps the hook with
//! the **legacy** `eval "$(runex export bash --bin ...)"` form, so it
//! proves the hook still works when invoked the old way. It does
//! *not* prove the new static-cache path — the one a user actually
//! gets after running `runex init bash` on 0.1.15 — wires up the
//! Space binding correctly.
//!
//! This file fills that gap. We:
//!
//! 1. run the real `runex init bash --yes` against an isolated HOME,
//!    same as a user would on their first 0.1.15 invocation;
//! 2. spawn an interactive bash with `--rcfile <home>/.bashrc`, the
//!    actual rcfile that `runex init` just wrote (which `source`s
//!    the cache file under `<home>/.cache/runex/integration.bash`);
//! 3. type `gst<Space><Enter>` through a PTY and observe that the
//!    expansion fires (the shell ends up executing `echo EXPANDED`
//!    and the line `EXPANDED` shows up on stdout).
//!
//! Linux only — the cache layout depends on XDG_CACHE_HOME and
//! requires bash 4+ for the `bind -x` trigger.

#![cfg(target_family = "unix")]

use std::process::Command;
use std::time::Duration;

use tempfile::tempdir;

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_runex")
}

fn bash4_available() -> bool {
    let Ok(path) = which::which("bash") else { return false };
    let out = Command::new(path)
        .args(["--norc", "--noprofile", "-c", "echo $BASH_VERSION"])
        .output();
    let Ok(out) = out else { return false };
    let ver = String::from_utf8_lossy(&out.stdout);
    ver.trim()
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .is_some_and(|major| major >= 4)
}

/// Run `runex init bash --yes` exactly as a user would: isolated HOME,
/// isolated XDG_CACHE_HOME and XDG_CONFIG_HOME, the config inside that
/// home, no special wrapper. Returns the rcfile path that init wrote.
fn user_runs_init_bash(home: &std::path::Path) -> std::path::PathBuf {
    let bin = bin_path();
    let cfg_dir = home.join(".config").join("runex");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let cfg = cfg_dir.join("config.toml");
    std::fs::write(
        &cfg,
        "version = 1\n\n[keybind.trigger]\ndefault = \"space\"\n\n[[abbr]]\nkey = \"gst\"\nexpand = \"echo EXPANDED\"\n",
    )
    .unwrap();
    let out = Command::new(bin)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("SHELL", "/bin/bash")
        .args(["--config", cfg.to_str().unwrap(), "init", "bash", "--yes"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "`runex init bash --yes` must succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    home.join(".bashrc")
}

#[test]
fn user_typed_trigger_key_expands_via_static_cache() {
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let rcfile = user_runs_init_bash(home);
    let cache = home.join(".cache").join("runex").join("integration.bash");
    assert!(cache.exists(), "init must have written the cache file at {}", cache.display());
    assert!(rcfile.exists(), "init must have written the rcfile at {}", rcfile.display());

    // Spawn an interactive bash that reads the rcfile init wrote.
    // `--norc` would skip /etc/bash.bashrc; we want to mimic a real
    // user-session shell, so let bash do its normal bootstrap and
    // then load our isolated rcfile via `--rcfile`. The `-i` is
    // required for `bind -x` (the Space trigger) to take effect.
    let bash = which::which("bash").expect("bash on PATH");
    let mut session = expectrl::spawn(format!(
        "{bash} --rcfile {rc} -i",
        bash = bash.display(),
        rc = rcfile.display()
    ))
    .expect("spawn interactive bash");
    session.set_expect_timeout(Some(Duration::from_secs(5)));

    // Override HOME / XDG so the child bash's later subprocesses don't
    // contaminate the real user's `~`. expectrl spawns via `sh -c` so
    // we use `export` after the prompt.
    session
        .send_line(&format!("export HOME={}", home.display()))
        .ok();
    session
        .send_line(&format!(
            "export XDG_CACHE_HOME={} XDG_CONFIG_HOME={}",
            home.join(".cache").display(),
            home.join(".config").display()
        ))
        .ok();

    // Disable bracketed paste so individual sends aren't wrapped in
    // ESC[200~ … ESC[201~ by bash's readline.
    session
        .send_line("bind 'set enable-bracketed-paste off' 2>/dev/null")
        .ok();
    // Pin a deterministic prompt sentinel so a future expect_regex on
    // arbitrary banners doesn't false-trip.
    session.send_line("PS1='__PTY__ '").ok();
    // Wait for the prompt to settle before sending the abbreviation.
    use expectrl::Regex;
    session.expect(Regex(r"__PTY__\s*$")).ok();

    // Type the abbreviation + the trigger key. Then submit the line.
    session.send("gst ").ok();
    session.send_line("").ok();

    // The expansion replaces `gst ` with `echo EXPANDED `; the Enter
    // we just sent executes it; bash prints `EXPANDED` to stdout.
    let saw_expanded = session.expect(Regex(r"EXPANDED")).is_ok();
    // Be polite — try to exit cleanly so the PTY closes.
    session.send_line("exit").ok();

    assert!(
        saw_expanded,
        "the rcfile written by `runex init bash` must wire up the Space \
         binding so that `gst<Space>` expands to `echo EXPANDED` and bash \
         runs it; instead the PTY never saw the word EXPANDED in stdout. \
         This is the regression that would otherwise hide behind the existing \
         `bash_pty_integration.rs`'s legacy-eval bootstrap path."
    );
}
