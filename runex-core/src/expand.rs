use crate::model::{Config, ExpandResult};

/// Result of a `which` lookup — carries enough detail for `--why` output.
#[derive(Debug, Clone, PartialEq)]
pub enum WhichResult {
    /// Token matched a rule and all conditions passed.
    Expanded {
        key: String,
        expansion: String,
        rule_index: usize,
        /// Commands that were checked via `when_command_exists` and passed.
        satisfied_conditions: Vec<String>,
    },
    /// Token matched a rule but key == expand (self-loop guard fired).
    SelfLoop { key: String },
    /// Token matched a rule but one or more guard commands are absent.
    ConditionFailed {
        key: String,
        missing_commands: Vec<String>,
        rule_index: usize,
    },
    /// No rule matched this token.
    NoMatch { token: String },
}

/// Expand a token using the config.
///
/// `command_exists` is injected for testability (DI).
pub fn expand<F>(config: &Config, token: &str, command_exists: F) -> ExpandResult
where
    F: Fn(&str) -> bool,
{
    for abbr in &config.abbr {
        if abbr.key != token {
            continue;
        }
        // Infinite-loop guard: key == expand means no-op.
        if abbr.key == abbr.expand {
            continue;
        }
        // Check when_command_exists condition.
        if let Some(cmds) = &abbr.when_command_exists {
            if !cmds.iter().all(|c| command_exists(c)) {
                continue;
            }
        }
        return ExpandResult::Expanded(abbr.expand.clone());
    }
    ExpandResult::PassThrough(token.to_string())
}

/// Look up a token and return why it expands (or doesn't).
///
/// Unlike `expand()`, this returns a detailed result suitable for human display.
/// `expand()` itself is unchanged to keep the hot path clean.
pub fn which_abbr<F>(config: &Config, token: &str, command_exists: F) -> WhichResult
where
    F: Fn(&str) -> bool,
{
    for (i, abbr) in config.abbr.iter().enumerate() {
        if abbr.key != token {
            continue;
        }
        if abbr.key == abbr.expand {
            return WhichResult::SelfLoop {
                key: abbr.key.clone(),
            };
        }
        if let Some(cmds) = &abbr.when_command_exists {
            let missing: Vec<String> = cmds
                .iter()
                .filter(|c| !command_exists(c))
                .cloned()
                .collect();
            if !missing.is_empty() {
                return WhichResult::ConditionFailed {
                    key: abbr.key.clone(),
                    missing_commands: missing,
                    rule_index: i,
                };
            }
        }
        return WhichResult::Expanded {
            key: abbr.key.clone(),
            expansion: abbr.expand.clone(),
            rule_index: i,
            satisfied_conditions: abbr
                .when_command_exists
                .as_deref()
                .unwrap_or(&[])
                .to_vec(),
        };
    }
    WhichResult::NoMatch {
        token: token.to_string(),
    }
}

/// List all abbreviations as (key, expand) pairs.
pub fn list(config: &Config) -> Vec<(&str, &str)> {
    config
        .abbr
        .iter()
        .map(|a| (a.key.as_str(), a.expand.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Abbr, Config};

    fn cfg(abbrs: Vec<Abbr>) -> Config {
        Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: abbrs,
        }
    }

    fn abbr(key: &str, expand: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: expand.into(),
            when_command_exists: None,
        }
    }

    fn abbr_when(key: &str, exp: &str, cmds: Vec<&str>) -> Abbr {
        Abbr {
            key: key.into(),
            expand: exp.into(),
            when_command_exists: Some(cmds.into_iter().map(String::from).collect()),
        }
    }

    #[test]
    fn match_expands() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        assert_eq!(
            expand(&c, "gcm", |_| true),
            ExpandResult::Expanded("git commit -m".into())
        );
    }

    #[test]
    fn no_match_passes_through() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        assert_eq!(
            expand(&c, "xyz", |_| true),
            ExpandResult::PassThrough("xyz".into())
        );
    }

    #[test]
    fn selects_correct_abbr() {
        let c = cfg(vec![
            abbr("gcm", "git commit -m"),
            abbr("gp", "git push"),
        ]);
        assert_eq!(
            expand(&c, "gp", |_| true),
            ExpandResult::Expanded("git push".into())
        );
    }

    #[test]
    fn key_eq_expand_passes_through() {
        let c = cfg(vec![abbr("ls", "ls")]);
        assert_eq!(
            expand(&c, "ls", |_| true),
            ExpandResult::PassThrough("ls".into())
        );
    }

    #[test]
    fn when_command_exists_present() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        assert_eq!(
            expand(&c, "ls", |_| true),
            ExpandResult::Expanded("lsd".into())
        );
    }

    #[test]
    fn when_command_exists_absent() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        assert_eq!(
            expand(&c, "ls", |_| false),
            ExpandResult::PassThrough("ls".into())
        );
    }

    #[test]
    fn list_returns_all_pairs() {
        let c = cfg(vec![
            abbr("gcm", "git commit -m"),
            abbr("gp", "git push"),
        ]);
        let pairs = list(&c);
        assert_eq!(pairs, vec![("gcm", "git commit -m"), ("gp", "git push")]);
    }
}
