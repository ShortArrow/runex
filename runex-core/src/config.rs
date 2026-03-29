use std::path::PathBuf;

use crate::model::Config;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot determine config directory")]
    NoConfigDir,
}

/// Parse a TOML string into Config.
pub fn parse_config(s: &str) -> Result<Config, ConfigError> {
    let config: Config = toml::from_str(s)?;
    Ok(config)
}

/// Default config file path: `$XDG_CONFIG_HOME/runex/config.toml`,
/// falling back to `~/.config/runex/config.toml` when `XDG_CONFIG_HOME` is unset.
/// All platforms use this same resolution order.
/// Overridden by `RUNEX_CONFIG` env var.
pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    if let Ok(p) = std::env::var("RUNEX_CONFIG") {
        return Ok(PathBuf::from(p));
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
pub fn load_config(path: &std::path::Path) -> Result<Config, ConfigError> {
    let content = std::fs::read_to_string(path)?;
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

    #[test]
    #[serial]
    fn default_config_path_env_override() {
        // RUNEX_CONFIG takes priority over XDG_CONFIG_HOME.
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::set_var("RUNEX_CONFIG", "/tmp/custom.toml");
        let path = default_config_path().unwrap();
        std::env::remove_var("RUNEX_CONFIG");
        assert_eq!(path, PathBuf::from("/tmp/custom.toml"));
    }

    #[test]
    #[serial]
    fn xdg_config_home_uses_env_var() {
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-test");
        let dir = xdg_config_home().unwrap();
        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(dir, PathBuf::from("/tmp/xdg-test"));
    }

    #[test]
    #[serial]
    fn xdg_config_home_empty_env_falls_back_to_home() {
        std::env::set_var("XDG_CONFIG_HOME", "");
        let dir = xdg_config_home().unwrap();
        std::env::remove_var("XDG_CONFIG_HOME");
        // Falls back to home/.config — must end with .config
        assert!(dir.ends_with(".config"), "expected ~/.config fallback, got {dir:?}");
    }

    #[test]
    #[serial]
    fn default_config_path_uses_xdg_config_home() {
        std::env::remove_var("RUNEX_CONFIG");
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg-runex-test");
        let path = default_config_path().unwrap();
        std::env::remove_var("XDG_CONFIG_HOME");
        assert_eq!(path, PathBuf::from("/tmp/xdg-runex-test/runex/config.toml"));
    }
}
