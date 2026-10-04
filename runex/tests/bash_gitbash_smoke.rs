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
/// Git for Windows; tests skip silently when it isn't found. MSYS2's
/// `usr/bin/bash` reports `OSTYPE=cygwin` (not `msys`), so a passing
/// MSYS2 run also proves the `cygwin*` arm of the dispatcher case.
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

/// Resolve the upstream Cygwin (cygwin.com) bash binary. Different
/// project from MSYS2 — closer to the original cygwin newlib + DLL,
/// often installed at `C:\cygwin64`. Like MSYS2 it sets
/// `OSTYPE=cygwin`, but the underlying cygwin1.dll is a separate
/// codebase, so a passing run here proves the dispatcher works on
/// the real cygwin runtime (not just msys2's fork). Skipped when
/// not installed, which is the common case on CI.
fn cygwin_bash() -> Option<PathBuf> {
    let candidates = [
        r"C:\cygwin64\bin\bash.exe",
        r"C:\cygwin\bin\bash.exe",
        r"C:\tools\cygwin\bin\bash.exe",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
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
    if let Some(p) = cygwin_bash() {
        out.push(("Cygwin bash", p));
    }
    out
}

/// Write a config that exercises every shape the bake path supports
/// and generate the cache file through `runex export bash --bin <...>`.
/// Returns `(cache_path, runex_bin_path)`.
fn build_cache(home: &Path) -> (PathBuf, String) {
    build_cache_from(home, SHARED_CONFIG)
}

/// Write `config` under `home` and export the bash cache from it, the
/// same way [`build_cache`] does for the shared config.
fn build_cache_from(home: &Path, config: &str) -> (PathBuf, String) {
    let cfg_dir = home.join(".config").join("runex");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let cfg = cfg_dir.join("config.toml");
    std::fs::write(&cfg, config).unwrap();
    export_bash_cache(home, &cfg)
}

const SHARED_CONFIG: &str = r#"version = 1

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

[[abbr]]
key    = "k*"
match  = "glob"
expand = "kubectl {*}"

[[abbr]]
key    = "gs*"
match  = "glob"
expand = "WRONG {*}"

[[abbr]]
key    = "m*"
match  = "glob"
expand = "missing {*}"
when_command_exists = ["runex-no-such-command"]

[[abbr]]
key    = "c*"
match  = "glob"
expand = "echo '{*}{}'"

[[abbr]]
key    = "p*"
match  = "glob"
expand = "pwshonly {*}"
when_command_exists = { pwsh = ["git"] }

[[abbr]]
key    = "e*"
match  = "glob"
expand = "{}{*}"

[[abbr]]
key    = "e*"
match  = "glob"
expand = "E2 {*}"

[[abbr]]
key    = "l*"
match  = "glob"
expand = "{*}{*}{*}{*}"

[[abbr]]
key    = "dup"
expand = "dup{}"

[[abbr]]
key    = "dup"
expand = "second"

[[abbr]]
key    = "chn"
expand = "first"
when_command_exists = ["runex-no-such-command"]

[[abbr]]
key    = "chn"
expand = "third {}end"

[[abbr]]
key    = "pwx"
expand = "pwshexact"
when_command_exists = { pwsh = ["git"] }

[[abbr]]
key    = "wn{number}"
expand = "cond-number {number}"
number = "a"
when_command_exists = ["runex-no-such-command"]

[[abbr]]
key    = "amp{number}"
expand = "A{number}B"
number = "&"

[[abbr]]
key    = "bs{number}"
expand = "S{number}{}E"
number = '\'

[[abbr]]
key    = "oc{number}"
expand = "O{number}"
number = "x"

[[abbr]]
key    = "nb{number}"
expand = "x{number}"
number = "ääääääääääääääää"

[[abbr]]
key    = "{number}zp"
expand = "NP{number}"
number = "a"

[[abbr]]
key    = 'r(\w+)'
match  = "regex"
expand = "REGEX {1}"
"#;

fn export_bash_cache(home: &Path, cfg: &Path) -> (PathBuf, String) {
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

// ─── command-position (issue #9) — Windows-local smokes ─────────────
//
// These four tests drive `__runex_expand` directly with crafted
// READLINE_LINE / READLINE_POINT values against every cygwin-family
// bash that happens to be installed (Git Bash, MSYS2, optional
// Cygwin). PTY isn't required: we just inspect the buffer rewrite
// the bake dispatcher produces, which is exactly what the runtime
// would have observed.

#[test]
fn bake_skips_expansion_after_echo_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_skips_expansion_after_echo", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        // `echo gst` with the cursor right after `gst`. `echo ` is
        // not a command position, so the bake dispatcher must
        // self-insert a literal space rather than expanding `gst`.
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="echo gst"
READLINE_POINT=8
__runex_expand
echo "LINE=$READLINE_LINE""#,
        );
        assert!(
            out.contains("LINE=echo gst "),
            "[{label}] bake must NOT expand `gst` after `echo ` (issue #9 \
             argument-position parity); got:\n{out}"
        );
        assert!(
            !out.contains("git status"),
            "[{label}] bake leaked the expanded form into argument position: \
             {out}"
        );
    });
}

#[test]
fn bake_expands_after_sudo_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_expands_after_sudo", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        // `sudo` at the head of the buffer is a command position via
        // the sudo-recursion rule (see `domain::hook::is_command_position`).
        // The bake dispatcher should expand `gst` to its `expand`
        // value (`git status`) right after the trailing space.
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="sudo gst"
READLINE_POINT=8
__runex_expand
echo "LINE=$READLINE_LINE""#,
        );
        assert!(
            out.contains("LINE=sudo git status "),
            "[{label}] bake must expand `gst` after `sudo ` (issue #9 sudo \
             recursion); got:\n{out}"
        );
    });
}

#[test]
fn bake_expands_after_pipe_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_expands_after_pipe", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="cat foo | gst"
READLINE_POINT=13
__runex_expand
echo "LINE=$READLINE_LINE""#,
        );
        assert!(
            out.contains("LINE=cat foo | git status "),
            "[{label}] bake must expand `gst` after `|` (issue #9 pipeline \
             command position); got:\n{out}"
        );
    });
}

#[test]
fn bake_expands_after_and_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_expands_after_and", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="true && gst"
READLINE_POINT=11
__runex_expand
echo "LINE=$READLINE_LINE""#,
        );
        assert!(
            out.contains("LINE=true && git status "),
            "[{label}] bake must expand `gst` after `&&` (issue #9 list \
             command position); got:\n{out}"
        );
    });
}

/// Issue #19: a glob rule expands on the bake path with the `*` capture
/// substituted into `{*}`.
#[test]
fn bake_expands_glob_rule_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_expands_glob_rule", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="kgp"
READLINE_POINT=3
__runex_expand
echo "LINE=[$READLINE_LINE] POINT=$READLINE_POINT""#,
        );
        assert!(
            out.contains("LINE=[kubectl gp ] POINT=11"),
            "[{label}] bake path must render `kgp` via the glob table to `kubectl gp `; got:\n{out}"
        );
    });
}

/// bash 5.2's `patsub_replacement` turns `&` in a `${var//pat/rep}`
/// replacement into the matched text; the capture must stay literal.
#[test]
fn bake_glob_capture_keeps_ampersand_literal_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_glob_capture_ampersand", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="ka&b"
READLINE_POINT=4
__runex_expand
echo "LINE=[$READLINE_LINE]""#,
        );
        assert!(
            out.contains("LINE=[kubectl a&b ]"),
            "[{label}] the glob capture must be inserted literally; got:\n{out}"
        );
    });
}

/// A glob rule whose `when_command_exists` fails must not fire on the
/// bake path, matching the exec path.
#[test]
fn bake_glob_rule_respects_when_command_exists_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_glob_rule_when_command_exists", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="mx"
READLINE_POINT=2
__runex_expand
echo "LINE=[$READLINE_LINE]""#,
        );
        assert!(
            out.contains("LINE=[mx ]"),
            "[{label}] a glob rule with a missing command must insert a plain space; got:\n{out}"
        );
    });
}

/// A `{}` typed inside the glob capture is literal text; only the
/// template's own `{}` places the cursor (review of #46).
#[test]
fn bake_glob_capture_braces_do_not_move_the_cursor_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_glob_capture_braces", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="cA{}B"
READLINE_POINT=5
__runex_expand
echo "LINE=[$READLINE_LINE] POINT=$READLINE_POINT"
READLINE_LINE="k{}x"
READLINE_POINT=4
__runex_expand
echo "LINE=[$READLINE_LINE] POINT=$READLINE_POINT""#,
        );
        assert!(
            out.contains("LINE=[echo 'A{}B'] POINT=10"),
            "[{label}] the template's {{}} must place the cursor; got:\n{out}"
        );
        assert!(
            out.contains("LINE=[kubectl {}x ] POINT=12"),
            "[{label}] a typed {{}} without a template placeholder stays literal; got:\n{out}"
        );
    });
}

/// Parity with the exec path found by differential review of #46.
#[test]
fn bake_glob_edge_cases_match_the_exec_path_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_glob_edge_cases", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="px"; READLINE_POINT=2; __runex_expand
echo "PWSH_ONLY=[$READLINE_LINE]"
shopt -s nocasematch
READLINE_LINE="Kx"; READLINE_POINT=2; __runex_expand
echo "NOCASE=[$READLINE_LINE]"
shopt -q nocasematch && echo "NOCASE_RESTORED=on"
shopt -u nocasematch
READLINE_LINE="e"; READLINE_POINT=1; __runex_expand
echo "EMPTY=[$READLINE_LINE]"
export LC_ALL=C.UTF-8
tok="l"; for i in $(seq 1 600); do tok="${tok}ä"; done
READLINE_LINE="$tok"; READLINE_POINT=${#tok}; __runex_expand
[ "$READLINE_LINE" = "$tok " ] && echo "BYTECAP=ok""#,
        );
        assert!(out.contains("PWSH_ONLY=[px ]"), "[{label}] a rule whose condition has no bash entry is skipped; got:\n{out}");
        assert!(out.contains("NOCASE=[Kx ]"), "[{label}] glob matching is case-sensitive even under nocasematch; got:\n{out}");
        assert!(out.contains("NOCASE_RESTORED=on"), "[{label}] the user's nocasematch setting is restored; got:\n{out}");
        assert!(
            out.contains("EMPTY=[E2  ]"),
            "[{label}] a glob rule that renders to nothing is skipped and the next glob rule is tried; got:\n{out}"
        );
        assert!(out.contains("BYTECAP=ok"), "[{label}] the 4096 cap counts bytes, as the exec path does; got:\n{out}");
    });
}

/// Issue #47: exact and `{number}` rules on the bake path follow the
/// exec path's skip-and-fall-through semantics, per-rule conditions,
/// decimal counts, literal units and the 4096-byte cap.
#[test]
fn bake_exact_and_number_rules_match_the_exec_path_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_exact_and_number_rules", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"export LC_ALL=C.UTF-8
READLINE_LINE="dup"; READLINE_POINT=3; __runex_expand; echo "DUP=[$READLINE_LINE] P=$READLINE_POINT"
READLINE_LINE="chn"; READLINE_POINT=3; __runex_expand; echo "CHN=[$READLINE_LINE] P=$READLINE_POINT"
READLINE_LINE="pwx"; READLINE_POINT=3; __runex_expand; echo "PWX=[$READLINE_LINE] P=$READLINE_POINT"
READLINE_LINE="wn2"; READLINE_POINT=3; __runex_expand; echo "WN=[$READLINE_LINE] P=$READLINE_POINT"
READLINE_LINE="amp2"; READLINE_POINT=4; __runex_expand; echo "AMP=[$READLINE_LINE] P=$READLINE_POINT"
READLINE_LINE="bs1"; READLINE_POINT=3; __runex_expand; echo "BS=[$READLINE_LINE] P=$READLINE_POINT"
READLINE_LINE="oc010"; READLINE_POINT=5; __runex_expand; echo "OC10=[$READLINE_LINE] P=$READLINE_POINT"
READLINE_LINE="oc08"; READLINE_POINT=4; __runex_expand; echo "OC8=[$READLINE_LINE] P=$READLINE_POINT"
READLINE_LINE="nb128"; READLINE_POINT=5; __runex_expand; echo "NB=[$READLINE_LINE] P=$READLINE_POINT"
READLINE_LINE="3zp"; READLINE_POINT=3; __runex_expand; echo "NOPREFIX=[$READLINE_LINE] P=$READLINE_POINT""#,
        );
        let expected = [
            ("DUP=[dup] P=3", "the first of two rules with the same key wins; `dup{}` only adds a cursor and is not a self-loop"),
            ("CHN=[third end] P=6", "a failed condition skips only its own rule"),
            ("PWX=[pwx ] P=4", "an exact rule whose condition has no bash entry is skipped"),
            ("WN=[wn2 ] P=4", "a {number} rule respects when_command_exists"),
            ("AMP=[A&&B ] P=5", "an & unit is inserted literally"),
            ("BS=[S\\E] P=2", "a backslash unit is inserted literally"),
            ("OC10=[Oxxxxxxxxxx ] P=12", "a leading-zero count is decimal"),
            ("OC8=[Oxxxxxxxx ] P=10", "08 is the decimal count 8"),
            ("NB=[nb128 ] P=6", "the 4096 cap on a {number} rule counts bytes"),
            ("NOPREFIX=[NPaaa ] P=6", "a {number} key with no prefix matches"),
        ];
        for (needle, why) in expected {
            assert!(out.contains(needle), "[{label}] {why}: expected `{needle}`; got:\n{out}");
        }
    });
}

/// Review of #48: `when_command_exists` looks only at PATH (a bash
/// builtin such as `shopt` is not a command there, matching the exec
/// path's `which`), `{number}` substitution ignores `nocasematch`, and a
/// command missing from PATH is looked up once per expansion even when
/// many rules name it. A command name starting with `-` is a name, not an
/// option to the lookup.
#[test]
fn bake_conditions_and_number_rendering_match_the_exec_path_on_every_cygwin_bash() {
    let mut config = String::from(
        r#"version = 1

[keybind.trigger]
default = "space"

[[abbr]]
key    = "bi"
expand = "builtin-wrong"
when_command_exists = ["shopt"]

[[abbr]]
key    = "bi"
expand = "bi2"

[[abbr]]
key    = "da"
expand = "dash-wrong"
when_command_exists = ["-t"]

[[abbr]]
key    = "da"
expand = "da2"

[[abbr]]
key    = "zz"
expand = "{}"

[[abbr]]
key    = "ez{number}"
expand = "{}"
number = "x"

[[abbr]]
key    = "cn{number}"
expand = "cn {number} {NUMBER}"
number = "x"
"#,
    );
    for i in 0..100 {
        config.push_str(&format!(
            "\n[[abbr]]\nkey    = \"pf\"\nexpand = \"never{i}\"\nwhen_command_exists = [\"runex-no-such-command\"]\n"
        ));
    }
    config.push_str("\n[[abbr]]\nkey    = \"pf\"\nexpand = \"pfok\"\n");
    for_each_cygwin_bash("bake_conditions_and_number_rendering", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache_from(dir.path(), &config);
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"export LC_ALL=C.UTF-8
READLINE_LINE="bi"; READLINE_POINT=2; __runex_expand; echo "BI=[$READLINE_LINE]"
READLINE_LINE="da"; READLINE_POINT=2; __runex_expand; echo "DA=[$READLINE_LINE]"
READLINE_LINE="zz"; READLINE_POINT=2; __runex_expand; echo "ZZ=[$READLINE_LINE] P=$READLINE_POINT"
READLINE_LINE="echo a; ez2"; READLINE_POINT=11; __runex_expand; echo "EZ=[$READLINE_LINE] P=$READLINE_POINT"
shopt -s nocasematch
READLINE_LINE="cn2"; READLINE_POINT=3; __runex_expand; echo "CN=[$READLINE_LINE]"
shopt -q nocasematch && echo "NOCASE_RESTORED=on"
shopt -u nocasematch
start=${EPOCHREALTIME/./}
READLINE_LINE="pf"; READLINE_POINT=2; __runex_expand
end=${EPOCHREALTIME/./}
echo "PF=[$READLINE_LINE] MS=$(( (end - start) / 1000 ))""#,
        );
        assert!(out.contains("BI=[bi2 ]"), "[{label}] a builtin is not a PATH command; got:\n{out}");
        assert!(out.contains("DA=[da2 ]"), "[{label}] `-t` is a missing command, not an option; got:
{out}");
        assert!(out.contains("ZZ=[] P=0"), "[{label}] an expand of only {{}} empties the line like exec; got:
{out}");
        assert!(out.contains("EZ=[echo a; ] P=8"), "[{label}] a {{number}} rule rendering to nothing still fires, like exec; got:
{out}");
        assert!(out.contains("CN=[cn xx {NUMBER} ]"), "[{label}] {{number}} substitution is case-sensitive; got:\n{out}");
        assert!(out.contains("NOCASE_RESTORED=on"), "[{label}] the user's nocasematch is restored; got:\n{out}");
        assert!(out.contains("PF=[pfok ]"), "[{label}] the fallback after 100 failing rules fires; got:\n{out}");
        let ms: u64 = out
            .split("MS=")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("[{label}] no timing in output:\n{out}"));
        assert!(ms < 500, "[{label}] 100 rules naming one missing command took {ms} ms; got:\n{out}");
    });
}

/// Regex rules (issue #20) are not baked: on the bake path a token only
/// a regex rule matches gets a plain space, never an expansion.
#[test]
fn bake_never_expands_a_regex_rule_on_every_cygwin_bash() {
    for_each_cygwin_bash("bake_regex_rule_not_baked", |label, bash| {
        let dir = tempdir().unwrap();
        let (cache, _bin) = build_cache(dir.path());
        let out = run_with_label(
            label,
            bash,
            &cache,
            "msys",
            r#"READLINE_LINE="rab"
READLINE_POINT=3
__runex_expand
echo "LINE=[$READLINE_LINE]""#,
        );
        assert!(
            out.contains("LINE=[rab ]"),
            "[{label}] a regex rule must not expand on the bake path; got:\n{out}"
        );
    });
}
