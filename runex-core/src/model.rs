use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TriggerKey {
    #[default]
    Space,
    Tab,
    AltSpace,
    ShiftSpace,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PerShellKey {
    pub default: Option<TriggerKey>,
    pub bash: Option<TriggerKey>,
    pub zsh: Option<TriggerKey>,
    pub pwsh: Option<TriggerKey>,
    pub nu: Option<TriggerKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct KeybindConfig {
    #[serde(default)]
    pub trigger: PerShellKey,
    #[serde(default)]
    pub self_insert: PerShellKey,
}

/// A single abbreviation rule: rune → cast.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Abbr {
    pub key: String,
    pub expand: String,
    pub when_command_exists: Option<Vec<String>>,
}

/// Top-level configuration.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub keybind: KeybindConfig,
    #[serde(default)]
    pub abbr: Vec<Abbr>,
}

/// Result of an expand operation.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpandResult {
    Expanded(String),
    PassThrough(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbr_fields() {
        let a = Abbr {
            key: "gcm".into(),
            expand: "git commit -m".into(),
            when_command_exists: None,
        };
        assert_eq!(a.key, "gcm");
        assert_eq!(a.expand, "git commit -m");
        assert!(a.when_command_exists.is_none());
    }

    #[test]
    fn abbr_with_when_command_exists() {
        let a = Abbr {
            key: "ls".into(),
            expand: "lsd".into(),
            when_command_exists: Some(vec!["lsd".into()]),
        };
        assert_eq!(a.when_command_exists.unwrap(), vec!["lsd".to_string()]);
    }

    #[test]
    fn config_fields() {
        let c = Config {
            version: 1,
            keybind: KeybindConfig::default(),
            abbr: vec![],
        };
        assert_eq!(c.version, 1);
        assert_eq!(c.keybind, KeybindConfig::default());
        assert!(c.abbr.is_empty());
    }

    #[test]
    fn keybind_config_fields() {
        let k = KeybindConfig {
            trigger: PerShellKey {
                default: Some(TriggerKey::Space),
                bash: Some(TriggerKey::AltSpace),
                zsh: Some(TriggerKey::Space),
                pwsh: Some(TriggerKey::Tab),
                nu: None,
            },
            self_insert: PerShellKey::default(),
        };
        assert_eq!(k.trigger.default, Some(TriggerKey::Space));
        assert_eq!(k.trigger.bash, Some(TriggerKey::AltSpace));
        assert_eq!(k.trigger.zsh, Some(TriggerKey::Space));
        assert_eq!(k.trigger.pwsh, Some(TriggerKey::Tab));
        assert_eq!(k.trigger.nu, None);
        assert_eq!(k.self_insert, PerShellKey::default());
    }

    #[test]
    fn parse_config_accepts_self_insert_shift_space() {
        let toml = r#"
version = 1
[keybind.self_insert]
pwsh = "shift-space"
"#;
        let config: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(
            config.keybind.self_insert.pwsh,
            Some(TriggerKey::ShiftSpace),
            "self_insert.pwsh should deserialize to ShiftSpace"
        );
    }

    #[test]
    fn parse_config_keybind_entirely_absent() {
        let toml = "version = 1\n";
        let config: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(config.keybind.trigger, PerShellKey::default());
        assert_eq!(config.keybind.self_insert, PerShellKey::default());
    }

    #[test]
    fn expand_result_variants() {
        let expanded = ExpandResult::Expanded("git commit -m".into());
        let pass = ExpandResult::PassThrough("unknown".into());
        assert_eq!(expanded, ExpandResult::Expanded("git commit -m".into()));
        assert_eq!(pass, ExpandResult::PassThrough("unknown".into()));
    }
}
