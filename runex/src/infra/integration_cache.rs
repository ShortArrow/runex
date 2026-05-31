//! Static shell-integration cache: write the `runex export <shell>`
//! output to a stable on-disk location so the user's rcfile only has
//! to `source` it, paying zero PATH-resolution cost on every shell
//! startup or keystroke.
//!
//! ## Why this exists
//!
//! The previous integration model put `eval "$(runex export bash)"`
//! directly into the user's rcfile. Two consequences:
//!
//! 1. **Per-shell-startup cost**: each interactive shell launches
//!    `runex` synchronously through `$PATH`. On WSL with a `mise`
//!    shim ahead of `~/.cargo/bin`, that lookup hits the shim
//!    binary, which `exec`s the real `runex` after ~470 ms of
//!    `mise` startup. That's once per shell — annoying but
//!    bounded.
//! 2. **Per-keystroke cost**: the generated hook function still
//!    embedded `'runex' hook ...`, so `bind -x` callbacks paid the
//!    same shim overhead on every Space press, producing visible
//!    "prompt blanks for one second" UX breakage.
//!
//! Caching the export output to a file with the absolute path of
//! the producing `runex` baked in eliminates both: rcfile sources
//! a static file (no `$()`-substitution), and the hook function
//! invokes the binary by absolute path (no PATH walk).
//!
//! ## Layering
//!
//! `infra::integration_cache` is the only module that performs file
//! I/O for the cache. `cmd::init` calls into here to write; `app::
//! shell_export` produces the script content (a pure function of
//! `(shell, bin, config)`); `infra::integration_check` reads the
//! cache to verify drift / version mismatches in `runex doctor`.
//!
//! The atomic-write pattern (sibling temp + fsync + rename, with
//! `O_NOFOLLOW` on Unix and a symlink reject everywhere) is the
//! same one used by `cmd::init::write_clink_lua_safely` since
//! 0.1.13. Phase G generalises it so all five shells share the
//! same on-disk safety properties.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::domain::shell::Shell;
use crate::infra::env::{xdg_cache_home_with, HomeDirResolver};

/// Schema version for the cache file format. Bump when the file
/// header layout changes in a way that doctor / future runex
/// versions need to reject. Read by
/// [`crate::infra::integration_check::check_cache_freshness`].
///
/// History
/// -------
/// - `1` (0.1.13–0.1.16): runtime-hook integration; `bind -x` invokes
///   `runex hook` on every keypress.
/// - `2` (0.1.17): bash integration adds the bake-mode dispatcher used
///   by the cygwin/msys path (issue #7 workaround). v1 caches still
///   load on the exec path, so the bump exists to push `runex doctor`
///   into nudging Git Bash users back to `runex init bash` so they
///   pick up the Ctrl+C fix.
pub(crate) const INTEGRATION_CACHE_VERSION: u32 = 2;

/// Marker token that appears in the cache header so doctor can
/// re-identify a runex-managed file even if the user has renamed
/// the file or the comment syntax differs across shells.
pub(crate) const INTEGRATION_CACHE_MARKER: &str = "runex-integration-version:";

#[derive(Debug, thiserror::Error)]
pub(crate) enum CacheError {
    #[error("cannot resolve cache directory (set $XDG_CACHE_HOME or $HOME)")]
    NoCacheDir,
    #[error("refusing to write through a symlink at {path}")]
    SymlinkAtTarget { path: PathBuf },
    #[error("cache path has no parent directory: {path}")]
    NoParent { path: PathBuf },
    #[error("cache path has no file name: {path}")]
    NoFileName { path: PathBuf },
    #[error("OS error writing cache at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Resolve the on-disk path for `shell`'s integration cache file.
///
/// Layout: `<XDG_CACHE_HOME or fallback>/runex/integration.<ext>`.
/// Extensions follow each shell's conventional source-file suffix
/// (`.bash`, `.zsh`, `.ps1`, `.nu`). Clink is **not** part of this
/// path scheme: clink loads its lua autoloader from a fixed
/// platform-specific location (`%LOCALAPPDATA%\clink\runex.lua`)
/// that is resolved by [`crate::app::init::clink_lua_install_path_with_resolver`]
/// instead. Returning `None` for `Shell::Clink` here keeps the
/// existing clink install path untouched while letting the same
/// `cache_path` API serve all four cache-eligible shells.
pub(crate) fn cache_path(
    shell: Shell,
    env: &dyn HomeDirResolver,
) -> Result<Option<PathBuf>, CacheError> {
    let ext = match shell {
        Shell::Bash => "bash",
        Shell::Zsh => "zsh",
        Shell::Pwsh => "ps1",
        Shell::Nu => "nu",
        // Clink uses a fixed install path under %LOCALAPPDATA%\clink\;
        // the cache layout doesn't apply.
        Shell::Clink => return Ok(None),
    };
    let cache_root = xdg_cache_home_with(env).ok_or(CacheError::NoCacheDir)?;
    Ok(Some(cache_root.join("runex").join(format!("integration.{ext}"))))
}

/// Atomically write `contents` to `path`. Uses a sibling
/// `.<name>.runex.tmp` file then renames over the target so a
/// crash mid-write leaves the previous version in place.
///
/// Refuses to follow a symlink at `path` (Phase D security
/// posture: an attacker who can place a symlink in the cache dir
/// must not be able to redirect writes elsewhere).
pub(crate) fn write_cache_file(path: &Path, contents: &str) -> Result<(), CacheError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| CacheError::NoParent { path: path.to_path_buf() })?;
    std::fs::create_dir_all(parent).map_err(|e| CacheError::Io {
        path: parent.to_path_buf(),
        source: e,
    })?;

    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(CacheError::SymlinkAtTarget {
                path: path.to_path_buf(),
            });
        }
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| CacheError::NoFileName { path: path.to_path_buf() })?;
    let tmp_path = parent.join(format!(".{file_name}.runex.tmp"));
    // Best-effort cleanup of a stale temp from a previous crash.
    let _ = std::fs::remove_file(&tmp_path);

    let mut tmp_opts = std::fs::OpenOptions::new();
    tmp_opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        tmp_opts.custom_flags(libc::O_NOFOLLOW);
    }
    let mut tmp_file = tmp_opts.open(&tmp_path).map_err(|e| CacheError::Io {
        path: tmp_path.clone(),
        source: e,
    })?;
    tmp_file
        .write_all(contents.as_bytes())
        .map_err(|e| CacheError::Io {
            path: tmp_path.clone(),
            source: e,
        })?;
    tmp_file.sync_all().map_err(|e| CacheError::Io {
        path: tmp_path.clone(),
        source: e,
    })?;
    drop(tmp_file);

    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(CacheError::Io {
            path: path.to_path_buf(),
            source: e,
        });
    }
    Ok(())
}

/// Generate the standard cache header that prefixes every script.
/// `bin` is the absolute path of the producing runex binary
/// (typically `current_exe()`).
///
/// `comment_prefix` is shell-specific:
/// * `#` for bash / zsh / nu
/// * `#` is also valid for pwsh as a comment, but powershell idiom
///   prefers `#` here for consistency since the header is parsed
///   by line, not by AST.
pub(crate) fn cache_header(comment_prefix: &str, bin: &str) -> String {
    format!(
        "{cp} {marker} {ver}\n\
         {cp} runex-bin: {bin}\n\
         {cp} Generated by `runex init <shell>`; do not edit.\n",
        cp = comment_prefix,
        marker = INTEGRATION_CACHE_MARKER,
        ver = INTEGRATION_CACHE_VERSION,
        bin = bin
    )
}

/// Pick the comment-prefix string for a given shell. Used both when
/// writing a cache file and when scanning one in
/// `infra::integration_check`.
pub(crate) fn comment_prefix_for(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash | Shell::Zsh | Shell::Pwsh | Shell::Nu => "#",
        // Clink uses lua's `--` but is excluded from the cache layout.
        Shell::Clink => "--",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::env::EnvHomeDir;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn env_with(map: HashMap<&'static str, String>) -> EnvHomeDir<impl Fn(&str) -> Option<String> + Send + Sync> {
        let owned: HashMap<String, String> = map.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        EnvHomeDir::new(move |n| owned.get(n).cloned())
    }

    #[test]
    fn cache_path_returns_none_for_clink() {
        let env = env_with(HashMap::from([("HOME", "/test/home".to_string())]));
        assert_eq!(cache_path(Shell::Clink, &env).unwrap(), None);
    }

    #[test]
    fn cache_path_uses_xdg_cache_home_when_set() {
        let env = env_with(HashMap::from([
            ("XDG_CACHE_HOME", "/explicit/cache".to_string()),
            ("HOME", "/test/home".to_string()),
        ]));
        let p = cache_path(Shell::Bash, &env).unwrap().unwrap();
        assert_eq!(p, PathBuf::from("/explicit/cache/runex/integration.bash"));
    }

    #[test]
    #[cfg(not(windows))]
    fn cache_path_falls_back_to_home_dotcache_on_unix() {
        let env = env_with(HashMap::from([("HOME", "/test/home".to_string())]));
        for (shell, ext) in [(Shell::Bash, "bash"), (Shell::Zsh, "zsh"), (Shell::Pwsh, "ps1"), (Shell::Nu, "nu")] {
            let p = cache_path(shell, &env).unwrap().unwrap();
            assert_eq!(p, PathBuf::from(format!("/test/home/.cache/runex/integration.{ext}")));
        }
    }

    #[test]
    fn cache_path_returns_no_cache_dir_when_no_signal() {
        let env = env_with(HashMap::new());
        assert!(matches!(
            cache_path(Shell::Bash, &env),
            Err(CacheError::NoCacheDir)
        ));
    }

    #[test]
    fn write_cache_file_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("nested/deep/integration.bash");
        write_cache_file(&target, "hello").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");
    }

    #[test]
    fn write_cache_file_replaces_existing_atomically() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("integration.bash");
        std::fs::write(&target, "old contents").unwrap();
        write_cache_file(&target, "new contents").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new contents");
        // No leftover temp file
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "atomic rename must not leave a sibling temp behind: {entries:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn write_cache_file_rejects_symlink_target() {
        let dir = tempdir().unwrap();
        let real = dir.path().join("real.txt");
        std::fs::write(&real, "real").unwrap();
        let link = dir.path().join("integration.bash");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let err = write_cache_file(&link, "hijack attempt").unwrap_err();
        assert!(matches!(err, CacheError::SymlinkAtTarget { .. }));
        // The symlink target is unchanged
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "real");
    }

    #[test]
    fn cache_header_contains_required_fields() {
        let h = cache_header("#", "/abs/path/to/runex");
        let expected_version = format!("runex-integration-version: {INTEGRATION_CACHE_VERSION}");
        assert!(h.contains(&expected_version), "header missing version field: {h}");
        assert!(h.contains("runex-bin: /abs/path/to/runex"));
        assert!(h.contains("do not edit"));
    }

    #[test]
    fn comment_prefix_pwsh_is_hash() {
        assert_eq!(comment_prefix_for(Shell::Pwsh), "#");
        assert_eq!(comment_prefix_for(Shell::Bash), "#");
        assert_eq!(comment_prefix_for(Shell::Clink), "--");
    }
}
