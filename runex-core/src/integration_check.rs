//! Health check for the user-side shell-integration scripts.
//!
//! ## Why this module exists
//!
//! Most shells (bash/zsh/pwsh/nu) source `runex export <shell>` at every
//! shell start, so they always see the latest integration template — by
//! construction, they cannot drift.
//!
//! **clink** is the exception: `runex init clink` *copies* the export
//! output into a standalone lua file under clink's scripts directory.
//! The user has to re-run `runex init clink` to refresh it. When the
//! integration template changes (and it has — the hook migration
//! rewrote it from the ground up), users with a stale `runex.lua`
//! silently miss out on the new behavior.
//!
//! For bash/zsh/pwsh/nu we instead check that the rcfile *contains* the
//! init marker — i.e. that integration was ever set up at all. That
//! catches "user never ran `runex init`" but not drift, because drift
//! can't happen.

use std::path::{Path, PathBuf};

use crate::init::{rc_file_for, RUNEX_INIT_MARKER};
use crate::sanitize::sanitize_for_display;
use crate::shell::Shell;

/// Maximum bytes any of the rcfile / clink-lua probes will read.
/// Matches `config::MAX_CONFIG_FILE_BYTES` so the safety story is
/// uniform across the three doctor file-reads.
///
/// 10 MiB is plenty for a real rcfile (multi-megabyte rcfiles are
/// pathological) and for a `runex.lua` (the template renders to a
/// few hundred lines). Anything bigger is treated as if the file
/// could not be read.
const MAX_PROBE_FILE_BYTES: u64 = 10 * 1024 * 1024;

/// Read a file with the same safety guarantees as
/// `config::read_config_source`: refuses non-regular files (FIFO /
/// device nodes), refuses oversized content, and on Unix uses
/// `O_NOFOLLOW | O_NONBLOCK` so a symlink at the final path
/// component is rejected and `open()` cannot block on a named pipe
/// with no writer. Returns `None` for any failure mode (the doctor
/// callers all treat a `None` read as "skip / missing", which is
/// the correct fail-safe outcome).
fn read_capped_regular_file(path: &Path) -> Option<String> {
    use std::io::Read;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .ok()?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    if meta.len() > MAX_PROBE_FILE_BYTES {
        return None;
    }
    let mut content = String::new();
    file.read_to_string(&mut content).ok()?;
    Some(content)
}

/// Outcome of a single integration check, deliberately small so the
/// caller can convert it into a `doctor::Check` without coupling this
/// module to `doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationCheck {
    /// Integration is in place and (where checkable) up-to-date.
    Ok { name: String, detail: String },
    /// Integration is reachable but content has drifted from what
    /// `runex export <shell>` would produce now (clink only).
    Outdated {
        name: String,
        detail: String,
        path: PathBuf,
    },
    /// Integration could not be located at any of the expected paths.
    Missing { name: String, detail: String },
    /// Check did not apply (e.g. user has no rcfile for this shell —
    /// they probably don't use it).
    Skipped { name: String, detail: String },
}

impl IntegrationCheck {
    pub fn name(&self) -> &str {
        match self {
            IntegrationCheck::Ok { name, .. }
            | IntegrationCheck::Outdated { name, .. }
            | IntegrationCheck::Missing { name, .. }
            | IntegrationCheck::Skipped { name, .. } => name,
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            IntegrationCheck::Ok { detail, .. }
            | IntegrationCheck::Outdated { detail, .. }
            | IntegrationCheck::Missing { detail, .. }
            | IntegrationCheck::Skipped { detail, .. } => detail,
        }
    }
}

/// Compare the user's clink integration script against `current_export`
/// (= what `runex export clink` produces today).
///
/// `search_paths` is the ordered list of file paths to probe. The first
/// existing file wins; subsequent paths are not consulted. This lets
/// callers decide policy (env var override, default location, …).
pub fn check_clink_lua_freshness(current_export: &str, search_paths: &[PathBuf]) -> IntegrationCheck {
    for candidate in search_paths {
        if let Some(on_disk) = read_capped_regular_file(candidate) {
            return if normalize_newlines(&on_disk) == normalize_newlines(current_export) {
                IntegrationCheck::Ok {
                    name: "integration:clink".into(),
                    detail: format!(
                        "up-to-date at {}",
                        sanitize_for_display(&candidate.display().to_string())
                    ),
                }
            } else {
                IntegrationCheck::Outdated {
                    name: "integration:clink".into(),
                    detail: format!(
                        "outdated at {} — re-run `runex init clink` to refresh",
                        sanitize_for_display(&candidate.display().to_string())
                    ),
                    path: candidate.clone(),
                }
            };
        }
    }
    // No file at any candidate path. Mirror the rcfile-marker policy:
    // a missing integration script most likely means the user doesn't
    // run clink. Don't shout about it. (Linux dev boxes hit this on
    // every `runex doctor` invocation.) Drift is what we actually care
    // about here — and that's a separate branch above.
    IntegrationCheck::Skipped {
        name: "integration:clink".into(),
        detail: "no clink integration found — assuming clink is not in use".into(),
    }
}

/// Confirm that the rcfile for `shell` mentions the runex init marker.
/// `rcfile_override` is for tests; production callers pass `None` and
/// fall back to [`crate::init::rc_file_for`].
pub fn check_rcfile_marker(shell: Shell, rcfile_override: Option<&Path>) -> IntegrationCheck {
    let name = format!("integration:{}", shell_short_name(shell));
    let path = match rcfile_override {
        Some(p) => p.to_path_buf(),
        None => match rc_file_for(shell) {
            Some(p) => p,
            None => {
                return IntegrationCheck::Skipped {
                    name,
                    detail: "no rcfile concept for this shell".into(),
                }
            }
        },
    };
    if !path.exists() {
        return IntegrationCheck::Skipped {
            name,
            detail: format!(
                "rcfile not found at {} — assuming this shell is not in use",
                sanitize_for_display(&path.display().to_string())
            ),
        };
    }
    let content = match read_capped_regular_file(&path) {
        Some(s) => s,
        None => {
            return IntegrationCheck::Missing {
                name,
                detail: format!(
                    "could not read {} — `runex init {}` may not have been run, or the file is not a regular file or exceeds the safety cap",
                    sanitize_for_display(&path.display().to_string()),
                    shell_short_name(shell)
                ),
            };
        }
    };
    if content.contains(RUNEX_INIT_MARKER) {
        IntegrationCheck::Ok {
            name,
            detail: format!(
                "marker found in {}",
                sanitize_for_display(&path.display().to_string())
            ),
        }
    } else {
        IntegrationCheck::Missing {
            name,
            detail: format!(
                "marker missing in {} — run `runex init {}`",
                sanitize_for_display(&path.display().to_string()),
                shell_short_name(shell)
            ),
        }
    }
}

/// Default ordered list of paths to probe for the clink lua file on
/// this platform. Callers may extend or override this list.
pub fn default_clink_lua_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("RUNEX_CLINK_LUA_PATH") {
        if !p.is_empty() {
            out.push(PathBuf::from(p));
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        out.push(PathBuf::from(local).join("clink").join("runex.lua"));
    }
    if let Some(home) = dirs::home_dir() {
        // Linux clink fork (rare): keeps state under ~/.local/share/clink.
        out.push(home.join(".local").join("share").join("clink").join("runex.lua"));
    }
    out
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn shell_short_name(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Pwsh => "pwsh",
        Shell::Clink => "clink",
        Shell::Nu => "nu",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn clink_lua_match_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("runex.lua");
        write(&p, "-- runex shell integration for clink\nlocal RUNEX_BIN = \"r\"\n");
        let r = check_clink_lua_freshness(
            "-- runex shell integration for clink\nlocal RUNEX_BIN = \"r\"\n",
            &[p.clone()],
        );
        assert!(
            matches!(r, IntegrationCheck::Ok { .. }),
            "expected Ok, got {r:?}"
        );
    }

    /// CRLF on disk vs LF in our generated string must NOT count as drift.
    /// Files saved on Windows often round-trip with CRLF.
    #[test]
    fn clink_lua_match_normalises_newlines() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("runex.lua");
        write(&p, "line1\r\nline2\r\n");
        let r = check_clink_lua_freshness("line1\nline2\n", &[p.clone()]);
        assert!(
            matches!(r, IntegrationCheck::Ok { .. }),
            "CRLF/LF mismatch must not flag drift; got {r:?}"
        );
    }

    #[test]
    fn clink_lua_drift_returns_outdated() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("runex.lua");
        write(&p, "old script\n");
        let r = check_clink_lua_freshness("new script\n", &[p.clone()]);
        match r {
            IntegrationCheck::Outdated { path, .. } => assert_eq!(path, p),
            other => panic!("expected Outdated, got {other:?}"),
        }
    }

    /// When the only candidate path doesn't exist, treat it as "user
    /// doesn't run clink" and skip silently. Linux machines hit this
    /// branch on every `runex doctor` and don't deserve a warning for
    /// not having a Windows shell installed.
    #[test]
    fn clink_lua_not_found_is_skipped() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("does_not_exist.lua");
        let r = check_clink_lua_freshness("anything\n", &[p]);
        assert!(matches!(r, IntegrationCheck::Skipped { .. }), "got {r:?}");
    }

    #[test]
    fn rcfile_marker_present_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join(".bashrc");
        write(
            &p,
            "alias ll=ls\n\n# runex-init\neval \"$(runex export bash)\"\n",
        );
        let r = check_rcfile_marker(Shell::Bash, Some(&p));
        assert!(matches!(r, IntegrationCheck::Ok { .. }), "got {r:?}");
    }

    #[test]
    fn rcfile_marker_absent_returns_missing() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join(".bashrc");
        write(&p, "alias ll=ls\nexport PATH=...\n");
        let r = check_rcfile_marker(Shell::Bash, Some(&p));
        assert!(matches!(r, IntegrationCheck::Missing { .. }), "got {r:?}");
    }

    /// A non-existent rcfile means the user doesn't use this shell.
    /// That's not an error — just skip the check.
    #[test]
    fn rcfile_missing_returns_skipped() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("nonexistent.zshrc");
        let r = check_rcfile_marker(Shell::Zsh, Some(&p));
        assert!(matches!(r, IntegrationCheck::Skipped { .. }), "got {r:?}");
    }

    /// clink has no rcfile concept; passing it without an override must skip.
    #[test]
    fn rcfile_check_for_clink_skips_when_no_override() {
        let r = check_rcfile_marker(Shell::Clink, None);
        assert!(
            matches!(r, IntegrationCheck::Skipped { .. }),
            "clink without override must skip; got {r:?}"
        );
    }

    /// An rcfile larger than the safety cap must not cause `runex doctor`
    /// to read the whole thing into memory. The check should treat
    /// oversized content the same as missing — `Missing` if the file
    /// exists but exceeds the cap (we can't know if the marker is
    /// inside without reading, and we refuse to read).
    #[test]
    fn rcfile_marker_check_skips_oversized_file() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join(".bashrc");
        // Write 11 MB; cap is 10 MB.
        let big: Vec<u8> = vec![b'x'; 11 * 1024 * 1024];
        std::fs::write(&p, &big).unwrap();
        let r = check_rcfile_marker(Shell::Bash, Some(&p));
        assert!(
            matches!(r, IntegrationCheck::Missing { .. }),
            "oversized rcfile must be treated as Missing (bypass cap), got {r:?}"
        );
    }

    /// Same DoS/safety property for the clink lua freshness check.
    /// An attacker who controls the clink scripts directory could
    /// otherwise make `runex doctor` read an arbitrarily large file
    /// every invocation.
    #[test]
    fn clink_lua_freshness_skips_oversized_file() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("runex.lua");
        let big: Vec<u8> = vec![b'x'; 11 * 1024 * 1024];
        std::fs::write(&p, &big).unwrap();
        let r = check_clink_lua_freshness("anything\n", &[p]);
        // Treated as if the file weren't readable at all: Skipped /
        // Missing — the important thing is that we don't load the
        // 11 MB into a String.
        assert!(
            matches!(r, IntegrationCheck::Skipped { .. } | IntegrationCheck::Missing { .. }),
            "oversized clink lua must skip the comparison, got {r:?}"
        );
    }
}
