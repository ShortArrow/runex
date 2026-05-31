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
        .unwrap_or_else(|e| panic!("failed to invoke Git Bash: {e}"));
    assert!(
        out.status.success(),
        "Git Bash script must succeed (OSTYPE={ostype})\nscript:\n{script}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8(out.stdout).expect("Git Bash stdout must be UTF-8")
}

#[test]
fn generated_cache_passes_gitbash_syntax_check() {
    let Some(bash) = git_bash() else {
        eprintln!("skipping: Git Bash not installed");
        return;
    };
    let dir = tempdir().unwrap();
    let (cache, _bin) = build_cache(dir.path());

    let out = Command::new(&bash)
        .args(["-n", &to_posix_path(&cache)])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "Git Bash `bash -n` must accept the generated cache file\n\
         stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn gitbash_routes_to_bake_dispatcher_under_cygwin_family_ostypes() {
    let Some(bash) = git_bash() else {
        eprintln!("skipping: Git Bash not installed");
        return;
    };
    let dir = tempdir().unwrap();
    let (cache, _bin) = build_cache(dir.path());

    for ostype in ["msys", "cygwin", "msys2"] {
        // `declare -F NAME` exits 0 iff NAME is a defined function.
        // We probe the body of __runex_expand for the literal call to
        // __runex_cyg_expand. (`type -p` is unreliable here because
        // the function body is what `case` redefined it to.)
        let out = run_under_gitbash(
            &bash,
            &cache,
            ostype,
            "declare -f __runex_expand | grep -q __runex_cyg_expand && echo CYG || echo OTHER",
        );
        assert!(
            out.trim() == "CYG",
            "OSTYPE={ostype} must route to bake dispatcher, got: {out:?}"
        );
    }
}

#[test]
fn gitbash_routes_to_exec_dispatcher_under_non_cygwin_ostype() {
    let Some(bash) = git_bash() else {
        eprintln!("skipping: Git Bash not installed");
        return;
    };
    let dir = tempdir().unwrap();
    let (cache, _bin) = build_cache(dir.path());

    let out = run_under_gitbash(
        &bash,
        &cache,
        "linux-gnu",
        "declare -f __runex_expand | grep -q __runex_exec_expand && echo EXEC || echo OTHER",
    );
    assert_eq!(
        out.trim(),
        "EXEC",
        "OSTYPE=linux-gnu must route to exec dispatcher"
    );
}

#[test]
fn gitbash_bake_expands_simple_abbreviation() {
    let Some(bash) = git_bash() else {
        eprintln!("skipping: Git Bash not installed");
        return;
    };
    let dir = tempdir().unwrap();
    let (cache, _bin) = build_cache(dir.path());

    // Simulate the trigger: set READLINE_LINE/POINT as readline would
    // and invoke __runex_expand. Pure-bash bake path rewrites them.
    let out = run_under_gitbash(
        &bash,
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
        "bake path must rewrite `gst` to `git status`; got:\n{out}"
    );
    // Default cursor placement is end-of-line + trailing space.
    assert!(
        out.contains("POINT=11"),
        "bake path must place the cursor at end of `git status ` (11); got:\n{out}"
    );
}

#[test]
fn gitbash_bake_expands_number_pattern() {
    let Some(bash) = git_bash() else {
        eprintln!("skipping: Git Bash not installed");
        return;
    };
    let dir = tempdir().unwrap();
    let (cache, _bin) = build_cache(dir.path());

    let out = run_under_gitbash(
        &bash,
        &cache,
        "msys",
        r#"READLINE_LINE="up3"
READLINE_POINT=3
__runex_expand
echo "LINE=$READLINE_LINE""#,
    );
    assert!(
        out.contains("LINE=cd ../../../"),
        "bake path must render `up3` via the pattern table to `cd ../../../`; got:\n{out}"
    );
}

#[test]
fn gitbash_bake_strips_cursor_placeholder_and_reports_offset() {
    let Some(bash) = git_bash() else {
        eprintln!("skipping: Git Bash not installed");
        return;
    };
    let dir = tempdir().unwrap();
    let (cache, _bin) = build_cache(dir.path());

    // `gca` expands to `git commit -am '{}'`. The `{}` is the cursor
    // marker — it must drop out of the rendered line and the cursor
    // should land between the two single quotes. Length of
    // `git commit -am ''` is 17; the cursor sits at byte 16 (just
    // before the closing `'`).
    let out = run_under_gitbash(
        &bash,
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
        "bake path must drop the `{{}}` placeholder; got:\n{out}"
    );
    assert!(
        out.contains("POINT=16"),
        "bake path must report cursor offset 16 (between the quotes); got:\n{out}"
    );
}

#[test]
fn gitbash_bake_self_inserts_unknown_token() {
    let Some(bash) = git_bash() else {
        eprintln!("skipping: Git Bash not installed");
        return;
    };
    let dir = tempdir().unwrap();
    let (cache, _bin) = build_cache(dir.path());

    let out = run_under_gitbash(
        &bash,
        &cache,
        "msys",
        r#"READLINE_LINE="zzzzz"
READLINE_POINT=5
__runex_expand
echo "LINE=$READLINE_LINE"
echo "POINT=$READLINE_POINT""#,
    );
    // Unknown token: insert a literal space at the cursor.
    assert!(
        out.contains("LINE=zzzzz "),
        "unknown token must self-insert a space; got:\n{out}"
    );
    assert!(
        out.contains("POINT=6"),
        "unknown-token self-insert must advance cursor by 1; got:\n{out}"
    );
}
