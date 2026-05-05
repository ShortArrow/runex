//! Filesystem-backed reads and writes for the user's config file.
//!
//! Phase D D3 split this out of `app/config.rs`. The contract is:
//!
//! - `app/config.rs` holds *parsing* and *validation* (pure, no
//!   I/O — exercised by `parse_config` / `collect_validation_issues`).
//! - This module holds every `std::fs::*` call against
//!   `~/.config/runex/config.toml` (and the `RUNEX_CONFIG` override
//!   path), wrapped behind functions that return `ConfigError` so the
//!   call sites stay typed.
//!
//! The architecture test `no_filesystem_calls_in_app_layer` enforces
//! the split: any `std::fs::write` / `OpenOptions` / `rename` /
//! `remove_file` in `app/` is a regression and will fail CI.
//!
//! ## Why `infra → app::config::ConfigError` is OK
//!
//! `ConfigError` is a *type* defined in `app::config`; this module
//! re-uses it so callers don't have to convert between two error
//! enums. That's a leaf type import, not a behaviour cycle —
//! `infra::config_store` doesn't call any function from `app::config`,
//! it only constructs `ConfigError::Io(_)` / `ConfigError::FileTooLarge`
//! / `ConfigError::NoConfigDir` variants. A future refactor could lift
//! `ConfigError` into `domain/errors.rs` to remove even the type
//! reference, but that's purely cosmetic.

use std::path::{Path, PathBuf};

use crate::app::config::ConfigError;

const MAX_CONFIG_FILE_BYTES: u64 = 10 * 1024 * 1024; // 10 MB

/// Default config file path: `$XDG_CONFIG_HOME/runex/config.toml`,
/// falling back to `~/.config/runex/config.toml` when `XDG_CONFIG_HOME`
/// is unset. All platforms use this same resolution order. Overridden
/// by the `RUNEX_CONFIG` env var.
pub(crate) fn default_config_path() -> Result<PathBuf, ConfigError> {
    if let Ok(p) = std::env::var("RUNEX_CONFIG") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    let dir = crate::infra::env::xdg_config_home_with(&crate::infra::env::SystemHomeDir);
    Ok(dir.ok_or(ConfigError::NoConfigDir)?.join("runex").join("config.toml"))
}

/// Read a config file into a string with the safety guarantees that
/// `app::config::load_config` relies on:
///
/// - Single fd for metadata + read (no TOCTOU between size check and
///   read).
/// - Rejects non-regular files (FIFO / device nodes) which can bypass
///   the size guard by reporting `len() == 0`.
/// - Enforces a 10 MB size cap.
///
/// On Unix, `O_NONBLOCK` prevents `open()` from blocking on a named
/// pipe with no writer.
///
/// ## Symlinks: deliberately allowed
///
/// `~/.config/runex/config.toml` is commonly a symlink into a dotfiles
/// repository (`~/dotfiles/runex/config.toml`). We canonicalise the
/// path before opening so this idiom keeps working — `O_NOFOLLOW` is
/// then applied on the *resolved* path, which means it's effectively a
/// no-op for this code path. The trade-off is documented: an attacker
/// who can already write to the user's config directory can redirect
/// this read to any file. That's a strictly weaker capability than
/// what they already have (writing arbitrary commands into the
/// abbreviation table), so we accept the risk in exchange for keeping
/// the dotfiles pattern frictionless.
pub(crate) fn read_config_source(path: &Path) -> Result<String, ConfigError> {
    use std::io::Read;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        let resolved = path.canonicalize()?;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&resolved)?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "config path must be a regular file",
        )));
    }
    if meta.len() > MAX_CONFIG_FILE_BYTES {
        return Err(ConfigError::FileTooLarge);
    }
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

/// Open a config file for append/write, rejecting symlinks at the
/// final path component on Unix. Prevents an attacker who controls the
/// config directory from redirecting writes to a sensitive file via a
/// swapped symlink.
#[cfg(unix)]
fn open_config_for_append_safely(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_config_for_append_safely(path: &Path) -> std::io::Result<std::fs::File> {
    // Windows has no portable O_NOFOLLOW equivalent at open() time;
    // rely on NTFS permissions at the config dir level.
    std::fs::OpenOptions::new().create(true).append(true).open(path)
}

/// Atomically replace a config file: write to a sibling temp file then
/// rename. On Unix the temp file is created with `O_NOFOLLOW` so a
/// pre-existing symlink at the temp path cannot redirect the write.
fn atomically_write_config(path: &Path, contents: &str) -> Result<(), ConfigError> {
    use std::io::Write;
    let parent = path.parent().ok_or_else(|| {
        ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "config path has no parent directory",
        ))
    })?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            ConfigError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "config path has no file name",
            ))
        })?;
    let tmp = parent.join(format!(".{file_name}.runex.tmp"));

    // Best-effort cleanup of a stale temp file from a previous crash.
    let _ = std::fs::remove_file(&tmp);

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .map_err(ConfigError::Io)?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp)
        .map_err(ConfigError::Io)?;

    file.write_all(contents.as_bytes()).map_err(ConfigError::Io)?;
    file.sync_all().map_err(ConfigError::Io)?;
    drop(file);

    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        ConfigError::Io(e)
    })
}

/// Append a `[[abbr]]` block to `path`, preserving existing content
/// and formatting. The caller is responsible for validating the
/// `key`, `expand`, and `when_command_exists` values — the wrapper in
/// `app::abbr` (Phase D D4) does that before calling here. Rejects
/// symlinks at the final path component on Unix.
pub(crate) fn append_abbr_block(
    path: &Path,
    key: &str,
    expand: &str,
    when_command_exists: Option<&[String]>,
) -> Result<(), ConfigError> {
    let mut block = String::from("\n[[abbr]]\n");
    block.push_str(&format!("key = {}\n", toml_quote(key)));
    block.push_str(&format!("expand = {}\n", toml_quote(expand)));
    if let Some(cmds) = when_command_exists {
        let quoted: Vec<String> = cmds.iter().map(|c| toml_quote(c)).collect();
        block.push_str(&format!("when_command_exists = [{}]\n", quoted.join(", ")));
    }

    use std::io::Write;
    let mut file = open_config_for_append_safely(path).map_err(ConfigError::Io)?;
    file.write_all(block.as_bytes()).map_err(ConfigError::Io)?;
    Ok(())
}

/// Remove all abbreviation rules with `key` from `path`. Uses
/// `toml_edit` to preserve formatting; writes atomically via a
/// sibling temp + rename. Returns the number of rules removed.
pub(crate) fn remove_abbr_block(path: &Path, key: &str) -> Result<usize, ConfigError> {
    let content = read_config_source(path)?;
    let mut doc = content.parse::<toml_edit::DocumentMut>().map_err(|_| {
        ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "failed to parse config as editable TOML",
        ))
    })?;

    let removed = if let Some(toml_edit::Item::ArrayOfTables(arr)) = doc.get_mut("abbr") {
        let before = arr.len();
        let mut i = 0;
        while i < arr.len() {
            let matches = arr
                .get(i)
                .and_then(|t| t.get("key"))
                .and_then(|v| v.as_str())
                .map(|k| k == key)
                .unwrap_or(false);
            if matches {
                arr.remove(i);
            } else {
                i += 1;
            }
        }
        before - arr.len()
    } else {
        0
    };

    if removed > 0 {
        atomically_write_config(path, &doc.to_string())?;
    }
    Ok(removed)
}

/// Quote a string value for TOML output.
fn toml_quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::config::load_config;
    use serial_test::serial;

    /// Filesystem-shape tests that were previously colocated with
    /// `load_config` in `app/config.rs`. Moved to `infra` in Phase D
    /// D3b alongside `read_config_source` itself; the symbol the
    /// test exercises (the size cap, the FIFO rejection, the symlink
    /// follow rules) lives here now.
    #[test]
    fn load_config_rejects_oversized_file() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&vec![b'x'; 11 * 1024 * 1024]).unwrap();
        f.flush().unwrap();
        assert!(
            load_config(f.path()).is_err(),
            "must reject files larger than 10 MB"
        );
    }

    /// On Linux a symlink to /dev/zero reports `metadata().len() == 0`,
    /// bypassing the size guard. `read_config_source` must reject
    /// non-regular files via `is_file()`.
    #[test]
    #[cfg(unix)]
    fn load_config_rejects_symlink_to_dev_zero() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("fake_config.toml");
        std::os::unix::fs::symlink("/dev/zero", &link).unwrap();
        assert!(
            load_config(&link).is_err(),
            "load_config must reject a symlink to /dev/zero"
        );
    }

    /// A symlink pointing to a regular TOML file must be followed —
    /// the dotfiles-symlink idiom relies on it.
    #[test]
    #[cfg(unix)]
    fn load_config_follows_symlink_to_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.toml");
        std::fs::write(&target, b"version = 1\n").unwrap();
        let link = dir.path().join("link_config.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let result = load_config(&link);
        assert!(
            result.is_ok(),
            "load_config must follow a symlink to a regular file: {result:?}"
        );
    }

    /// A named pipe reports `metadata().len() == 0` and
    /// `read_to_string()` blocks. The non-regular file guard rejects
    /// it before any read happens.
    #[test]
    #[cfg(unix)]
    fn load_config_rejects_named_pipe() {
        use std::ffi::CString;
        let dir = tempfile::tempdir().unwrap();
        let pipe = dir.path().join("fake_config.toml");
        let path_c = CString::new(pipe.to_str().unwrap()).unwrap();
        unsafe { libc::mkfifo(path_c.as_ptr(), 0o600) };
        assert!(
            load_config(&pipe).is_err(),
            "load_config must reject a named pipe"
        );
    }

    /// Round-trip: write a config, read it back. Mostly a smoke test
    /// for `read_config_source` + `parse_config` working together
    /// across the layer boundary.
    #[test]
    fn load_config_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
version = 1

[[abbr]]
key = "gcm"
expand = "git commit -m"
"#,
        )
        .unwrap();

        let config = load_config(&path).unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(config.abbr[0].key, "gcm");
    }

    /// Safety: env mutation is serialized via `#[serial]`. Inherited
    /// from the original `app::config` test — the contract being
    /// pinned (RUNEX_CONFIG overrides the XDG path) is unchanged.
    #[test]
    #[serial]
    fn default_config_path_env_override() {
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        unsafe { std::env::set_var("RUNEX_CONFIG", "/tmp/custom.toml") };
        let path = default_config_path().unwrap();
        unsafe { std::env::remove_var("RUNEX_CONFIG") };
        assert_eq!(path, PathBuf::from("/tmp/custom.toml"));
    }

    /// Safety: see `default_config_path_env_override`.
    #[test]
    #[serial]
    fn default_config_path_uses_xdg_config_home() {
        unsafe { std::env::remove_var("RUNEX_CONFIG") };
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-runex-test") };
        let path = default_config_path().unwrap();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        assert_eq!(
            path,
            PathBuf::from("/tmp/xdg-runex-test/runex/config.toml")
        );
    }

    /// Safety: see `default_config_path_env_override`.
    #[test]
    #[serial]
    fn default_config_path_ignores_empty_runex_config() {
        unsafe { std::env::set_var("RUNEX_CONFIG", "") };
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-empty-test") };
        let path = default_config_path().unwrap();
        unsafe { std::env::remove_var("RUNEX_CONFIG") };
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        assert_eq!(
            path,
            PathBuf::from("/tmp/xdg-empty-test/runex/config.toml"),
            "empty RUNEX_CONFIG must fall through to XDG resolution"
        );
    }

    /// Safety: see `default_config_path_env_override`.
    #[test]
    #[serial]
    fn xdg_config_home_with_system_resolver_uses_env_var() {
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-test") };
        let dir = crate::infra::env::xdg_config_home_with(&crate::infra::env::SystemHomeDir).unwrap();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        assert_eq!(dir, PathBuf::from("/tmp/xdg-test"));
    }

    /// Safety: see `default_config_path_env_override`.
    #[test]
    #[serial]
    fn xdg_config_home_with_system_resolver_empty_env_falls_back() {
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "") };
        let dir = crate::infra::env::xdg_config_home_with(&crate::infra::env::SystemHomeDir).unwrap();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        assert!(
            dir.ends_with(".config"),
            "expected ~/.config fallback, got {dir:?}"
        );
    }
}
