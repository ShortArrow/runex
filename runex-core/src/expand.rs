use serde::Serialize;

use std::cell::RefCell;
use std::time::Instant;

use crate::model::{Config, ExpandResult};
use crate::shell::Shell;
use crate::timings::{CommandExistsCall, Timings};

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
        let text = expansion.to_string();
        let (text, cursor_offset) = extract_cursor_placeholder(&text);
        return ExpandResult::Expanded { text, cursor_offset };
    }
    ExpandResult::PassThrough(token.to_string())
}

/// Extract cursor placeholder `{}` from expansion text.
/// Returns the text with `{}` removed and the byte offset where it was.
fn extract_cursor_placeholder(text: &str) -> (String, Option<usize>) {
    if let Some(pos) = text.find(crate::model::CURSOR_PLACEHOLDER) {
        let mut result = String::with_capacity(text.len() - 2);
        result.push_str(&text[..pos]);
        result.push_str(&text[pos + 2..]);
        (result, Some(pos))
    } else {
        (text.to_string(), None)
    }
}

/// Like [`expand`], but records timing data into `timings`.
///
/// Each `command_exists` call is individually timed, and the overall expand
/// phase is recorded as a single phase entry.
pub fn expand_timed<F>(
    config: &Config,
    token: &str,
    shell: Shell,
    command_exists: F,
    timings: &mut Timings,
) -> ExpandResult
where
    F: Fn(&str) -> bool,
{
    let calls: RefCell<Vec<CommandExistsCall>> = RefCell::new(Vec::new());
    let timer = Instant::now();

    let timed_exists = |cmd: &str| -> bool {
        let t = Instant::now();
        let found = command_exists(cmd);
        let elapsed = t.elapsed();
        calls.borrow_mut().push(CommandExistsCall {
            command: cmd.to_string(),
            found,
            duration: elapsed,
            // Heuristic: if the lookup completed in under 100us, it was likely a cache hit.
            // A real which::which() call takes ~9ms on typical systems.
            cached: elapsed.as_micros() < 100,
        });
        found
    };

    let result = expand(config, token, shell, timed_exists);
    timings.record_phase("expand", timer.elapsed());

    for call in calls.into_inner() {
        timings.record_command_exists(&call.command, call.found, call.duration, call.cached);
    }

    result
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
            precache: crate::model::PrecacheConfig::default(),
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
            ExpandResult::Expanded { text: "git commit -m".into(), cursor_offset: None }
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
            ExpandResult::Expanded { text: "git push".into(), cursor_offset: None }
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
            ExpandResult::Expanded { text: "lsd".into(), cursor_offset: None }
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
            ExpandResult::Expanded { text: "lsd".into(), cursor_offset: None }
        );
    }

    #[test]
    fn duplicate_key_failed_condition_then_real_expands() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"]), abbr("ls", "ls2")]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| false),
            ExpandResult::Expanded { text: "ls2".into(), cursor_offset: None }
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
            ExpandResult::Expanded { text: "7z.exe".into(), cursor_offset: None }
        );
        assert_eq!(
            expand(&c, "7z", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "7zip".into(), cursor_offset: None }
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
            ExpandResult::Expanded { text: "7z.exe".into(), cursor_offset: None }
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

    // ── expand_timed tests ──────────────────────────────────────────────

    #[test]
    fn expand_timed_same_result_as_expand() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let mut timings = crate::timings::Timings::new();
        let result = expand_timed(&c, "ls", Shell::Bash, |_| true, &mut timings);
        assert_eq!(result, ExpandResult::Expanded { text: "lsd".into(), cursor_offset: None });
    }

    #[test]
    fn expand_timed_records_command_exists_calls() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let mut timings = crate::timings::Timings::new();
        expand_timed(&c, "ls", Shell::Bash, |_| true, &mut timings);
        let calls = timings.command_exists_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].command, "lsd");
        assert!(calls[0].found);
    }

    #[test]
    fn expand_timed_records_expand_phase() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        let mut timings = crate::timings::Timings::new();
        expand_timed(&c, "gcm", Shell::Bash, |_| true, &mut timings);
        let phases = timings.phases();
        assert!(
            phases.iter().any(|p| p.name == "expand"),
            "must record an 'expand' phase, got: {:?}",
            phases.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }

    // ── cursor placeholder tests ────────────────────────────────────────

    #[test]
    fn expand_with_cursor_placeholder() {
        let c = cfg(vec![abbr("gcam", "git commit -am '{}'")] );
        let result = expand(&c, "gcam", Shell::Bash, |_| true);
        assert_eq!(
            result,
            ExpandResult::Expanded {
                text: "git commit -am ''".into(),
                cursor_offset: Some(16), // position between the quotes
            }
        );
    }

    #[test]
    fn expand_without_cursor_placeholder() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        let result = expand(&c, "gcm", Shell::Bash, |_| true);
        assert_eq!(
            result,
            ExpandResult::Expanded { text: "git commit -m".into(), cursor_offset: None }
        );
    }

    #[test]
    fn extract_cursor_placeholder_found() {
        let (text, offset) = extract_cursor_placeholder("git commit -am '{}'");
        assert_eq!(text, "git commit -am ''");
        assert_eq!(offset, Some(16));
    }

    #[test]
    fn extract_cursor_placeholder_not_found() {
        let (text, offset) = extract_cursor_placeholder("git commit -m");
        assert_eq!(text, "git commit -m");
        assert_eq!(offset, None);
    }

    #[test]
    fn extract_cursor_placeholder_at_end() {
        let (text, offset) = extract_cursor_placeholder("echo {}");
        assert_eq!(text, "echo ");
        assert_eq!(offset, Some(5));
    }
}
