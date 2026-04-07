use serde::Serialize;

use crate::model::{Config, ExpandResult};
use crate::shell::Shell;

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
    /// No expand entry for this shell (and no default).
    NoShellEntry,
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
/// `shell` selects the per-shell expand/when_command_exists entry.
/// `command_exists` is injected for testability (DI).
pub fn expand<F>(config: &Config, token: &str, shell: Shell, command_exists: F) -> ExpandResult
where
    F: Fn(&str) -> bool,
{
    for abbr in &config.abbr {
        if abbr.key != token {
            continue;
        }
        let Some(expansion) = abbr.expand.for_shell(shell) else {
            continue; // no entry for this shell → skip
        };
        if abbr.key == expansion {
            continue; // self-loop
        }
        if let Some(cmds) = &abbr.when_command_exists {
            let shell_cmds = cmds.for_shell(shell);
            if let Some(list) = shell_cmds {
                if !list.iter().all(|c| command_exists(c)) {
                    continue;
                }
            } else {
                continue; // no when_command_exists entry for this shell → skip
            }
        }
        return ExpandResult::Expanded(expansion.to_string());
    }
    ExpandResult::PassThrough(token.to_string())
}

/// Look up a token and return why it expands (or doesn't).
///
/// Scans rules in the same order as `expand()`, collecting skip reasons for
/// every bypassed rule before returning the first one that passes. This means
/// `which_abbr` and `expand` always agree on the final outcome, even when
/// multiple rules share the same key.
pub fn which_abbr<F>(config: &Config, token: &str, shell: Shell, command_exists: F) -> WhichResult
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

        let Some(expansion) = abbr.expand.for_shell(shell) else {
            skipped.push((i, SkipReason::NoShellEntry));
            continue;
        };

        if abbr.key == expansion {
            skipped.push((i, SkipReason::SelfLoop));
            continue;
        }

        if let Some(cmds) = &abbr.when_command_exists {
            match cmds.for_shell(shell) {
                None => {
                    skipped.push((i, SkipReason::NoShellEntry));
                    continue;
                }
                Some(list) => {
                    let (found, missing): (Vec<String>, Vec<String>) =
                        list.iter().cloned().partition(|c| command_exists(c));
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
                    return WhichResult::Expanded {
                        key: abbr.key.clone(),
                        expansion: expansion.to_string(),
                        rule_index: i,
                        satisfied_conditions: list.to_vec(),
                        skipped,
                    };
                }
            }
        }

        return WhichResult::Expanded {
            key: abbr.key.clone(),
            expansion: expansion.to_string(),
            rule_index: i,
            satisfied_conditions: Vec::new(),
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

/// List abbreviations as (key, expand) pairs.
///
/// When `shell` is `Some`, returns only rules that have an entry for that shell,
/// using the resolved expansion string.
/// When `shell` is `None`, uses the `All` value or the `default` field.
pub fn list<'a>(config: &'a Config, shell: Option<Shell>) -> Vec<(&'a str, String)> {
    config
        .abbr
        .iter()
        .filter_map(|a| {
            let exp = match shell {
                Some(sh) => a.expand.for_shell(sh)?.to_string(),
                None => match &a.expand {
                    crate::model::PerShellString::All(s) => s.clone(),
                    crate::model::PerShellString::ByShell { default, .. } => {
                        default.as_deref()?.to_string()
                    }
                },
            };
            Some((a.key.as_str(), exp))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Abbr, Config, PerShellCmds, PerShellString};

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
            expand: PerShellString::All(expand.into()),
            when_command_exists: None,
        }
    }

    fn abbr_when(key: &str, exp: &str, cmds: Vec<&str>) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(exp.into()),
            when_command_exists: Some(PerShellCmds::All(
                cmds.into_iter().map(String::from).collect(),
            )),
        }
    }

    fn abbr_pershell_expand(key: &str, expand: PerShellString) -> Abbr {
        Abbr {
            key: key.into(),
            expand,
            when_command_exists: None,
        }
    }

    // ── existing tests (updated signatures) ────────────────────────────────

    #[test]
    fn match_expands() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        assert_eq!(
            expand(&c, "gcm", Shell::Bash, |_| true),
            ExpandResult::Expanded("git commit -m".into())
        );
    }

    #[test]
    fn no_match_passes_through() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        assert_eq!(
            expand(&c, "xyz", Shell::Bash, |_| true),
            ExpandResult::PassThrough("xyz".into())
        );
    }

    #[test]
    fn selects_correct_abbr() {
        let c = cfg(vec![abbr("gcm", "git commit -m"), abbr("gp", "git push")]);
        assert_eq!(
            expand(&c, "gp", Shell::Bash, |_| true),
            ExpandResult::Expanded("git push".into())
        );
    }

    #[test]
    fn key_eq_expand_passes_through() {
        let c = cfg(vec![abbr("ls", "ls")]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| true),
            ExpandResult::PassThrough("ls".into())
        );
    }

    #[test]
    fn when_command_exists_present() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| true),
            ExpandResult::Expanded("lsd".into())
        );
    }

    #[test]
    fn when_command_exists_absent() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| false),
            ExpandResult::PassThrough("ls".into())
        );
    }

    #[test]
    fn duplicate_key_self_loop_then_real_expands() {
        let c = cfg(vec![abbr("ls", "ls"), abbr("ls", "lsd")]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| true),
            ExpandResult::Expanded("lsd".into())
        );
    }

    #[test]
    fn duplicate_key_failed_condition_then_real_expands() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"]), abbr("ls", "ls2")]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| false),
            ExpandResult::Expanded("ls2".into())
        );
    }

    #[test]
    fn which_abbr_duplicate_self_loop_then_expanded() {
        let c = cfg(vec![abbr("ls", "ls"), abbr("ls", "lsd")]);
        let result = which_abbr(&c, "ls", Shell::Bash, |_| true);
        match result {
            WhichResult::Expanded { expansion, skipped, .. } => {
                assert_eq!(expansion, "lsd");
                assert_eq!(skipped.len(), 1);
                assert_eq!(skipped[0].0, 0);
                assert!(matches!(skipped[0].1, SkipReason::SelfLoop));
            }
            other => panic!("expected Expanded, got {other:?}"),
        }
    }

    #[test]
    fn which_abbr_all_skipped_returns_all_skipped() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let result = which_abbr(&c, "ls", Shell::Bash, |_| false);
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
        assert!(matches!(
            which_abbr(&c, "xyz", Shell::Bash, |_| true),
            WhichResult::NoMatch { .. }
        ));
    }

    #[test]
    fn list_returns_all_pairs() {
        let c = cfg(vec![abbr("gcm", "git commit -m"), abbr("gp", "git push")]);
        let pairs = list(&c, None);
        assert_eq!(
            pairs,
            vec![("gcm", "git commit -m".to_string()), ("gp", "git push".to_string())]
        );
    }

    // ── per-shell expand tests ──────────────────────────────────────────────

    #[test]
    fn expand_per_shell_pwsh_uses_pwsh_expand() {
        // key="7z", default="7zip", pwsh="7z.exe" — no self-loop on any shell
        let c = cfg(vec![abbr_pershell_expand(
            "7z",
            PerShellString::ByShell {
                default: Some("7zip".into()),
                pwsh: Some("7z.exe".into()),
                bash: None, zsh: None, nu: None,
            },
        )]);
        assert_eq!(
            expand(&c, "7z", Shell::Pwsh, |_| true),
            ExpandResult::Expanded("7z.exe".into())
        );
        assert_eq!(
            expand(&c, "7z", Shell::Bash, |_| true),
            ExpandResult::Expanded("7zip".into())
        );
    }

    #[test]
    fn expand_per_shell_skips_when_no_shell_entry() {
        let c = cfg(vec![abbr_pershell_expand(
            "7z",
            PerShellString::ByShell {
                default: None,
                pwsh: Some("7z.exe".into()),
                bash: None, zsh: None, nu: None,
            },
        )]);
        // No entry for bash/default → pass-through
        assert_eq!(
            expand(&c, "7z", Shell::Bash, |_| true),
            ExpandResult::PassThrough("7z".into())
        );
        // pwsh has an entry → expands
        assert_eq!(
            expand(&c, "7z", Shell::Pwsh, |_| true),
            ExpandResult::Expanded("7z.exe".into())
        );
    }

    #[test]
    fn which_abbr_no_shell_entry_is_skipped() {
        let c = cfg(vec![abbr_pershell_expand(
            "7z",
            PerShellString::ByShell {
                default: None,
                pwsh: Some("7z.exe".into()),
                bash: None, zsh: None, nu: None,
            },
        )]);
        let result = which_abbr(&c, "7z", Shell::Bash, |_| true);
        match result {
            WhichResult::AllSkipped { skipped, .. } => {
                assert_eq!(skipped.len(), 1);
                assert!(matches!(skipped[0].1, SkipReason::NoShellEntry));
            }
            other => panic!("expected AllSkipped, got {other:?}"),
        }
    }

    #[test]
    fn list_with_shell_filters_per_shell() {
        let c = cfg(vec![
            abbr_pershell_expand(
                "7z",
                PerShellString::ByShell {
                    default: Some("7zip".into()),
                    pwsh: Some("7z.exe".into()),
                    bash: None, zsh: None, nu: None,
                },
            ),
            abbr_pershell_expand(
                "pwsh-only",
                PerShellString::ByShell {
                    default: None,
                    pwsh: Some("pwsh-cmd".into()),
                    bash: None, zsh: None, nu: None,
                },
            ),
        ]);
        let bash_list = list(&c, Some(Shell::Bash));
        // "7z" has default so shows; "pwsh-only" has no bash/default → filtered out
        assert_eq!(bash_list, vec![("7z", "7zip".to_string())]);

        let pwsh_list = list(&c, Some(Shell::Pwsh));
        assert_eq!(
            pwsh_list,
            vec![
                ("7z", "7z.exe".to_string()),
                ("pwsh-only", "pwsh-cmd".to_string()),
            ]
        );
    }
}
