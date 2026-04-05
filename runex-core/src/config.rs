use std::path::PathBuf;

use crate::model::Config;
use crate::sanitize::is_deceptive_unicode;

const MAX_CONFIG_FILE_BYTES: u64 = 10 * 1024 * 1024; // 10 MB
const MAX_ABBR_RULES: usize = 10_000;
const MAX_KEY_BYTES: usize = 1_024;
const MAX_EXPAND_BYTES: usize = 4_096;
const MAX_CMD_BYTES: usize = 255;
const MAX_CMD_LIST_LEN: usize = 64;

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
    #[error("abbr rule #{0}: when_command_exists entry contains an ASCII control character (use printable characters only)")]
    CmdContainsControlChar(usize),
    #[error("abbr rule #{0}: key contains an ASCII control character (use printable characters only)")]
    KeyContainsControlChar(usize),
    #[error("abbr rule #{0}: expand contains an ASCII control character (use printable characters only)")]
    ExpandContainsControlChar(usize),
    #[error("abbr rule #{0}: key is empty (an empty key can never match anything)")]
    KeyEmpty(usize),
    #[error("abbr rule #{0}: key contains only whitespace (a whitespace-only key can never match)")]
    KeyWhitespaceOnly(usize),
    #[error("abbr rule #{0}: when_command_exists entry is empty (an empty command name can never be found)")]
    CmdEmpty(usize),
    #[error("abbr rule #{0}: when_command_exists entry contains only whitespace (a whitespace-only command name can never be found)")]
    CmdWhitespaceOnly(usize),
    #[error("abbr rule #{0}: key contains a Unicode visual-deception character (invisible/directional char that makes the key unmatchable or misleading)")]
    KeyContainsDeceptiveUnicode(usize),
    #[error("abbr rule #{0}: expand contains a Unicode visual-deception character (invisible/directional char that makes the expansion misleading)")]
    ExpandContainsDeceptiveUnicode(usize),
    #[error("abbr rule #{0}: when_command_exists entry contains a Unicode visual-deception character")]
    CmdContainsDeceptiveUnicode(usize),
    #[error("abbr rule #{0}: when_command_exists entry contains a path separator ('/', '\\\\', or ':'); only bare command names are allowed")]
    CmdContainsPathSeparator(usize),
    #[error("abbr rule #{0}: when_command_exists has too many entries (max {MAX_CMD_LIST_LEN})")]
    TooManyCmds(usize),
    #[error("unsupported config version {0}; only version 1 is supported")]
    UnsupportedVersion(u32),
    #[error("abbr rule #{0}: expand is empty (an empty expansion would silently delete the typed token)")]
    ExpandEmpty(usize),
    #[error("abbr rule #{0}: expand contains only whitespace (a whitespace-only expansion is almost certainly a config mistake)")]
    ExpandWhitespaceOnly(usize),
}

/// Validate the `key` field of an abbreviation rule.
///
/// Rejects keys that are empty, whitespace-only, or exceed [`MAX_KEY_BYTES`].
/// Also rejects keys containing NUL bytes, ASCII control characters, or Unicode
/// visual-deception characters — all of which would make the key unmatchable or
/// cause it to display differently from its actual byte content.
fn validate_abbr_key(key: &str, n: usize) -> Result<(), ConfigError> {
    if key.is_empty() {
        return Err(ConfigError::KeyEmpty(n));
    }
    if key.trim().is_empty() {
        return Err(ConfigError::KeyWhitespaceOnly(n));
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(ConfigError::KeyTooLong(n));
    }
    if key.contains('\0') {
        return Err(ConfigError::KeyContainsNul(n));
    }
    if key.chars().any(|c| c.is_ascii_control()) {
        return Err(ConfigError::KeyContainsControlChar(n));
    }
    if key.chars().any(is_deceptive_unicode) {
        return Err(ConfigError::KeyContainsDeceptiveUnicode(n));
    }
    Ok(())
}

/// Validate the `expand` field of an abbreviation rule.
///
/// Rejects values that are empty, whitespace-only, or exceed [`MAX_EXPAND_BYTES`].
/// Also rejects values containing NUL bytes, ASCII control characters, or Unicode
/// visual-deception characters — all of which would corrupt the expanded output.
fn validate_abbr_expand(expand: &str, n: usize) -> Result<(), ConfigError> {
    if expand.is_empty() {
        return Err(ConfigError::ExpandEmpty(n));
    }
    if expand.trim().is_empty() {
        return Err(ConfigError::ExpandWhitespaceOnly(n));
    }
    if expand.len() > MAX_EXPAND_BYTES {
        return Err(ConfigError::ExpandTooLong(n));
    }
    if expand.contains('\0') {
        return Err(ConfigError::ExpandContainsNul(n));
    }
    if expand.chars().any(|c| c.is_ascii_control()) {
        return Err(ConfigError::ExpandContainsControlChar(n));
    }
    if expand.chars().any(is_deceptive_unicode) {
        return Err(ConfigError::ExpandContainsDeceptiveUnicode(n));
    }
    Ok(())
}

/// Validate a single `when_command_exists` entry.
///
/// Rejects entries that are empty, whitespace-only, or exceed [`MAX_CMD_BYTES`].
/// Also rejects entries containing NUL bytes, ASCII control characters, Unicode
/// visual-deception characters, or path separators (`/`, `\`, `:`).
/// Only bare command names are allowed — filesystem paths would bypass the intent
/// of checking only within `path_prepend`.
fn validate_cmd_entry(cmd: &str, n: usize) -> Result<(), ConfigError> {
    if cmd.is_empty() {
        return Err(ConfigError::CmdEmpty(n));
    }
    if cmd.trim().is_empty() {
        return Err(ConfigError::CmdWhitespaceOnly(n));
    }
    if cmd.len() > MAX_CMD_BYTES {
        return Err(ConfigError::CmdTooLong(n));
    }
    if cmd.contains('\0') {
        return Err(ConfigError::CmdContainsNul(n));
    }
    if cmd.chars().any(|c| c.is_ascii_control()) {
        return Err(ConfigError::CmdContainsControlChar(n));
    }
    if cmd.chars().any(is_deceptive_unicode) {
        return Err(ConfigError::CmdContainsDeceptiveUnicode(n));
    }
    if cmd.contains('/') || cmd.contains('\\') || cmd.contains(':') {
        return Err(ConfigError::CmdContainsPathSeparator(n));
    }
    Ok(())
}

/// Parse a TOML string into a [`Config`].
///
/// Only version 1 is accepted. Validates all abbreviation rules via
/// [`validate_abbr_key`], [`validate_abbr_expand`], and [`validate_cmd_entry`].
pub fn parse_config(s: &str) -> Result<Config, ConfigError> {
    let config: Config = toml::from_str(s)?;
    if config.version != 1 {
        return Err(ConfigError::UnsupportedVersion(config.version));
    }
    if config.abbr.len() > MAX_ABBR_RULES {
        return Err(ConfigError::TooManyRules);
    }
    for (i, abbr) in config.abbr.iter().enumerate() {
        let n = i + 1;
        validate_abbr_key(&abbr.key, n)?;
        validate_abbr_expand(&abbr.expand, n)?;
        if let Some(cmds) = &abbr.when_command_exists {
            if cmds.len() > MAX_CMD_LIST_LEN {
                return Err(ConfigError::TooManyCmds(n));
            }
            for cmd in cmds {
                validate_cmd_entry(cmd, n)?;
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
///
/// On Unix, `O_NOFOLLOW` rejects symlinks at the final path component, and `O_NONBLOCK`
/// prevents `open()` from blocking on a named pipe with no writer. Non-regular files
/// (device nodes, FIFOs) can bypass the size guard by reporting `len() == 0`, so they
/// are rejected via `is_file()` immediately after open.
pub fn load_config(path: &std::path::Path) -> Result<Config, ConfigError> {
    use std::io::Read;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?
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
    parse_config(&content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TriggerKey;
    use serial_test::serial;

    mod parsing {
        use super::*;

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

    /// TOML allows any string for `trigger`, but only known variants are valid.
    /// An unknown value must be rejected so the user gets an explicit error rather than
    /// silently falling back to a default they didn't request.
    #[test]
    fn parse_config_rejects_invalid_trigger_key() {
        let toml = "version = 1\n[keybind]\ntrigger = \"invalid-key\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject unknown trigger key value 'invalid-key'"
        );
    }

    #[test]
    fn parse_config_rejects_invalid_per_shell_keybind() {
        for field in ["bash", "zsh", "pwsh", "nu"] {
            let toml = format!("version = 1\n[keybind]\n{field} = \"unknown-keybind\"\n");
            assert!(
                parse_config(&toml).is_err(),
                "must reject unknown keybind value for field '{field}'"
            );
        }
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
        f.write_all(&vec![b'x'; 11 * 1024 * 1024]).unwrap();
        f.flush().unwrap();
        assert!(load_config(f.path()).is_err(), "must reject files larger than 10 MB");
    }

    /// On Linux, a symlink to /dev/zero reports metadata().len() == 0, bypassing the
    /// size guard. load_config must reject non-regular files.
    #[test]
    #[cfg(unix)]
    fn load_config_rejects_symlink_to_dev_zero() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("fake_config.toml");
        std::os::unix::fs::symlink("/dev/zero", &link).unwrap();
        let err = load_config(&link);
        assert!(err.is_err(), "load_config must reject a symlink to /dev/zero");
    }

    /// A symlink pointing to a regular file (e.g. /etc/passwd) must be rejected.
    /// The O_NOFOLLOW protection must prevent following the symlink.
    #[test]
    #[cfg(unix)]
    fn load_config_rejects_symlink_to_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.toml");
        std::fs::write(&target, b"version = 1\n").unwrap();
        let link = dir.path().join("link_config.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let err = load_config(&link);
        assert!(err.is_err(), "load_config must reject a symlink even to a regular file (O_NOFOLLOW)");
    }

    /// A named pipe reports metadata().len() == 0 and read_to_string() blocks.
    /// load_config must reject non-regular files before attempting to read.
    #[test]
    #[cfg(unix)]
    fn load_config_rejects_named_pipe() {
        use std::ffi::CString;
        let dir = tempfile::tempdir().unwrap();
        let pipe = dir.path().join("fake_config.toml");
        let path_c = CString::new(pipe.to_str().unwrap()).unwrap();
        unsafe { libc::mkfifo(path_c.as_ptr(), 0o600) };
        let err = load_config(&pipe);
        assert!(err.is_err(), "load_config must reject a named pipe");
    }

    } // mod parsing

    mod field_validation {
        use super::*;

    #[test]
    fn parse_config_rejects_oversized_key() {
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
        let toml = "version = 1\n[[abbr]]\nkey = \"k\\u0000evil\"\nexpand = \"v\"\n";
        assert!(parse_config(toml).is_err(), "must reject key containing NUL byte");
    }

    #[test]
    fn parse_config_rejects_nul_byte_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\\u0000evil\"\n";
        assert!(parse_config(toml).is_err(), "must reject expand containing NUL byte");
    }

    } // mod field_validation

    /// TOML allows `\uXXXX` escapes for any Unicode code point, including ASCII
    /// control characters (U+0001–U+001F, U+007F). These pass through `toml::from_str`
    /// but must be rejected by `parse_config` because:
    /// - key: quoting functions silently drop them, making the key unmatchable
    /// - expand: the expansion is silently mangled when printed
    /// - both: users get silent wrong behavior instead of a clear error
    mod control_char_rejection {
        use super::*;

    #[test]
    fn parse_config_rejects_control_char_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\\u001Bevil\"\nexpand = \"v\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject key containing ASCII control char (\\u001B)"
        );
    }

    #[test]
    fn parse_config_rejects_control_char_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\\u001Bevil\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject expand containing ASCII control char (\\u001B)"
        );
    }

    #[test]
    fn parse_config_rejects_del_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\\u007Fevil\"\nexpand = \"v\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject key containing DEL (\\u007F)"
        );
    }

    #[test]
    fn parse_config_rejects_del_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\\u007Fevil\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject expand containing DEL (\\u007F)"
        );
    }

    #[test]
    fn parse_config_accepts_key_without_control_chars() {
        let toml = "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n";
        assert!(parse_config(toml).is_ok(), "must accept key without control chars");
    }

    /// An empty key produces `''` in bash/zsh case statements, which matches
    /// the empty string — any empty-token expansion would silently fire.
    /// Reject early with a clear error rather than producing a broken script.
    #[test]
    fn parse_config_rejects_empty_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"\"\nexpand = \"git commit -m\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject an abbr rule with an empty key"
        );
    }

    /// A key consisting only of spaces would be silently dropped by quoting functions,
    /// making the rule unmatchable while appearing valid.
    #[test]
    fn parse_config_rejects_whitespace_only_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"   \"\nexpand = \"git commit -m\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject an abbr rule with a whitespace-only key"
        );
    }

    /// An empty string in `when_command_exists` is meaningless: `which::which("")` always
    /// fails, silently causing the rule to never expand.
    #[test]
    fn parse_config_rejects_empty_when_command_exists_entry() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry that is an empty string"
        );
    }

    /// A whitespace-only command name silently makes the rule permanently inactive.
    #[test]
    fn parse_config_rejects_whitespace_only_when_command_exists_entry() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"   \"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry that is whitespace-only"
        );
    }

    #[test]
    fn parse_config_rejects_control_char_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [\"cmd\\u001Bevil\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry containing ASCII control char (\\u001B)"
        );
    }

    #[test]
    fn parse_config_rejects_del_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [\"cmd\\u007Fevil\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry containing DEL (\\u007F)"
        );
    }

    } // mod control_char_rejection

    /// Characters such as U+FEFF (BOM/zero-width no-break space), U+202E (Right-to-Left
    /// Override), and other Unicode formatting/invisible characters cannot be seen in most
    /// terminals and text editors. If embedded in `key`, `expand`, or `when_command_exists`,
    /// they cause:
    /// - `key`: rule appears valid but never matches (invisible difference from real command)
    /// - `expand`: expansion contains invisible/deceptive text printed to terminal
    /// - `when_command_exists`: command lookup silently fails forever
    /// - `list` output: shows a key that looks like "ls" but is really `"\u{FEFF}ls"`
    ///
    /// These must be rejected early with a clear error.
    mod deceptive_unicode {
        use super::*;

    #[test]
    fn parse_config_rejects_bom_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"\\uFEFFls\"\nexpand = \"lsd\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject key containing U+FEFF (BOM / zero-width no-break space)"
        );
    }

    #[test]
    fn parse_config_rejects_rlo_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ab\\u202Ecd\"\nexpand = \"v\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject key containing U+202E (Right-to-Left Override)"
        );
    }

    #[test]
    fn parse_config_rejects_rlo_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"rm -rf \\u202E/ echo safe\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject expand containing U+202E (Right-to-Left Override)"
        );
    }

    #[test]
    fn parse_config_rejects_bom_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\\uFEFF\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject expand containing U+FEFF (BOM)"
        );
    }

    #[test]
    fn parse_config_rejects_bom_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"\\uFEFFlsd\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry containing U+FEFF (BOM)"
        );
    }

    #[test]
    fn parse_config_rejects_zwsp_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\\u200Bcd\"\nexpand = \"v\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject key containing U+200B (Zero-Width Space)"
        );
    }

    /// `when_command_exists` values must be bare command names, not filesystem paths.
    /// A value like `"/usr/bin/ls"` is a path traversal attempt: `dir.join("/usr/bin/ls")`
    /// on Unix resolves to an absolute path, bypassing the intended restriction to check
    /// only within `path_prepend`.
    #[test]
    fn parse_config_rejects_path_separator_in_when_command_exists() {
        for bad in ["/usr/bin/ls", "../../evil", "../bin/sh"] {
            let toml = format!(
                "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"{bad}\"]\n"
            );
            assert!(
                parse_config(&toml).is_err(),
                "must reject when_command_exists entry containing '/': {bad:?}"
            );
        }
    }

    /// On Windows, backslash is a path separator. Paths like `C:\bin\ls` must be
    /// caught at parse time before they reach `make_command_exists`.
    #[test]
    fn parse_config_rejects_backslash_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"bin\\\\ls\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry containing backslash"
        );
    }

    /// A colon introduces a Windows drive letter (e.g. `C:ls`) or acts as a
    /// PATH-like separator in some contexts.
    #[test]
    fn parse_config_rejects_colon_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"C:ls\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry containing colon"
        );
    }

    #[test]
    fn parse_config_accepts_bare_command_name_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"lsd\"]\n";
        assert!(
            parse_config(toml).is_ok(),
            "must accept bare command name in when_command_exists"
        );
    }

    } // mod deceptive_unicode

    /// Each abbr rule's `when_command_exists` list is iterated on every expand call.
    /// Without a cap, a config with 100,000 entries would cause:
    /// - ~25 MB memory per rule (100,000 × 255 bytes)
    /// - 100,000 `which::which()` calls per keystroke — CPU/I/O DoS
    ///
    /// Capped at `MAX_CMD_LIST_LEN` entries per rule.
    mod when_command_exists_limit {
        use super::*;

    #[test]
    fn parse_config_rejects_too_many_when_command_exists_entries() {
        let cmds: Vec<String> = (0..=64).map(|i| format!("\"cmd{i}\"")).collect();
        let toml = format!(
            "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [{}]\n",
            cmds.join(", ")
        );
        assert!(
            parse_config(&toml).is_err(),
            "must reject when_command_exists with more than 64 entries"
        );
    }

    #[test]
    fn parse_config_accepts_max_when_command_exists_entries() {
        let cmds: Vec<String> = (0..64).map(|i| format!("\"cmd{i}\"")).collect();
        let toml = format!(
            "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [{}]\n",
            cmds.join(", ")
        );
        assert!(
            parse_config(&toml).is_ok(),
            "must accept when_command_exists with exactly 64 entries"
        );
    }

    } // mod when_command_exists_limit

    /// The only supported config schema version is 1. A config file with version=2
    /// (or any other value) was written for a different schema and must be rejected
    /// rather than silently processed as version=1. Accepting unknown versions risks
    /// missing new validation rules introduced in a later schema.
    mod version_validation {
        use super::*;

    #[test]
    fn parse_config_rejects_version_0() {
        let toml = "version = 0\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject version=0 (unsupported schema version)"
        );
    }

    #[test]
    fn parse_config_rejects_version_2() {
        let toml = "version = 2\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject version=2 (unsupported schema version)"
        );
    }

    #[test]
    fn parse_config_rejects_version_99() {
        let toml = "version = 99\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject version=99 (unsupported schema version)"
        );
    }

    #[test]
    fn parse_config_accepts_version_1() {
        let toml = "version = 1\n";
        assert!(
            parse_config(toml).is_ok(),
            "must accept version=1 (the current supported schema)"
        );
    }

    } // mod version_validation

    /// An expand value that is empty or whitespace-only is functionally broken:
    /// - Empty: pressing the trigger key replaces the token with nothing — almost certainly a mistake.
    /// - Whitespace-only: replaces the token with invisible characters — confusing and unintended.
    ///
    /// Both are rejected early so users get a clear error rather than silent breakage.
    mod expand_validation {
        use super::*;

    #[test]
    fn parse_config_rejects_empty_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject an abbr rule with an empty expand"
        );
    }

    #[test]
    fn parse_config_rejects_whitespace_only_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"   \"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject an abbr rule with a whitespace-only expand"
        );
    }

    #[test]
    fn parse_config_accepts_normal_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n";
        assert!(
            parse_config(toml).is_ok(),
            "must accept a normal non-empty expand value"
        );
    }

    } // mod expand_validation
}
