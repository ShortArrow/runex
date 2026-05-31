//! Windows-local smoke test for the Git Bash bake-mode dispatcher
//! (issue #7 workaround). Runs `bash -c` against the cache file in
//! a non-interactive shell so we don't need a Windows PTY backend
//! (expectrl is not safely available on Windows at the moment).
//!
//! What this covers:
//!
//! 1. `runex export bash` generates a cache file whose bash syntax
//!    is valid under the real Git Bash binary (`bash -n`).
//! 2. With OSTYPE in `(msys, cygwin, msys2)`, sourcing the cache
//!    routes `__runex_expand` to the bake dispatcher
//!    (`__runex_cyg_expand`).
//! 3. Calling `__runex_expand` with `READLINE_LINE=gst` /
//!    `READLINE_POINT=3` rewrites the line to `git status` in pure
//!    bash — no subprocess spawn — which is exactly the property
//!    that fixes the Ctrl+C signal loss on real Git Bash.
//! 4. The `{number}` pattern table renders correctly.
//! 5. The `{}` cursor placeholder is stripped from the rendered
//!    line and the cursor offset is reported back via
//!    `READLINE_POINT`.
//! 6. Non-msys/cygwin OSTYPE values fall through to the exec path.
//!
//! What this does NOT cover (= same as `bash_cygwin_bake_pty.rs`):
//!
//! - The cygwin signal interference that actually motivates the
//!   fix. `bash -c` runs non-interactively and doesn't load
//!   readline, so we can't reproduce the `bind -x` + SIGINT
//!   interaction here. Verifying the fix end-to-end remains a
//!   manual step in the release checklist.
//!
//! Windows only. Skips silently if Git Bash isn't installed at the
//! default Git for Windows path.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::tempdir;

fn runex_bin() -> &'static str {
    env!("CARGO_BIN_EXE_runex")
}

/// Resolve the Git Bash binary. We deliberately avoid `where bash`
/// because the WSL launcher (`C:\Windows\System32\bash.exe`) usually
/// resolves first and is not the cygwin bash we want to test.
fn git_bash() -> Option<PathBuf> {
    let candidates = [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Resolve the MSYS2 bash binary. MSYS2 is a separate install from
/// Git for Windows and is not present by default; tests that take
/// this path skip silently when it isn't found. Where Git Bash
/// reports `OSTYPE=msys`, MSYS2's main `usr/bin/bash` also reports
/// `msys` — the bake dispatcher's `case "${OSTYPE-}"` covers both
/// the same way, so the exercise here is "does the same cache file
/// keep working under a different cygwin-derived shell binary".
///
/// Common install paths (chocolatey, scoop, manual installer):
fn msys2_bash() -> Option<PathBuf> {
    let candidates = [
        r"C:\msys64\usr\bin\bash.exe",
        r"C:\msys2\usr\bin\bash.exe",
        r"C:\tools\msys64\usr\bin\bash.exe",
    ];
    let env_paths = [
        std::env::var("MSYS2_PATH_TYPE").ok(),
        std::env::var("MSYS").ok(),
    ];
    candidates
        .iter()
        .map(PathBuf::from)
        .chain(env_paths.iter().flatten().map(|p| {
            PathBuf::from(p)
                .join("usr")
                .join("bin")
                .join("bash.exe")
        }))
        .find(|p| p.exists())
}

/// Generic resolver used by the parameterised tests below. Returns
/// (label, path) pairs for whichever cygwin-family bash binaries the
/// machine has installed. An empty result means "skip all dispatcher
/// tests" — never a failure on its own, since the suite still
/// catches cache-generation regressions through the
/// `generated_cache_passes_*_syntax_check` tests that run per binary.
fn cygwin_family_bashes() -> Vec<(&'static str, PathBuf)> {
    let mut out = Vec::new();
    if let Some(p) = git_bash() {
        out.push(("Git Bash", p));
    }
    if let Some(p) = msys2_bash() {
        out.push(("MSYS2 bash", p));
    }
    out
}

/// Write a config that exercises every shape the bake path supports
/// and generate the cache file through `runex export bash --bin <...>`.
/// Returns `(cache_path, runex_bin_path)`.
fn build_cache(home: &Path) -> (PathBuf, String) {
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
expand = "git status"

[[abbr]]
key    = "gca"
expand = "git commit -am '{}'"

[[abbr]]
key    = "up{number}"
expand = "cd {number}"
number = "../"
"#,
    )
    .unwrap();

    let bin = runex_bin().to_string();
    let cache_path = home
        .join(".cache")
        .join("runex")
        .join("integration.bash");
    std::fs::create_dir_all(cache_path.parent().unwrap()).unwrap();

    let out = Command::new(&bin)
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "export",
            "bash",
            "--bin",
            &bin,
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "`runex export bash` must succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    std::fs::write(&cache_path, &out.stdout).unwrap();
    (cache_path, bin)
}

/// Convert a Windows path to the POSIX form Git Bash expects in
/// double-quoted strings, e.g. `C:\foo\bar` → `/c/foo/bar`. Git
/// Bash's `bash` accepts both, but POSIX form keeps backslash-vs-
/// escape ambiguity out of the test fixtures.
fn to_posix_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        let drive = s.as_bytes()[0].to_ascii_lowercase() as char;
        format!("/{}{}", drive, &s[2..])
    } else {
        s
    }
}

/// Strip the cache file's non-interactive early-return guard so we
/// can source it from `bash -c`. The guard
/// (`case $- in *i*) ;; *) return 0 ;; esac`) is intentionally
/// emitted by the cache template — it prevents cron / CI scripts
/// from accidentally loading abbreviation tables. For this smoke
/// test, though, we want the bake dispatcher to install itself even
/// under non-interactive bash, so we copy the cache into the temp
/// dir with that single guard block elided. The rest of the file
/// (and crucially, the dispatcher selection `case "${OSTYPE-}"`) is
/// preserved bit-for-bit.
fn cache_without_interactive_guard(src: &Path, dst: &Path) {
    let body = std::fs::read_to_string(src).unwrap();
    let mut out_lines: Vec<&str> = Vec::with_capacity(body.lines().count());
    let mut skipping = 0u8;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if skipping == 0 && trimmed.starts_with("case $- in") {
            // Skip the next two lines (`  *i*) ;;` and
            // `  *) return 0 ;;`) and the closing `esac`.
            skipping = 4;
        }
        if skipping > 0 {
            skipping -= 1;
            continue;
        }
        out_lines.push(line);
    }
    std::fs::write(dst, out_lines.join("\n") + "\n").unwrap();
}

/// Run a bash script under Git Bash with a given OSTYPE, sourcing
/// the cache file first. Returns stdout (panics on non-zero exit).
fn run_under_gitbash(bash: &Path, cache: &Path, ostype: &str, script: &str) -> String {
    // Strip the interactive guard into a sibling file so we can
    // source it from `bash -c`. The original cache is untouched —
    // every other test in this module reads it as the user would.
    let dst = cache.with_extension("bash.test");
    cache_without_interactive_guard(cache, &dst);

    let wrapper = format!(
        "export OSTYPE={ostype}\nsource '{cache}'\n{script}",
        ostype = ostype,
        cache = to_posix_path(&dst),
        script = script,
    );
    let out = Command::new(bash)
        .args(["--norc", "--noprofile", "-c", &wrapper])
        .output()
        .unwrap_or_else(|e| panic!("failed to invoke bash at {}: {e}", bash.display()));
    assert!(
        out.status.success(),
        "bash script must succeed at {} (OSTYPE={ostype})\nscript:\n{script}\nstdout:\n{}\nstderr:\n{}",
        bash.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8(out.stdout).expect("bash stdout must be UTF-8")
}

/// Wrap a `run_under_gitbash` call with a per-binary label so
/// failure messages identify which cygwin-family bash blew up.
fn run_with_label(label: &str, bash: &Path, cache: &Path, ostype: &str, script: &str) -> String {
    let out = run_under_gitbash(bash, cache, ostype, script);
    eprintln!("[{label}] OSTYPE={ostype} stdout:\n{out}");
    out
}

/// Skip-aware iteration: if no cygwin-family bash is on the host,
/// the test prints a skip notice and returns. Otherwise the closure
/// runs once per available binary with its label / path.
fn for_each_cygwin_bash(test_name: &str, body: impl Fn(&str, &Path)) {
    let bashes = cygwin_family_bashes();
    if bashes.is_empty() {
        eprintln!("{test_name}: skipping (no Git Bash or MSYS2 bash installed)");
        return;
    }
    for (label, bash) in bashes {
        body(label, &bash);
    }
}

#[test]
fn generated_cache_passes_syntax_check_on_every_cygwin_bash() {
    for_each_cygwin_bash("generated_cache_passes_syntax_check", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = Command::new(bash)
            .args(["-n", &to_posix_path(&cache)])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "[{label}] `bash -n` must accept the generated cache file\n\
             stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    });
}

#[test]
fn routes_to_bake_dispatcher_under_cygwin_family_ostypes() {
    for_each_cygwin_bash("routes_to_bake_dispatcher", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        // MSYS2's real OSTYPE is `cygwin`, Git Bash's is `msys`.
        // We test both literal values plus `msys2` for completeness;
        // any cygwin-family OSTYPE must select the bake path.
        for ostype in ["msys", "cygwin", "msys2"] {
            let out = run_with_label(
                label,
                bash,
                &cache,
                ostype,
                "declare -f __runex_expand | grep -q __runex_cyg_expand && echo CYG || echo OTHER",
            );
            assert_eq!(
                out.trim(),
                "CYG",
                "[{label}] OSTYPE={ostype} must route to bake dispatcher"
            );
        }
    });
}

#[test]
fn routes_to_exec_dispatcher_under_non_cygwin_ostype() {
    for_each_cygwin_bash("routes_to_exec_dispatcher", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "linux-gnu",
            "declare -f __runex_expand | grep -q __runex_exec_expand && echo EXEC || echo OTHER",
        );
        assert_eq!(
            out.trim(),
            "EXEC",
            "[{label}] OSTYPE=linux-gnu must route to exec dispatcher"
        );
    });
}

#[test]
fn bake_expands_simple_abbreviation_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_expands_simple", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="gst"
READLINE_POINT=3
__runex_expand
echo "LINE=$READLINE_LINE"
echo "POINT=$READLINE_POINT""#,
        );
        assert!(
            out.contains("LINE=git status"),
            "[{label}] bake path must rewrite `gst` to `git status`; got:\n{out}"
        );
        assert!(
            out.contains("POINT=11"),
            "[{label}] bake path must place the cursor at end of `git status ` (11); got:\n{out}"
        );
    });
}

#[test]
fn bake_expands_number_pattern_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_expands_number_pattern", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="up3"
READLINE_POINT=3
__runex_expand
echo "LINE=$READLINE_LINE""#,
        );
        assert!(
            out.contains("LINE=cd ../../../"),
            "[{label}] bake path must render `up3` via the pattern table to `cd ../../../`; got:\n{out}"
        );
    });
}

#[test]
fn bake_strips_cursor_placeholder_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_strips_cursor_placeholder", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="gca"
READLINE_POINT=3
__runex_expand
echo "LINE=$READLINE_LINE"
echo "POINT=$READLINE_POINT""#,
        );
        assert!(
            out.contains("LINE=git commit -am ''"),
            "[{label}] bake path must drop the `{{}}` placeholder; got:\n{out}"
        );
        assert!(
            out.contains("POINT=16"),
            "[{label}] bake path must report cursor offset 16 (between the quotes); got:\n{out}"
        );
    });
}

#[test]
fn bake_self_inserts_unknown_token_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_self_inserts_unknown_token", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="zzzzz"
READLINE_POINT=5
__runex_expand
echo "LINE=$READLINE_LINE"
echo "POINT=$READLINE_POINT""#,
        );
        assert!(
            out.contains("LINE=zzzzz "),
            "[{label}] unknown token must self-insert a space; got:\n{out}"
        );
        assert!(
            out.contains("POINT=6"),
            "[{label}] unknown-token self-insert must advance cursor by 1; got:\n{out}"
        );
    });
}
