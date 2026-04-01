use std::path::PathBuf;

use crate::model::Config;

const MAX_CONFIG_FILE_BYTES: u64 = 10 * 1024 * 1024; // 10 MB
const MAX_ABBR_RULES: usize = 10_000;
const MAX_KEY_BYTES: usize = 1_024;
const MAX_EXPAND_BYTES: usize = 4_096;
const MAX_CMD_BYTES: usize = 255;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot determine config directory")]
    NoConfigDir,
    #[error("config file exceeds maximum size of 10 MB")]
    FileTooLarge,
    #[error("config has too many abbr rules (max {MAX_ABBR_RULES})")]
    TooManyRules,
    #[error("abbr rule #{0}: key exceeds maximum length of {MAX_KEY_BYTES} bytes")]
    KeyTooLong(usize),
    #[error("abbr rule #{0}: expand exceeds maximum length of {MAX_EXPAND_BYTES} bytes")]
    ExpandTooLong(usize),
    #[error("abbr rule #{0}: key contains a NUL byte")]
    KeyContainsNul(usize),
    #[error("abbr rule #{0}: expand contains a NUL byte")]
    ExpandContainsNul(usize),
    #[error("abbr rule #{0}: when_command_exists entry exceeds maximum length of {MAX_CMD_BYTES} bytes")]
    CmdTooLong(usize),
    #[error("abbr rule #{0}: when_command_exists entry contains a NUL byte")]
    CmdContainsNul(usize),
}

/// Parse a TOML string into Config.
pub fn parse_config(s: &str) -> Result<Config, ConfigError> {
    let config: Config = toml::from_str(s)?;
    if config.abbr.len() > MAX_ABBR_RULES {
        return Err(ConfigError::TooManyRules);
    }
    for (i, abbr) in config.abbr.iter().enumerate() {
        let n = i + 1;
        if abbr.key.len() > MAX_KEY_BYTES {
            return Err(ConfigError::KeyTooLong(n));
        }
        if abbr.expand.len() > MAX_EXPAND_BYTES {
            return Err(ConfigError::ExpandTooLong(n));
        }
        if abbr.key.contains('\0') {
            return Err(ConfigError::KeyContainsNul(n));
        }
        if abbr.expand.contains('\0') {
            return Err(ConfigError::ExpandContainsNul(n));
        }
        if let Some(cmds) = &abbr.when_command_exists {
            for cmd in cmds {
                if cmd.len() > MAX_CMD_BYTES {
                    return Err(ConfigError::CmdTooLong(n));
                }
                if cmd.contains('\0') {
                    return Err(ConfigError::CmdContainsNul(n));
                }
            }
        }
    }
    Ok(config)
}

/// Default config file path: `$XDG_CONFIG_HOME/runex/config.toml`,
/// falling back to `~/.config/runex/config.toml` when `XDG_CONFIG_HOME` is unset.
/// All platforms use this same resolution order.
/// Overridden by `RUNEX_CONFIG` env var.
pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    if let Ok(p) = std::env::var("RUNEX_CONFIG") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    let dir = xdg_config_home();
    Ok(dir.ok_or(ConfigError::NoConfigDir)?.join("runex").join("config.toml"))
}

/// Resolve `$XDG_CONFIG_HOME`, falling back to `~/.config`.
pub(crate) fn xdg_config_home() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("XDG_CONFIG_HOME") {
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    dirs::home_dir().map(|h| h.join(".config"))
}

/// Load config from a file path.
///
/// Opens the file once and uses the same file descriptor for both the size check
/// and the read, eliminating the TOCTOU race that exists when `metadata()` and
/// `read_to_string()` open the file separately.
pub fn load_config(path: &std::path::Path) -> Result<Config, ConfigError> {
    use std::io::Read;
    // On Unix, open with O_NOFOLLOW to refuse symlinks at the final path component.
    // This prevents an attacker from racing to replace config.toml with a symlink
    // to any readable regular file (e.g. /etc/passwd), not just device nodes.
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    // Reject non-regular files (symlinks to /dev/zero, named pipes, device nodes).
    // These can bypass the size guard (reporting len=0) or block indefinitely on read.
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
    parse_config(&content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TriggerKey;
    use serial_test::serial;

    #[test]
    fn parse_minimal_toml() {
        let toml = r#"
version = 1

[[abbr]]
key = "gcm"
expand = "git commit -m"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(config.abbr.len(), 1);
        assert_eq!(config.abbr[0].key, "gcm");
        assert_eq!(config.abbr[0].expand, "git commit -m");
    }

    #[test]
    fn parse_with_when_command_exists() {
        let toml = r#"
version = 1

[[abbr]]
key = "ls"
expand = "lsd"
when_command_exists = ["lsd"]
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(
            config.abbr[0].when_command_exists,
            Some(vec!["lsd".to_string()])
        );
    }

    #[test]
    fn parse_with_keybind() {
        let toml = r#"
version = 1

[keybind]
trigger = "space"
bash = "alt-space"
zsh = "space"
pwsh = "tab"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.keybind.trigger, Some(TriggerKey::Space));
        assert_eq!(config.keybind.bash, Some(TriggerKey::AltSpace));
        assert_eq!(config.keybind.zsh, Some(TriggerKey::Space));
        assert_eq!(config.keybind.pwsh, Some(TriggerKey::Tab));
        assert_eq!(config.keybind.nu, None);
    }

    #[test]
    fn parse_missing_version_is_err() {
        let toml = r#"
[[abbr]]
key = "gcm"
expand = "git commit -m"
"#;
        assert!(parse_config(toml).is_err());
    }

    #[test]
    fn parse_empty_abbr_list() {
        let toml = "version = 1\n";
        let config = parse_config(toml).unwrap();
        assert!(config.abbr.is_empty());
    }

    #[test]
    fn load_config_from_file() {
        let dir = std::env::temp_dir().join("runex_test_load");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
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

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Safety: env mutation is serialized via `#[serial]`; no concurrent
    /// env access within this test suite. External concurrent access is
    /// not fully excluded but acceptable in test context.
    #[test]
    #[serial]
    fn default_config_path_env_override() {
        // RUNEX_CONFIG takes priority over XDG_CONFIG_HOME.
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        unsafe { std::env::set_var("RUNEX_CONFIG", "/tmp/custom.toml") };
        let path = default_config_path().unwrap();
        unsafe { std::env::remove_var("RUNEX_CONFIG") };
        assert_eq!(path, PathBuf::from("/tmp/custom.toml"));
    }

    /// Safety: see `default_config_path_env_override`.
    #[test]
    #[serial]
    fn xdg_config_home_uses_env_var() {
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-test") };
        let dir = xdg_config_home().unwrap();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        assert_eq!(dir, PathBuf::from("/tmp/xdg-test"));
    }

    /// Safety: see `default_config_path_env_override`.
    #[test]
    #[serial]
    fn xdg_config_home_empty_env_falls_back_to_home() {
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "") };
        let dir = xdg_config_home().unwrap();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        // Falls back to home/.config — must end with .config
        assert!(dir.ends_with(".config"), "expected ~/.config fallback, got {dir:?}");
    }

    /// Safety: see `default_config_path_env_override`.
    #[test]
    #[serial]
    fn default_config_path_uses_xdg_config_home() {
        unsafe { std::env::remove_var("RUNEX_CONFIG") };
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-runex-test") };
        let path = default_config_path().unwrap();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        assert_eq!(path, PathBuf::from("/tmp/xdg-runex-test/runex/config.toml"));
    }

    /// Safety: see `default_config_path_env_override`.
    #[test]
    #[serial]
    fn default_config_path_ignores_empty_runex_config() {
        // RUNEX_CONFIG="" must be treated as unset, falling through to XDG resolution.
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

    #[test]
    fn parse_config_rejects_too_many_abbr() {
        let mut s = String::from("version = 1\n");
        for i in 0..10_001 {
            s.push_str(&format!("[[abbr]]\nkey = \"k{i}\"\nexpand = \"v{i}\"\n"));
        }
        assert!(parse_config(&s).is_err(), "must reject configs with more than 10,000 abbr rules");
    }

    #[test]
    fn parse_config_accepts_max_abbr() {
        let mut s = String::from("version = 1\n");
        for i in 0..10_000 {
            s.push_str(&format!("[[abbr]]\nkey = \"k{i}\"\nexpand = \"v{i}\"\n"));
        }
        assert!(parse_config(&s).is_ok(), "must accept exactly 10,000 abbr rules");
    }

    #[test]
    fn load_config_rejects_oversized_file() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        // Write 11 MB of data (above 10 MB limit)
        f.write_all(&vec![b'x'; 11 * 1024 * 1024]).unwrap();
        f.flush().unwrap();
        assert!(load_config(f.path()).is_err(), "must reject files larger than 10 MB");
    }

    #[test]
    #[cfg(unix)]
    fn load_config_rejects_symlink_to_dev_zero() {
        // On Linux, a symlink to /dev/zero reports metadata().len() == 0, bypassing the
        // size guard. load_config must reject non-regular files.
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("fake_config.toml");
        std::os::unix::fs::symlink("/dev/zero", &link).unwrap();
        let err = load_config(&link);
        assert!(err.is_err(), "load_config must reject a symlink to /dev/zero");
    }

    #[test]
    #[cfg(unix)]
    fn load_config_rejects_symlink_to_regular_file() {
        // A symlink pointing to a regular file (e.g. /etc/passwd) must be rejected.
        // The O_NOFOLLOW protection must prevent following the symlink.
        let dir = tempfile::tempdir().unwrap();
        // Create a real regular file to link to (avoid depending on /etc/passwd existing)
        let target = dir.path().join("target.toml");
        std::fs::write(&target, b"version = 1\n").unwrap();
        let link = dir.path().join("link_config.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let err = load_config(&link);
        assert!(err.is_err(), "load_config must reject a symlink even to a regular file (O_NOFOLLOW)");
    }

    #[test]
    #[cfg(unix)]
    fn load_config_rejects_named_pipe() {
        // A named pipe reports metadata().len() == 0 and read_to_string() blocks.
        // load_config must reject non-regular files before attempting to read.
        use std::ffi::CString;
        let dir = tempfile::tempdir().unwrap();
        let pipe = dir.path().join("fake_config.toml");
        let path_c = CString::new(pipe.to_str().unwrap()).unwrap();
        unsafe { libc::mkfifo(path_c.as_ptr(), 0o600) };
        let err = load_config(&pipe);
        assert!(err.is_err(), "load_config must reject a named pipe");
    }

    /// Canary: version=99 currently passes validation.
    /// Update this test when explicit version validation is added.
    #[test]
    fn parse_version_99_currently_passes() {
        let toml = "version = 99\n";
        assert!(
            parse_config(toml).is_ok(),
            "version=99 currently passes — update this test when version validation is added"
        );
    }

    // ─── per-field validation ─────────────────────────────────────────────────

    #[test]
    fn parse_config_rejects_oversized_key() {
        // A single key exceeding 1024 bytes must be rejected.
        let huge_key = "k".repeat(1025);
        let toml = format!("version = 1\n[[abbr]]\nkey = \"{huge_key}\"\nexpand = \"v\"\n");
        assert!(parse_config(&toml).is_err(), "must reject key longer than 1024 bytes");
    }

    #[test]
    fn parse_config_accepts_max_key_length() {
        let max_key = "k".repeat(1024);
        let toml = format!("version = 1\n[[abbr]]\nkey = \"{max_key}\"\nexpand = \"v\"\n");
        assert!(parse_config(&toml).is_ok(), "must accept key of exactly 1024 bytes");
    }

    #[test]
    fn parse_config_rejects_oversized_expand() {
        // A single expand value exceeding 4096 bytes must be rejected.
        let huge_expand = "x".repeat(4097);
        let toml = format!("version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"{huge_expand}\"\n");
        assert!(parse_config(&toml).is_err(), "must reject expand longer than 4096 bytes");
    }

    #[test]
    fn parse_config_accepts_max_expand_length() {
        let max_expand = "x".repeat(4096);
        let toml = format!("version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"{max_expand}\"\n");
        assert!(parse_config(&toml).is_ok(), "must accept expand of exactly 4096 bytes");
    }

    #[test]
    fn parse_config_rejects_oversized_when_command_exists_entry() {
        let huge_cmd = "c".repeat(256);
        let toml = format!(
            "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [\"{huge_cmd}\"]\n"
        );
        assert!(parse_config(&toml).is_err(), "must reject when_command_exists entry longer than 255 bytes");
    }

    #[test]
    fn parse_config_accepts_max_when_command_exists_entry() {
        let max_cmd = "c".repeat(255);
        let toml = format!(
            "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [\"{max_cmd}\"]\n"
        );
        assert!(parse_config(&toml).is_ok(), "must accept when_command_exists entry of exactly 255 bytes");
    }

    #[test]
    fn parse_config_rejects_nul_byte_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [\"cmd\\u0000evil\"]\n";
        assert!(parse_config(toml).is_err(), "must reject when_command_exists entry containing NUL byte");
    }

    #[test]
    fn parse_config_rejects_nul_byte_in_key() {
        // TOML \u0000 in a string key must be rejected as an invalid field value.
        let toml = "version = 1\n[[abbr]]\nkey = \"k\\u0000evil\"\nexpand = \"v\"\n";
        assert!(parse_config(toml).is_err(), "must reject key containing NUL byte");
    }

    #[test]
    fn parse_config_rejects_nul_byte_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\\u0000evil\"\n";
        assert!(parse_config(toml).is_err(), "must reject expand containing NUL byte");
    }
}
