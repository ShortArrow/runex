//! Cross-platform helpers that every shell-integration test needs:
//! locating the runex binary, writing a config file, and checking
//! whether a particular shell is on `PATH`.

use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// Absolute path to the freshly-built `runex` binary for this test
/// crate. Cargo sets `CARGO_BIN_EXE_<name>` for every `[[bin]]` in
/// the parent crate, so this is build-system-correct without any
/// path walking.
pub fn runex_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_runex"))
}

/// Same as [`runex_bin`] but as `&'static str`. Useful when the
/// caller needs to format it into a shell command via `format!`,
/// where `Path::display()` would add a borrow that complicates the
/// type story.
pub fn runex_bin_str() -> &'static str {
    env!("CARGO_BIN_EXE_runex")
}

/// Write `toml` to a fresh `NamedTempFile` and return it. The file
/// is deleted when the returned handle drops, so the caller must
/// keep it alive for the duration of any subprocess that reads from
/// `RUNEX_CONFIG`.
///
/// `toml` is taken verbatim — no `version = 1` is prepended; tests
/// that need different schema versions write their own header.
pub fn write_config_file(toml: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("tempfile creation");
    f.write_all(toml.as_bytes()).expect("tempfile write");
    f.flush().expect("tempfile flush");
    f
}

/// Convenience overload: write a single-abbr config that maps `key`
/// to `expand`, with the trigger key explicitly set to `space` for
/// every shell. The default-config path infers `space` too, but PTY-
/// driven tests are sensitive to the `[keybind.trigger]` header
/// being absent (the integration script's keymap won't bind the
/// trigger), so we make it explicit. The cost is one extra TOML
/// section in tests that don't care; the benefit is that tests
/// don't silently no-op if default-trigger plumbing changes.
pub fn write_simple_config(key: &str, expand: &str) -> NamedTempFile {
    write_config_file(&format!(
        "version = 1\n\n[keybind.trigger]\ndefault = \"space\"\n\n[[abbr]]\nkey = \"{key}\"\nexpand = \"{expand}\"\n"
    ))
}

/// Write `toml` to a named path under `dir`. Unlike [`write_config_file`]
/// this returns a `PathBuf` (no auto-cleanup) and is intended for tests
/// that need a stable, predictable path — e.g. when a subprocess will
/// later look up the file by name from a hardcoded directory.
pub fn write_config_at(dir: &Path, name: &str, toml: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, toml).expect("write config at path");
    path
}

/// Returns `true` iff `shell` resolves on the current `PATH`. The
/// shell-specific tests use this for runtime skip guards: a Linux CI
/// runner without zsh installed should silently skip the zsh suite
/// rather than fail.
///
/// Identical semantics to `which::which(shell).is_ok()` — wrapping it
/// here keeps `which` out of every test crate's import list.
pub fn shell_available(shell: &str) -> bool {
    which::which(shell).is_ok()
}

/// Returns `true` iff bash is available **and** is at least version 4.
/// macOS ships bash 3.2 (GPLv2 cut-off) which lacks the readline
/// features the runex bash integration depends on. Tests that source
/// the integration script must skip on bash 3.x.
///
/// `$BASH_VERSION` looks like `"5.2.37(1)-release"`; we parse only
/// the major version.
pub fn bash4_available() -> bool {
    let Ok(path) = which::which("bash") else {
        return false;
    };
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
