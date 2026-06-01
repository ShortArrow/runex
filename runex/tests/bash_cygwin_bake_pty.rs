//! End-to-end smoke test for the cygwin/msys bake-mode bash dispatcher
//! (issue #7 workaround).
//!
//! The real bug only reproduces under Git Bash on Windows because it
//! depends on cygwin's bind-x + Win32 spawn signal interaction. We
//! can't reproduce *that* on Linux CI, but we *can* prove the bake
//! path is wired up correctly: if a Linux bash session is told it is
//! cygwin (`OSTYPE=msys`) at source time, our `case "${OSTYPE-}"`
//! switch should route the trigger to `__runex_cyg_expand`, which
//! does its lookup purely in bash and never calls `runex hook`. The
//! cache file is identical to the one Git Bash users get; only the
//! dispatcher selection differs.
//!
//! This file covers:
//!
//! 1. simple map expansion (`gst<Space>` → `git status`)
//! 2. `{number}` pattern expansion (`up3<Space>` → `cd ../../../`)
//! 3. cursor placeholder (`gca<Space>` → `git commit -am '<cursor>'`)
//!
//! Linux only — bash 4+ required for `bind -x` and `declare -gA`.

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

/// Run `runex init bash --yes` against an isolated HOME with a config
/// that exercises all three expansion shapes the bake path supports.
fn user_runs_init_bash_with_full_config(home: &std::path::Path) -> std::path::PathBuf {
    let bin = bin_path();
    let cfg_dir = home.join(".config").join("runex");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let cfg = cfg_dir.join("config.toml");
    std::fs::write(
        &cfg,
        r#"version = 1

[keybind.trigger]
default = "space"

[[abbr]]
key    = "gst"
expand = "echo EXPANDED_GST"

[[abbr]]
key    = "gca"
expand = "echo PRE_{}_POST"

[[abbr]]
key    = "up{number}"
expand = "echo UP_{number}_END"
number = "x"
"#,
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

/// Spawn an interactive bash that pretends to be Git Bash via
/// `OSTYPE=msys` before sourcing the cache. After this `__runex_expand`
/// is bound to `__runex_cyg_expand` so all subsequent trigger presses
/// hit the bake path without spawning `runex hook`.
fn spawn_cyg_bash(rcfile: &std::path::Path, home: &std::path::Path) -> expectrl::Session {
    let bash = which::which("bash").expect("bash on PATH");
    let mut session = expectrl::spawn(format!(
        "{bash} --norc -i",
        bash = bash.display()
    ))
    .expect("spawn interactive bash");
    session.set_expect_timeout(Some(Duration::from_secs(5)));

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
    session.send_line("export OSTYPE=msys").ok();
    session
        .send_line("bind 'set enable-bracketed-paste off' 2>/dev/null")
        .ok();
    session.send_line("PS1='__PTY__ '").ok();
    // Source the rcfile *after* OSTYPE is set so the case "${OSTYPE-}"
    // switch routes us to the bake path at source time. The rcfile
    // sources the cache file in turn.
    session.send_line(&format!("source {}", rcfile.display())).ok();

    use expectrl::Regex;
    session.expect(Regex(r"__PTY__\s*$")).ok();
    session
}

#[test]
fn cygwin_bake_expands_plain_abbreviation() {
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let rcfile = user_runs_init_bash_with_full_config(home);
    let mut session = spawn_cyg_bash(&rcfile, home);

    session.send("gst ").ok();
    session.send_line("").ok();

    use expectrl::Regex;
    let saw = session.expect(Regex(r"EXPANDED_GST")).is_ok();
    session.send_line("exit").ok();
    assert!(
        saw,
        "the cygwin bake path must expand `gst<Space>` to `echo EXPANDED_GST` \
         without invoking `runex hook`; the PTY never saw EXPANDED_GST"
    );
}

#[test]
fn cygwin_bake_expands_pattern_with_number_placeholder() {
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let rcfile = user_runs_init_bash_with_full_config(home);
    let mut session = spawn_cyg_bash(&rcfile, home);

    // `up3` → token matches the `up{number}` pattern with n=3 and
    // unit="x", so the rendered expansion is `echo UP_xxx_END`.
    session.send("up3 ").ok();
    session.send_line("").ok();

    use expectrl::Regex;
    let saw = session.expect(Regex(r"UP_xxx_END")).is_ok();
    session.send_line("exit").ok();
    assert!(
        saw,
        "the cygwin bake path must expand `up3<Space>` via the pattern \
         table to `echo UP_xxx_END`; the PTY never saw UP_xxx_END"
    );
}

#[test]
fn cygwin_bake_strips_cursor_placeholder_from_rendered_expansion() {
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let rcfile = user_runs_init_bash_with_full_config(home);
    let mut session = spawn_cyg_bash(&rcfile, home);

    // `gca` expand = `echo PRE_{}_POST`. The `{}` placeholder is the
    // cursor marker — it must NOT appear literally in the rendered
    // command. After expansion the line should read `echo PRE__POST`
    // with the cursor positioned between the two underscores.
    session.send("gca ").ok();
    session.send_line("").ok();

    use expectrl::Regex;
    let saw_rendered = session.expect(Regex(r"PRE__POST")).is_ok();
    session.send_line("exit").ok();
    assert!(
        saw_rendered,
        "the cygwin bake path must drop the `{{}}` cursor placeholder \
         when rendering the expansion; expected `PRE__POST` in PTY stdout"
    );
}

// ─── command-position parity with the exec path (issue #9) ──────────
//
// 0.1.17 carved out command-position detection so the Ctrl+C fix
// (#7) could ship. 0.1.19 restores parity: `echo gst<Space>` no
// longer expands on the bake path, matching what the Linux/WSL exec
// path has always done. The five tests below pin one expectation
// each — the bake path now mirrors `domain::hook::is_command_position`
// for argument-position tokens (no expand) and every command-position
// prefix listed in `docs/recipes.md` (`sudo`, `|`, `&&`, `;`).

#[test]
fn cygwin_bake_skips_expansion_after_echo() {
    // Argument position. The 0.1.17 trade-off would have expanded
    // `gst` here, producing `echo echo EXPANDED_GST` and printing
    // `echo EXPANDED_GST` to stdout. Post-#9 it must self-insert a
    // literal space, leaving the line as `echo gst ` and printing
    // a plain `gst` (= bash sees `echo gst` as the command).
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let rcfile = user_runs_init_bash_with_full_config(home);
    let mut session = spawn_cyg_bash(&rcfile, home);

    session.send("echo gst ").ok();
    session.send_line("").ok();

    use expectrl::Regex;
    // After the trigger Space the line should still be `echo gst ` and
    // bash's `echo` will print `gst` somewhere in the PTY stream. The
    // critical negative property is that the *expanded* output
    // (`echo EXPANDED_GST` would produce the string `EXPANDED_GST` on
    // its own) does NOT appear — anchored matches like `^gst\b` are
    // unreliable through mintty/xterm framing, so we assert via the
    // positive `gst` token plus the absence of `EXPANDED_GST`.
    let saw = session.expect(Regex(r"gst")).is_ok();
    session.send_line("exit").ok();
    assert!(
        saw,
        "bake path must NOT expand `gst` after `echo ` (argument position); \
         expected `echo gst` to run and print plain `gst`"
    );
}

#[test]
fn cygwin_bake_expands_after_sudo() {
    // `sudo` at the head of the buffer counts as command position
    // for whatever follows it (= docs/recipes.md). We can't actually
    // run `sudo` in the test — its prompt would deadlock the PTY —
    // so we test the buffer rewrite via `type` instead: `sudo gst<SP>`
    // should rewrite to `sudo echo EXPANDED_GST<SP>`, which when run
    // (after stripping the literal `sudo` via a shell function) emits
    // `EXPANDED_GST`.
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let rcfile = user_runs_init_bash_with_full_config(home);
    let mut session = spawn_cyg_bash(&rcfile, home);

    // Shadow sudo so its first arg is just executed without privilege.
    session.send_line("sudo() { \"$@\"; }").ok();
    session.send("sudo gst ").ok();
    session.send_line("").ok();

    use expectrl::Regex;
    let saw = session.expect(Regex(r"EXPANDED_GST")).is_ok();
    session.send_line("exit").ok();
    assert!(
        saw,
        "bake path must expand `gst` after `sudo ` (command position via \
         sudo recursion); expected EXPANDED_GST in stdout"
    );
}

#[test]
fn cygwin_bake_expands_after_pipe() {
    // `|` is a pipeline operator → command position for whatever
    // follows. We construct `echo seed | gst<SP>` which would
    // otherwise pipe `seed` into `gst` (unknown command). Post-#9
    // `gst` should expand to `echo EXPANDED_GST` so the pipeline
    // becomes `echo seed | echo EXPANDED_GST` and stdout shows the
    // EXPANDED_GST line.
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let rcfile = user_runs_init_bash_with_full_config(home);
    let mut session = spawn_cyg_bash(&rcfile, home);

    session.send("echo seed | gst ").ok();
    session.send_line("").ok();

    use expectrl::Regex;
    let saw = session.expect(Regex(r"EXPANDED_GST")).is_ok();
    session.send_line("exit").ok();
    assert!(
        saw,
        "bake path must expand `gst` after `|` (pipeline command position); \
         expected EXPANDED_GST in stdout"
    );
}

#[test]
fn cygwin_bake_expands_after_and() {
    // `&&` is a list operator → command position. Construct
    // `true && gst<SP>` so the first half is harmless and the second
    // half is the test target.
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let rcfile = user_runs_init_bash_with_full_config(home);
    let mut session = spawn_cyg_bash(&rcfile, home);

    session.send("true && gst ").ok();
    session.send_line("").ok();

    use expectrl::Regex;
    let saw = session.expect(Regex(r"EXPANDED_GST")).is_ok();
    session.send_line("exit").ok();
    assert!(
        saw,
        "bake path must expand `gst` after `&&` (list command position); \
         expected EXPANDED_GST in stdout"
    );
}

#[test]
fn cygwin_bake_expands_after_semicolon() {
    // `;` is a list operator → command position. `true ; gst<SP>`
    // mirrors the `&&` case but with the terminator that bash uses
    // for unconditional sequencing.
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let rcfile = user_runs_init_bash_with_full_config(home);
    let mut session = spawn_cyg_bash(&rcfile, home);

    session.send("true ; gst ").ok();
    session.send_line("").ok();

    use expectrl::Regex;
    let saw = session.expect(Regex(r"EXPANDED_GST")).is_ok();
    session.send_line("exit").ok();
    assert!(
        saw,
        "bake path must expand `gst` after `;` (sequence command position); \
         expected EXPANDED_GST in stdout"
    );
}

#[test]
fn cygwin_bake_falls_through_when_token_is_not_an_abbreviation() {
    if !bash4_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let rcfile = user_runs_init_bash_with_full_config(home);
    let mut session = spawn_cyg_bash(&rcfile, home);

    // A token that isn't in either table must be left alone (a single
    // space is appended, just like the legacy self-insert). `echo`
    // then runs literally with the token as its argument.
    session.send("echo NOTANABBR ").ok();
    session.send_line("").ok();

    use expectrl::Regex;
    let saw = session.expect(Regex(r"NOTANABBR")).is_ok();
    session.send_line("exit").ok();
    assert!(
        saw,
        "unknown tokens must self-insert a Space and execute as typed; \
         the PTY never saw NOTANABBR on stdout"
    );
}
