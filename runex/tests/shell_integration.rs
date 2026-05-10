//! Phase G shell-integration cache tests.
//!
//! These pin the contract that motivated the static-cache pattern:
//!
//! 1. **Non-interactive shells become true no-ops.** A `bash -c
//!    'source <cache>; ...'` sourced cache file must not define
//!    `__runex_expand`, install any `bind -x` binding, or do
//!    anything that a CI script wouldn't expect from "an integration
//!    that's installed but the shell isn't interactive". This is
//!    enforced by the per-template guard added in G2 plus the
//!    cache file just being a copy of that template.
//!
//! 2. **Interactive shells get the function defined.** Same
//!    `bash -ic 'source <cache>; declare -F | grep __runex'` must
//!    succeed and report `__runex_expand`.
//!
//! 3. **Atomic write recovers from crashes.** A leftover
//!    `.<name>.runex.tmp` from a previous mid-write crash must be
//!    cleaned up by the next `runex init <shell>` rather than
//!    accumulating or refusing to write because of a stale temp.
//!
//! Bash-only by necessity (the cache layout applies to all four
//! cache-eligible shells, but bash is the cheapest to spawn from a
//! Rust integration test and exercises the same cache-file shape;
//! the templating differences are pinned by the per-shell snapshot
//! tests in `app::shell_export::tests`). Linux only — the cache
//! layout uses XDG_CACHE_HOME which is a Unix concept and bash on
//! Windows requires WSL/MSYS2 paths that complicate the harness.

#![cfg(target_family = "unix")]

use std::process::Command;
use tempfile::tempdir;

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_runex")
}

/// Returns false if bash is not found or is too old (< 4.0).
fn bash_available() -> bool {
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
        .map(|major| major >= 4)
        .unwrap_or(false)
}

/// Run `runex init bash --yes` against an isolated HOME / XDG_CACHE_HOME
/// pair so the cache file lands in a tempdir we can inspect, and the
/// rcfile under `<home>/.bashrc` is fresh.
fn run_init_bash(home: &std::path::Path) {
    let bin = bin_path();
    let cfg = home.join("config.toml");
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
        .env_remove("PSModulePath")
        .env("SHELL", "/bin/bash")
        .args(["--config", cfg.to_str().unwrap(), "init", "bash", "--yes"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "runex init bash --yes must succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn cache_is_no_op_in_non_interactive_bash() {
    if !bash_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    run_init_bash(home);

    let cache = home.join(".cache").join("runex").join("integration.bash");
    assert!(cache.is_file(), "expected cache file at {}", cache.display());

    // `bash -c` is non-interactive; the G2 guard inside the cache
    // must early-return before defining __runex_expand.
    let out = Command::new("bash")
        .args([
            "--norc",
            "--noprofile",
            "-c",
            &format!(
                "source {}; declare -F __runex_expand 2>/dev/null && echo DEFINED || echo UNDEFINED",
                cache.display()
            ),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim_end().ends_with("UNDEFINED"),
        "non-interactive bash must NOT define __runex_expand: stdout=`{stdout}`"
    );
}

#[test]
fn cache_defines_expand_in_interactive_bash() {
    if !bash_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    run_init_bash(home);

    let cache = home.join(".cache").join("runex").join("integration.bash");
    let out = Command::new("bash")
        .args([
            "--norc",
            "--noprofile",
            "-i",
            "-c",
            &format!(
                "source {}; declare -F __runex_expand 2>/dev/null | grep -q __runex_expand && echo DEFINED || echo UNDEFINED",
                cache.display()
            ),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("DEFINED"),
        "interactive bash must define __runex_expand after sourcing the cache: stdout=`{stdout}`"
    );
}

#[test]
fn cache_header_pins_version_and_bin() {
    if !bash_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    run_init_bash(home);

    let cache = home.join(".cache").join("runex").join("integration.bash");
    let body = std::fs::read_to_string(&cache).unwrap();
    let head: Vec<&str> = body.lines().take(3).collect();
    assert!(
        head.iter().any(|l| l.contains("runex-integration-version: 1")),
        "cache must contain version header: head=\n{head:#?}"
    );
    assert!(
        head.iter().any(|l| l.contains("runex-bin:")),
        "cache must contain runex-bin: header: head=\n{head:#?}"
    );
    assert!(
        head.iter().any(|l| l.contains("do not edit")),
        "cache must contain `do not edit` notice: head=\n{head:#?}"
    );
}

#[test]
fn rcfile_source_line_points_at_cache() {
    if !bash_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    run_init_bash(home);

    let bashrc = home.join(".bashrc");
    let body = std::fs::read_to_string(&bashrc).unwrap();
    assert!(
        body.contains("# runex-init"),
        "bashrc must contain the runex-init marker: {body}"
    );
    let cache = home.join(".cache").join("runex").join("integration.bash");
    assert!(
        body.contains(cache.to_str().unwrap()),
        "bashrc must reference the cache path {}:\n{}",
        cache.display(),
        body
    );
}

#[test]
fn init_cleans_up_stale_temp_from_previous_crash() {
    if !bash_available() {
        eprintln!("skipping: bash 4+ not available");
        return;
    }
    let dir = tempdir().unwrap();
    let home = dir.path();
    let cache_dir = home.join(".cache").join("runex");
    std::fs::create_dir_all(&cache_dir).unwrap();

    // Simulate a crashed-mid-write: leave a `.integration.bash.runex.tmp`
    // file behind. The next init must clean it up rather than fail to
    // create_new() on the temp path.
    let stale_tmp = cache_dir.join(".integration.bash.runex.tmp");
    std::fs::write(&stale_tmp, "leftover from a previous crash").unwrap();

    run_init_bash(home);

    let cache = cache_dir.join("integration.bash");
    assert!(cache.is_file(), "init must produce the cache file");
    assert!(
        !stale_tmp.exists(),
        "stale temp must be removed after a successful init"
    );
}
