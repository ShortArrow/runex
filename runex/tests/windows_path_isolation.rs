//! End-to-end regression: when `runex hook` is launched as a subprocess
//! with a *degraded* PATH (the shape clink's lua `io.popen` produces),
//! command-existence resolution must still succeed by augmenting the
//! search path from HKCU/HKLM. Previously this scenario silently
//! produced a no-op `InsertSpace` action and abbreviations failed to
//! expand.
//!
//! ## What this test pins
//!
//! 1. `runex.exe hook --shell clink --line "ls" --cursor 2`, when run
//!    with `PATH=C:\Windows\System32;C:\Windows` and a config that has
//!    `when_command_exists = ["lsd"]`, MUST emit a `Replace` action
//!    (`return { line = "lsd ", cursor = 4 }`), provided `lsd.exe` is
//!    reachable via the registry's User PATH.
//! 2. The same scenario without the registry-PATH augmentation would
//!    have produced the no-op output (`return { line = "ls ", cursor = 3 }`).
//!
//! The test only runs on Windows. On dev boxes that don't have `lsd` in
//! the registry's User PATH (e.g. CI runners), the precondition check
//! short-circuits the test rather than failing — we cover what we can
//! observe, not what we can't.

#![cfg(target_os = "windows")]

use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

fn bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_runex")
}

/// Probe whether `lsd.exe` is reachable from any directory listed in the
/// HKCU/HKLM `Environment\Path` registry values. Returns the directory
/// where it was found, or `None` to signal that this test environment
/// can't observe the bug we're trying to pin.
fn lsd_reachable_via_registry_path() -> Option<std::path::PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    fn read_path(hive: winreg::HKEY, sub: &str) -> Option<String> {
        let k = RegKey::predef(hive).open_subkey(sub).ok()?;
        k.get_value::<String, _>("Path").ok()
    }

    let user_profile = std::env::var("USERPROFILE").unwrap_or_default();
    let local_app = std::env::var("LOCALAPPDATA").unwrap_or_default();

    let mut sources = Vec::new();
    if let Some(v) = read_path(HKEY_CURRENT_USER, "Environment") {
        sources.push(v);
    }
    if let Some(v) = read_path(
        HKEY_LOCAL_MACHINE,
        r"System\CurrentControlSet\Control\Session Manager\Environment",
    ) {
        sources.push(v);
    }

    for raw in sources {
        for seg in raw.split(';') {
            let expanded = seg
                .replace("%UserProfile%", &user_profile)
                .replace("%USERPROFILE%", &user_profile)
                .replace("%LocalAppData%", &local_app)
                .replace("%LOCALAPPDATA%", &local_app);
            let candidate = std::path::PathBuf::from(&expanded).join("lsd.exe");
            if candidate.is_file() {
                return Some(std::path::PathBuf::from(&expanded));
            }
        }
    }
    None
}

fn write_config_with_ls_to_lsd() -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    write!(
        f,
        "version = 1\n\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"lsd\"]\n"
    )
    .unwrap();
    f.flush().unwrap();
    f
}

#[test]
fn hook_resolves_user_path_binary_under_minimal_process_path() {
    let Some(_lsd_dir) = lsd_reachable_via_registry_path() else {
        eprintln!(
            "skipping: this machine has no `lsd.exe` reachable via HKCU/HKLM Environment\\Path; \
             the test scenario can't be observed here"
        );
        return;
    };

    let cfg = write_config_with_ls_to_lsd();
    let bin = bin_path();

    // System32 + Windows is the minimal PATH a clink-degraded child sees.
    // No `~/.cargo/bin`, no `~/AppData/Local/Microsoft/WinGet/Links` —
    // exactly the shape that bit us in production.
    let minimal_path = r"C:\Windows\System32;C:\Windows";

    let out = Command::new(bin)
        .args([
            "hook", "--shell", "clink", "--line", "ls", "--cursor", "2",
        ])
        .env("PATH", minimal_path)
        .env("RUNEX_CONFIG", cfg.path())
        .output()
        .expect("runex.exe must be runnable");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

    // The successful expansion path returns `Replace` whose lua-rendered
    // form starts with `return { line = "lsd ` (key replaced and a
    // trailing space already inserted) and a cursor at 4 — past `lsd `.
    let expected_replace_prefix = r#"return { line = "lsd ", cursor = 4 }"#;

    // The buggy path (PATH-only resolution) would have produced an
    // `InsertSpace` action: line unchanged with a literal space appended.
    let regression_no_op_form = r#"return { line = "ls ", cursor = 3 }"#;

    assert!(
        !stdout.trim().contains(regression_no_op_form),
        "regression: hook fell back to InsertSpace because `lsd` was not \
         resolvable in the degraded child PATH. Output: {stdout:?}"
    );
    assert!(
        stdout.trim().starts_with(expected_replace_prefix),
        "expected hook to expand `ls` -> `lsd ` (Replace action). Output: {stdout:?}"
    );
}
