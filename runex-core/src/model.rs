use serde::Deserialize;

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
            abbr: vec![],
        };
        assert_eq!(c.version, 1);
        assert!(c.abbr.is_empty());
    }

    #[test]
    fn expand_result_variants() {
        let expanded = ExpandResult::Expanded("git commit -m".into());
        let pass = ExpandResult::PassThrough("unknown".into());
        assert_eq!(expanded, ExpandResult::Expanded("git commit -m".into()));
        assert_eq!(pass, ExpandResult::PassThrough("unknown".into()));
    }
}
