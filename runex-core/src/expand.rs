use serde::Serialize;

use crate::model::{Config, ExpandResult};

/// A single skipped rule — part of the `which_abbr` trace.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SkipReason {
    /// key == expand (self-loop guard).
    SelfLoop,
    /// One or more `when_command_exists` commands were absent.
    ConditionFailed {
        found_commands: Vec<String>,
        missing_commands: Vec<String>,
    },
}

/// Result of a `which` lookup — mirrors `expand()` scan order exactly.
///
/// `skipped` contains every rule that matched the key but was bypassed,
/// in the same order `expand()` would skip them. This ensures `which_abbr`
/// and `expand` agree on the final outcome even with duplicate-key rules.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum WhichResult {
    /// Token matched a rule and all conditions passed.
    Expanded {
        key: String,
        expansion: String,
        rule_index: usize,
        /// Commands that were checked via `when_command_exists` and passed.
        satisfied_conditions: Vec<String>,
        /// Earlier rules with the same key that were skipped before this one.
        skipped: Vec<(usize, SkipReason)>,
    },
    /// Every matching rule was skipped; here is why each one was bypassed.
    AllSkipped {
        token: String,
        skipped: Vec<(usize, SkipReason)>,
    },
    /// No rule had this key at all.
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
        if abbr.key == abbr.expand {
            continue;
        }
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
/// Scans rules in the same order as `expand()`, collecting skip reasons for
/// every bypassed rule before returning the first one that passes. This means
/// `which_abbr` and `expand` always agree on the final outcome, even when
/// multiple rules share the same key.
pub fn which_abbr<F>(config: &Config, token: &str, command_exists: F) -> WhichResult
where
    F: Fn(&str) -> bool,
{
    let mut skipped: Vec<(usize, SkipReason)> = Vec::new();
    let mut any_key_matched = false;

    for (i, abbr) in config.abbr.iter().enumerate() {
        if abbr.key != token {
            continue;
        }
        any_key_matched = true;

        if abbr.key == abbr.expand {
            skipped.push((i, SkipReason::SelfLoop));
            continue;
        }
        if let Some(cmds) = &abbr.when_command_exists {
            let (found, missing): (Vec<String>, Vec<String>) =
                cmds.iter().cloned().partition(|c| command_exists(c));
            if !missing.is_empty() {
                skipped.push((
                    i,
                    SkipReason::ConditionFailed {
                        found_commands: found,
                        missing_commands: missing,
                    },
                ));
                continue;
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
            skipped,
        };
    }

    if any_key_matched {
        WhichResult::AllSkipped {
            token: token.to_string(),
            skipped,
        }
    } else {
        WhichResult::NoMatch {
            token: token.to_string(),
        }
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
    fn duplicate_key_self_loop_then_real_expands() {
        // expand() must skip the self-loop and expand using the second rule
        let c = cfg(vec![abbr("ls", "ls"), abbr("ls", "lsd")]);
        assert_eq!(expand(&c, "ls", |_| true), ExpandResult::Expanded("lsd".into()));
    }

    #[test]
    fn duplicate_key_failed_condition_then_real_expands() {
        // expand() skips the first (condition fails) and uses the second (no condition)
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"]), abbr("ls", "ls2")]);
        assert_eq!(expand(&c, "ls", |_| false), ExpandResult::Expanded("ls2".into()));
    }

    #[test]
    fn which_abbr_duplicate_self_loop_then_expanded() {
        let c = cfg(vec![abbr("ls", "ls"), abbr("ls", "lsd")]);
        let result = which_abbr(&c, "ls", |_| true);
        match result {
            WhichResult::Expanded { expansion, skipped, .. } => {
                assert_eq!(expansion, "lsd");
                assert_eq!(skipped.len(), 1);
                assert_eq!(skipped[0].0, 0); // rule index 0 was skipped
                assert!(matches!(skipped[0].1, SkipReason::SelfLoop));
            }
            other => panic!("expected Expanded, got {other:?}"),
        }
    }

    #[test]
    fn which_abbr_all_skipped_returns_all_skipped() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let result = which_abbr(&c, "ls", |_| false);
        match result {
            WhichResult::AllSkipped { skipped, .. } => {
                assert_eq!(skipped.len(), 1);
                assert!(matches!(
                    &skipped[0].1,
                    SkipReason::ConditionFailed { missing_commands, .. }
                    if missing_commands == &["lsd"]
                ));
            }
            other => panic!("expected AllSkipped, got {other:?}"),
        }
    }

    #[test]
    fn which_abbr_no_match() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        assert!(matches!(which_abbr(&c, "xyz", |_| true), WhichResult::NoMatch { .. }));
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
