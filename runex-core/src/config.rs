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

/// Default config file path: `~/.config/runex/config.toml` (or platform equivalent).
/// Overridden by `RUNEX_CONFIG` env var.
pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    if let Ok(p) = std::env::var("RUNEX_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let dir = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
    Ok(dir.join("runex").join("config.toml"))
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
literal = "alt-space"
bash = "alt-space"
pwsh = "tab"
pwsh_literal = "alt-space"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.keybind.trigger, Some(TriggerKey::Space));
        assert_eq!(config.keybind.literal, Some(TriggerKey::AltSpace));
        assert_eq!(config.keybind.bash, Some(TriggerKey::AltSpace));
        assert_eq!(config.keybind.pwsh, Some(TriggerKey::Tab));
        assert_eq!(config.keybind.pwsh_literal, Some(TriggerKey::AltSpace));
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
    fn default_config_path_env_override() {
        std::env::set_var("RUNEX_CONFIG", "/tmp/custom.toml");
        let path = default_config_path().unwrap();
        assert_eq!(path, PathBuf::from("/tmp/custom.toml"));
        std::env::remove_var("RUNEX_CONFIG");
    }
}
