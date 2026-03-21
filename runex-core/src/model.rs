use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TriggerKey {
    #[default]
    Space,
    Tab,
    AltSpace,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
pub struct KeybindConfig {
    pub trigger: Option<TriggerKey>,
    pub bash: Option<TriggerKey>,
    pub pwsh: Option<TriggerKey>,
    pub nu: Option<TriggerKey>,
}

/// A single abbreviation rule: rune → cast.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Abbr {
    pub key: String,
    pub expand: String,
    pub when_command_exists: Option<Vec<String>>,
}

/// Top-level configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
            trigger: Some(TriggerKey::Space),
            bash: Some(TriggerKey::AltSpace),
            pwsh: Some(TriggerKey::Tab),
            nu: None,
        };
        assert_eq!(k.trigger, Some(TriggerKey::Space));
        assert_eq!(k.bash, Some(TriggerKey::AltSpace));
        assert_eq!(k.pwsh, Some(TriggerKey::Tab));
        assert_eq!(k.nu, None);
    }

    #[test]
    fn expand_result_variants() {
        let expanded = ExpandResult::Expanded("git commit -m".into());
        let pass = ExpandResult::PassThrough("unknown".into());
        assert_eq!(expanded, ExpandResult::Expanded("git commit -m".into()));
        assert_eq!(pass, ExpandResult::PassThrough("unknown".into()));
    }
}
