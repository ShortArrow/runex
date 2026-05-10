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

use crate::domain::sanitize::sanitize_for_display;
use crate::domain::shell::Shell;
use crate::infra::env::{rc_file_for, SystemHomeDir};

/// Marker comment written into rc files by `runex init` to enable
/// idempotent re-runs. Living in `infra::integration_check` rather
/// than `app::init` because both the *write side* (`cmd::init`) and
/// the *read side* (`check_rcfile_marker`) need it, and the latter
/// lives here in `infra`. Phase D moved the constant to break the
/// `app → infra → app` cycle the prior layout had.
pub(crate) const RUNEX_INIT_MARKER: &str = "# runex-init";

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
/// device nodes), refuses oversized content, and refuses symlinks at
/// the final path component on every platform.
///
/// * **Unix:** `O_NOFOLLOW | O_NONBLOCK` make `open()` itself fail
///   on a final-component symlink and prevent a named pipe with no
///   writer from blocking the doctor scan.
/// * **Windows:** `OpenOptions::open()` follows symlinks/reparse
///   points by default, so the protection is layered on by checking
///   `symlink_metadata().file_type().is_symlink()` *before* opening.
///   This covers ordinary symlinks; directory junctions and other
///   reparse-point variants typically fail the `is_file()` check
///   below, but we don't rely on that and the explicit symlink reject
///   keeps policy parity with the clink write path in `main.rs`.
///
/// Returns `None` for any failure mode (the doctor callers all
/// treat a `None` read as "skip / missing", which is the correct
/// fail-safe outcome — never block, never panic, never read).
fn read_capped_regular_file(path: &Path) -> Option<String> {
    use std::io::Read;

    // Cross-platform symlink reject. Cheap (metadata-only) and runs
    // before we call open(). On Unix this is redundant with O_NOFOLLOW
    // but guards against future refactors of the open() flags.
    let lmeta = std::fs::symlink_metadata(path).ok()?;
    if lmeta.file_type().is_symlink() {
        return None;
    }

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
pub(crate) enum IntegrationCheck {
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

/// Compare the user's clink integration script against `current_export`
/// (= what `runex export clink` produces today).
///
/// `search_paths` is the ordered list of file paths to probe. The first
/// existing file wins; subsequent paths are not consulted. This lets
/// callers decide policy (env var override, default location, …).
pub(crate) fn check_clink_lua_freshness(current_export: &str, search_paths: &[PathBuf]) -> IntegrationCheck {
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
/// fall back to [`crate::infra::env::rc_file_for`] with the
/// production [`SystemHomeDir`] resolver.
pub(crate) fn check_rcfile_marker(shell: Shell, rcfile_override: Option<&Path>) -> IntegrationCheck {
    let name = format!("integration:{}", shell_short_name(shell));
    let path = match rcfile_override {
        Some(p) => p.to_path_buf(),
        None => match rc_file_for(shell, &SystemHomeDir) {
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
pub(crate) fn default_clink_lua_paths() -> Vec<PathBuf> {
    default_clink_lua_paths_with(&crate::infra::env::SystemHomeDir)
}

/// Resolver-injectable variant of [`default_clink_lua_paths`]. The
/// non-`_with` version forwards here with a [`SystemHomeDir`].
///
/// Used by tests that need to probe behaviour with a known set of
/// `RUNEX_CLINK_LUA_PATH` / `LOCALAPPDATA` / `home_dir` values
/// without leaking into the process env.
pub(crate) fn default_clink_lua_paths_with(env: &dyn crate::infra::env::HomeDirResolver) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = env.env_var("RUNEX_CLINK_LUA_PATH") {
        out.push(PathBuf::from(p));
    }
    if let Some(local) = env.env_var("LOCALAPPDATA") {
        out.push(PathBuf::from(local).join("clink").join("runex.lua"));
    }
    if let Some(home) = env.home_dir() {
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

/// Phase G cache file freshness check. Reads
/// `<XDG_CACHE_HOME>/runex/integration.<ext>` for `shell` and
/// classifies the result:
///
/// * **Skipped** when the cache path can't be resolved (no
///   `XDG_CACHE_HOME` or `HOME` signal), the file does not exist
///   (= user has not run `runex init <shell>`), or `shell` is
///   `Clink` (clink uses its own freshness probe via
///   `check_clink_lua_freshness`).
/// * **Outdated** when the cache file is present but its
///   `runex-integration-version:` header doesn't match
///   [`crate::infra::integration_cache::INTEGRATION_CACHE_VERSION`],
///   the `runex-bin:` header points at a path that no longer
///   exists, or the header is absent entirely (= legacy `eval
///   "$(runex export bash)"`-style content from before Phase G).
/// * **Ok** when the version matches and the baked-bin path
///   exists on disk.
///
/// Returns `Missing` only on read failure for an existing file
/// (e.g. permission denied) — that's a real problem the user
/// needs to know about.
pub(crate) fn check_cache_freshness(
    shell: Shell,
    env: &dyn crate::infra::env::HomeDirResolver,
) -> IntegrationCheck {
    use crate::infra::integration_cache::{
        cache_path, INTEGRATION_CACHE_MARKER, INTEGRATION_CACHE_VERSION,
    };

    let name = format!("integration:{}:cache", shell_short_name(shell));

    let target = match cache_path(shell, env) {
        Ok(Some(p)) => p,
        Ok(None) => {
            return IntegrationCheck::Skipped {
                name,
                detail: "cache layout does not apply to clink (uses lua autoloader instead)"
                    .into(),
            };
        }
        Err(e) => {
            return IntegrationCheck::Skipped {
                name,
                detail: format!("cache path could not be resolved: {e}"),
            };
        }
    };

    if !target.exists() {
        return IntegrationCheck::Skipped {
            name,
            detail: format!(
                "no cache at {} — run `runex init {}` to install",
                sanitize_for_display(&target.display().to_string()),
                shell_short_name(shell)
            ),
        };
    }

    let content = match read_capped_regular_file(&target) {
        Some(s) => s,
        None => {
            return IntegrationCheck::Missing {
                name,
                detail: format!(
                    "cache file at {} exists but could not be read (not a regular file, or exceeds the 10 MB safety cap)",
                    sanitize_for_display(&target.display().to_string())
                ),
            };
        }
    };

    // Parse version + bin headers from the first ~5 lines. The
    // header format is documented in
    // `infra::integration_cache::cache_header`.
    let mut version_line: Option<&str> = None;
    let mut bin_line: Option<&str> = None;
    for line in content.lines().take(5) {
        if let Some(v) = line.split_once(INTEGRATION_CACHE_MARKER) {
            version_line = Some(v.1.trim());
        } else if let Some(b) = line.find("runex-bin: ") {
            bin_line = Some(&line[b + "runex-bin: ".len()..]);
        }
    }

    let Some(version_str) = version_line else {
        return IntegrationCheck::Outdated {
            name,
            detail: format!(
                "cache at {} has no `runex-integration-version:` header — likely legacy \
                 `eval \"$(runex export {})\"` content. Re-run `runex init {}` to upgrade",
                sanitize_for_display(&target.display().to_string()),
                shell_short_name(shell),
                shell_short_name(shell)
            ),
            path: target,
        };
    };

    let parsed: u32 = match version_str.parse() {
        Ok(v) => v,
        Err(_) => {
            return IntegrationCheck::Outdated {
                name,
                detail: format!(
                    "cache at {} has malformed version header `{}` — re-run `runex init {}` to refresh",
                    sanitize_for_display(&target.display().to_string()),
                    sanitize_for_display(version_str),
                    shell_short_name(shell)
                ),
                path: target,
            };
        }
    };

    if parsed != INTEGRATION_CACHE_VERSION {
        return IntegrationCheck::Outdated {
            name,
            detail: format!(
                "cache at {} is version {} but this runex expects version {} — re-run `runex init {}` to refresh",
                sanitize_for_display(&target.display().to_string()),
                parsed,
                INTEGRATION_CACHE_VERSION,
                shell_short_name(shell)
            ),
            path: target,
        };
    }

    if let Some(bin) = bin_line {
        let bin = bin.trim();
        // Bare `runex` is a deliberate power-user override (Phase
        // G3) — accept it without checking existence on disk
        // since it's PATH-resolved at run time.
        if bin != "runex" && !std::path::Path::new(bin).exists() {
            return IntegrationCheck::Outdated {
                name,
                detail: format!(
                    "cache at {} bakes a `runex-bin:` of {} which no longer exists. \
                     Re-run `runex init {}` to point the cache at the current binary",
                    sanitize_for_display(&target.display().to_string()),
                    sanitize_for_display(bin),
                    shell_short_name(shell)
                ),
                path: target,
            };
        }
    }

    IntegrationCheck::Ok {
        name,
        detail: format!(
            "cache up-to-date at {}",
            sanitize_for_display(&target.display().to_string())
        ),
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
    /// to read the whole thing into memory. The file *exists* (so we
    /// can't `Skipped` the way we do for an absent file), but we
    /// refuse to inspect it — that maps to `Missing` ("we can't
    /// confirm the marker is in there"). Tightly pinned: the
    /// alternative would be silently treating the file as
    /// marker-present, which would be a false-negative for the
    /// "drift / not initialised" check.
    #[test]
    fn rcfile_marker_check_oversized_file_is_missing() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join(".bashrc");
        // Write 11 MB; cap is 10 MB.
        let big: Vec<u8> = vec![b'x'; 11 * 1024 * 1024];
        std::fs::write(&p, &big).unwrap();
        let r = check_rcfile_marker(Shell::Bash, Some(&p));
        // Pin Missing exactly. The contrast with the clink test below
        // (which pins Skipped) is intentional and matches each
        // check's "we can't read this" semantics.
        assert!(
            matches!(r, IntegrationCheck::Missing { .. }),
            "oversized rcfile must be reported as Missing — path.exists() \
             but content can't be inspected; got {r:?}"
        );
    }

    /// A symlink at the final path component must be rejected on
    /// every platform — both for the clink-lua freshness check and
    /// for the rcfile-marker check. Without this, an attacker who
    /// can drop a symlink under `~/.local/share/clink/` (or under
    /// `$HOME` for an rcfile) could redirect doctor's read to any
    /// file the runex process can access — `~/.ssh/id_ed25519` etc.
    /// We `None`-out the read so callers see the safe outcome
    /// (Skipped for clink, Missing for rcfile).
    #[cfg(unix)]
    #[test]
    fn rcfile_marker_check_rejects_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new().unwrap();
        let real = tmp.path().join("real.bashrc");
        write(&real, "alias ll=ls\n# runex-init\neval \"$(runex export bash)\"\n");
        let link = tmp.path().join(".bashrc");
        symlink(&real, &link).unwrap();
        let r = check_rcfile_marker(Shell::Bash, Some(&link));
        // Without the symlink reject, the marker would be found via
        // the symlink and we'd return Ok — masking the redirection.
        assert!(
            matches!(r, IntegrationCheck::Missing { .. }),
            "rcfile_marker must reject a symlink rcfile (return Missing); got {r:?}"
        );
    }

    /// Windows mirror of `rcfile_marker_check_rejects_symlink`.
    /// `symlink_file` requires Developer Mode or admin on Windows;
    /// when the test runner can't create a symlink we *skip the test
    /// silently* rather than fail — there's no way to assert the
    /// guard without first creating a symlink, and forcing every
    /// developer to enable Dev Mode would be hostile. CI's
    /// windows-latest runners do allow it.
    #[cfg(windows)]
    #[test]
    fn rcfile_marker_check_rejects_symlink_windows() {
        use std::os::windows::fs::symlink_file;
        let tmp = TempDir::new().unwrap();
        let real = tmp.path().join("real.bashrc");
        write(&real, "alias ll=ls\n# runex-init\neval \"$(runex export bash)\"\n");
        let link = tmp.path().join(".bashrc");
        if symlink_file(&real, &link).is_err() {
            eprintln!("skipping: symlink creation requires Developer Mode / admin");
            return;
        }
        let r = check_rcfile_marker(Shell::Bash, Some(&link));
        assert!(
            matches!(r, IntegrationCheck::Missing { .. }),
            "rcfile_marker (Windows) must reject a symlink rcfile; got {r:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn clink_lua_freshness_rejects_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new().unwrap();
        let real = tmp.path().join("real.lua");
        write(&real, "anything\n");
        let link = tmp.path().join("runex.lua");
        symlink(&real, &link).unwrap();
        let r = check_clink_lua_freshness("anything\n", &[link]);
        // Without the reject, content match would yield Ok via the
        // symlink. With it, the only candidate is unreadable → Skipped.
        assert!(
            matches!(r, IntegrationCheck::Skipped { .. }),
            "clink_lua_freshness must reject a symlink lua (return Skipped); got {r:?}"
        );
    }

    /// Windows mirror of `clink_lua_freshness_rejects_symlink`. Same
    /// silent-skip behavior when symlink creation requires elevated
    /// privileges.
    #[cfg(windows)]
    #[test]
    fn clink_lua_freshness_rejects_symlink_windows() {
        use std::os::windows::fs::symlink_file;
        let tmp = TempDir::new().unwrap();
        let real = tmp.path().join("real.lua");
        write(&real, "anything\n");
        let link = tmp.path().join("runex.lua");
        if symlink_file(&real, &link).is_err() {
            eprintln!("skipping: symlink creation requires Developer Mode / admin");
            return;
        }
        let r = check_clink_lua_freshness("anything\n", &[link]);
        assert!(
            matches!(r, IntegrationCheck::Skipped { .. }),
            "clink_lua_freshness (Windows) must reject a symlink lua; got {r:?}"
        );
    }

    /// Same DoS/safety property for the clink lua freshness check.
    /// An attacker who controls the clink scripts directory could
    /// otherwise make `runex doctor` read an arbitrarily large file
    /// every invocation. Unlike the rcfile check, the clink scan
    /// loops over candidate paths and falls through to `Skipped`
    /// when no candidate yields readable content — so an oversize
    /// file at the only candidate looks the same as no file at all,
    /// which is the right outcome (clink may not be installed).
    #[test]
    fn clink_lua_freshness_oversized_file_is_skipped() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("runex.lua");
        let big: Vec<u8> = vec![b'x'; 11 * 1024 * 1024];
        std::fs::write(&p, &big).unwrap();
        let r = check_clink_lua_freshness("anything\n", &[p]);
        assert!(
            matches!(r, IntegrationCheck::Skipped { .. }),
            "oversized clink lua at the only candidate must be Skipped \
             (drift undetectable, treat as no integration installed), got {r:?}"
        );
    }

    /// `default_clink_lua_paths_with` (Phase B Step B3) is the
    /// resolver-injectable variant. These tests pin its honour for
    /// each env knob a real install consults: explicit
    /// `RUNEX_CLINK_LUA_PATH` first, then `LOCALAPPDATA`-derived,
    /// then `home_dir`-derived (Linux clink fork).
    #[test]
    fn default_clink_lua_paths_with_includes_explicit_override() {
        use crate::infra::env::EnvHomeDir;
        use std::collections::HashMap;
        let owned: HashMap<String, String> = HashMap::from([
            ("RUNEX_CLINK_LUA_PATH".to_string(), "/explicit/runex.lua".to_string()),
        ]);
        let env = EnvHomeDir::new(move |n| owned.get(n).cloned());
        let paths = default_clink_lua_paths_with(&env);
        assert_eq!(
            paths.first().map(|p| p.as_path()),
            Some(std::path::Path::new("/explicit/runex.lua")),
            "RUNEX_CLINK_LUA_PATH must be the first probed path"
        );
    }

    #[test]
    fn default_clink_lua_paths_with_uses_localappdata_when_set() {
        use crate::infra::env::EnvHomeDir;
        use std::collections::HashMap;
        let owned: HashMap<String, String> = HashMap::from([
            ("LOCALAPPDATA".to_string(), r"C:\Users\test\AppData\Local".to_string()),
        ]);
        let env = EnvHomeDir::new(move |n| owned.get(n).cloned());
        let paths = default_clink_lua_paths_with(&env);
        assert!(
            paths.iter().any(|p| p.ends_with("clink/runex.lua") || p.ends_with(r"clink\runex.lua")),
            "LOCALAPPDATA-derived path missing from {paths:?}"
        );
    }

    #[test]
    fn default_clink_lua_paths_with_includes_home_for_linux_fork() {
        use crate::infra::env::EnvHomeDir;
        use std::collections::HashMap;
        let owned: HashMap<String, String> = HashMap::from([
            ("HOME".to_string(), "/test/home".to_string()),
        ]);
        let env = EnvHomeDir::new(move |n| owned.get(n).cloned());
        let paths = default_clink_lua_paths_with(&env);
        assert!(
            paths.iter().any(|p| p == std::path::Path::new("/test/home/.local/share/clink/runex.lua")),
            "Linux clink fork path missing from {paths:?}"
        );
    }

    #[test]
    fn default_clink_lua_paths_with_empty_resolver_returns_empty() {
        use crate::infra::env::EnvHomeDir;
        use std::collections::HashMap;
        let env = EnvHomeDir::new(|_| -> Option<String> {
            let _: HashMap<String, String> = HashMap::new();
            None
        });
        let paths = default_clink_lua_paths_with(&env);
        assert!(
            paths.is_empty(),
            "no env vars set + no home → no paths probed; got {paths:?}"
        );
    }

    /// Phase G: pin every branch of `check_cache_freshness`.
    mod cache_freshness {
        use super::*;
        use crate::infra::env::EnvHomeDir;
        use crate::infra::integration_cache::{
            cache_header, cache_path, INTEGRATION_CACHE_VERSION,
        };
        use std::collections::HashMap;

        fn env_with(home: &std::path::Path) -> EnvHomeDir<impl Fn(&str) -> Option<String> + Send + Sync> {
            let owned: HashMap<String, String> = HashMap::from([
                ("HOME".to_string(), home.to_string_lossy().into_owned()),
                (
                    "XDG_CACHE_HOME".to_string(),
                    home.join(".cache").to_string_lossy().into_owned(),
                ),
            ]);
            EnvHomeDir::new(move |n| owned.get(n).cloned())
        }

        #[test]
        fn skipped_when_clink() {
            let dir = TempDir::new().unwrap();
            let env = env_with(dir.path());
            let r = check_cache_freshness(Shell::Clink, &env);
            assert!(matches!(r, IntegrationCheck::Skipped { .. }), "got {r:?}");
        }

        #[test]
        fn skipped_when_cache_does_not_exist() {
            let dir = TempDir::new().unwrap();
            let env = env_with(dir.path());
            let r = check_cache_freshness(Shell::Bash, &env);
            assert!(
                matches!(&r, IntegrationCheck::Skipped { detail, .. } if detail.contains("runex init")),
                "got {r:?}"
            );
        }

        #[test]
        fn ok_when_version_matches_and_bin_exists() {
            let dir = TempDir::new().unwrap();
            let env = env_with(dir.path());
            let cache = cache_path(Shell::Bash, &env).unwrap().unwrap();
            std::fs::create_dir_all(cache.parent().unwrap()).unwrap();

            // Use the test binary (the cargo test runner) as a
            // known-existing executable for the runex-bin: header.
            let bin = std::env::current_exe().unwrap();
            let bin_str = bin.to_string_lossy().into_owned();
            let header = cache_header("#", &bin_str);
            std::fs::write(&cache, format!("{header}# body placeholder\n")).unwrap();

            let r = check_cache_freshness(Shell::Bash, &env);
            assert!(matches!(r, IntegrationCheck::Ok { .. }), "got {r:?}");
        }

        #[test]
        fn outdated_when_version_mismatch() {
            let dir = TempDir::new().unwrap();
            let env = env_with(dir.path());
            let cache = cache_path(Shell::Bash, &env).unwrap().unwrap();
            std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
            // Hand-write a header with a future version that this
            // runex doesn't recognise.
            let bin = std::env::current_exe().unwrap();
            let header = format!(
                "# runex-integration-version: {}\n# runex-bin: {}\n# Generated.\n",
                INTEGRATION_CACHE_VERSION + 99,
                bin.display()
            );
            std::fs::write(&cache, format!("{header}# body\n")).unwrap();

            let r = check_cache_freshness(Shell::Bash, &env);
            match r {
                IntegrationCheck::Outdated { detail, .. } => {
                    assert!(detail.contains("version"), "expected version mismatch detail, got {detail}");
                }
                _ => panic!("expected Outdated, got {r:?}"),
            }
        }

        #[test]
        fn outdated_when_legacy_no_header() {
            let dir = TempDir::new().unwrap();
            let env = env_with(dir.path());
            let cache = cache_path(Shell::Bash, &env).unwrap().unwrap();
            std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
            // Simulate legacy `eval "$(runex export bash)"` content
            // — no version header at all.
            std::fs::write(&cache, "# runex shell integration for bash\n__runex_expand() {}\n").unwrap();

            let r = check_cache_freshness(Shell::Bash, &env);
            match r {
                IntegrationCheck::Outdated { detail, .. } => {
                    assert!(detail.contains("legacy") || detail.contains("no `runex-integration-version:` header"),
                        "expected legacy-cache hint, got {detail}");
                }
                _ => panic!("expected Outdated, got {r:?}"),
            }
        }

        #[test]
        fn outdated_when_baked_bin_does_not_exist() {
            let dir = TempDir::new().unwrap();
            let env = env_with(dir.path());
            let cache = cache_path(Shell::Bash, &env).unwrap().unwrap();
            std::fs::create_dir_all(cache.parent().unwrap()).unwrap();

            let header = cache_header("#", "/nonexistent/path/runex");
            std::fs::write(&cache, format!("{header}# body\n")).unwrap();

            let r = check_cache_freshness(Shell::Bash, &env);
            match r {
                IntegrationCheck::Outdated { detail, .. } => {
                    assert!(detail.contains("no longer exists"),
                        "expected missing-bin hint, got {detail}");
                }
                _ => panic!("expected Outdated, got {r:?}"),
            }
        }

        #[test]
        fn ok_when_baked_bin_is_bare_runex() {
            // Bare "runex" is a deliberate power-user override (G3
            // explicit --bin runex). Don't flag it as missing — it's
            // resolved through PATH at run time.
            let dir = TempDir::new().unwrap();
            let env = env_with(dir.path());
            let cache = cache_path(Shell::Bash, &env).unwrap().unwrap();
            std::fs::create_dir_all(cache.parent().unwrap()).unwrap();

            let header = cache_header("#", "runex");
            std::fs::write(&cache, format!("{header}# body\n")).unwrap();

            let r = check_cache_freshness(Shell::Bash, &env);
            assert!(matches!(r, IntegrationCheck::Ok { .. }), "got {r:?}");
        }
    }
}
