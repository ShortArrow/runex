use runex_core::doctor::{Check, CheckStatus};
use runex_core::expand::{self, WhichResult};
use runex_core::sanitize::sanitize_for_display;

use crate::{ANSI_GREEN, ANSI_RED, ANSI_RESET, ANSI_YELLOW, CHECK_TAG_WIDTH, GIT_COMMIT};

pub(crate) fn format_check_tag(status: &CheckStatus) -> String {
    match status {
        CheckStatus::Ok => format!("[{ANSI_GREEN}OK{ANSI_RESET}]"),
        CheckStatus::Warn => format!("[{ANSI_YELLOW}WARN{ANSI_RESET}]"),
        CheckStatus::Error => format!("[{ANSI_RED}ERROR{ANSI_RESET}]"),
    }
}

pub(crate) fn format_check_line(check: &Check) -> String {
    format!(
        "{:>CHECK_TAG_WIDTH$}  {}: {}",
        format_check_tag(&check.status),
        check.name,
        check.detail
    )
}

pub(crate) fn version_line() -> String {
    let version = env!("CARGO_PKG_VERSION");
    match GIT_COMMIT {
        Some(commit) if !commit.is_empty() => format!("runex {version} ({commit})"),
        _ => format!("runex {version}"),
    }
}

pub(crate) fn format_skip_reason(i: usize, reason: &expand::SkipReason, why: bool) -> String {
    if !why {
        return String::new();
    }
    match reason {
        expand::SkipReason::SelfLoop => {
            format!("\n  rule #{} skipped: key == expand (self-loop)", i + 1)
        }
        expand::SkipReason::ConditionFailed { found_commands, missing_commands } => {
            let mut parts = Vec::new();
            for cmd in found_commands {
                parts.push(format!("{}: found", sanitize_for_display(cmd)));
            }
            for cmd in missing_commands {
                parts.push(format!("{}: NOT FOUND", sanitize_for_display(cmd)));
            }
            format!(
                "\n  rule #{} skipped: when_command_exists [{}]",
                i + 1,
                parts.join(", ")
            )
        }
    }
}

/// Collect all missing commands from `ConditionFailed` skip reasons, deduplicated.
pub(crate) fn collect_all_missing_commands(skipped: &[(usize, expand::SkipReason)]) -> Vec<String> {
    skipped
        .iter()
        .flat_map(|(_, r)| match r {
            expand::SkipReason::ConditionFailed { missing_commands, .. } => {
                missing_commands.iter().map(|c| sanitize_for_display(c)).collect::<Vec<_>>()
            }
            _ => vec![],
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

/// Build the headline for a `WhichResult::AllSkipped` result.
///
/// Summarises why every matching rule was bypassed into a single human-readable
/// line, choosing the message based on which skip reasons are present.
pub(crate) fn format_all_skipped_headline(
    token: &str,
    skipped: &[(usize, expand::SkipReason)],
) -> String {
    let has_condition_fail = skipped
        .iter()
        .any(|(_, r)| matches!(r, expand::SkipReason::ConditionFailed { .. }));
    let has_self_loop = skipped
        .iter()
        .any(|(_, r)| matches!(r, expand::SkipReason::SelfLoop));
    match (has_condition_fail, has_self_loop) {
        (true, true) => format!(
            "{token}  [skipped: condition failed on some rules; others are self-loops]"
        ),
        (true, false) => {
            let all_missing = collect_all_missing_commands(skipped);
            format!("{token}  [skipped: {} not found]", all_missing.join(", "))
        }
        (false, true) => format!("{token}  [no-op: key and expansion are identical]"),
        (false, false) => format!("{token}: no rule found"),
    }
}

pub(crate) fn format_which_result(result: &WhichResult, why: bool) -> String {
    match result {
        WhichResult::Expanded {
            key,
            expansion,
            rule_index,
            satisfied_conditions,
            skipped,
        } => {
            let key = sanitize_for_display(key);
            let expansion = sanitize_for_display(expansion);
            let mut s = format!("{key}  ->  {expansion}");
            if why {
                for (i, reason) in skipped {
                    s.push_str(&format_skip_reason(*i, reason, true));
                }
                s.push_str(&format!("\n  rule #{} matched", rule_index + 1));
                if satisfied_conditions.is_empty() {
                    s.push_str(", no conditions");
                } else {
                    for cmd in satisfied_conditions {
                        let cmd = sanitize_for_display(cmd);
                        s.push_str(&format!("\n  condition: when_command_exists '{cmd}' -> found"));
                    }
                }
            }
            s
        }
        WhichResult::AllSkipped { token, skipped } => {
            let token = sanitize_for_display(token);
            let headline = format_all_skipped_headline(&token, skipped);
            let mut s = headline;
            if why {
                for (i, reason) in skipped {
                    s.push_str(&format_skip_reason(*i, reason, true));
                }
            }
            s
        }
        WhichResult::NoMatch { token } => {
            format!("{}: no rule found", sanitize_for_display(token))
        }
    }
}

/// Convert a `WhichResult` to a JSON value with 1-based rule indices.
///
/// `WhichResult` stores 0-based indices internally (matching `enumerate()`).
/// The text output already presents these as `rule #1`, `rule #2`, etc., so
/// JSON must use the same numbering for consistency.
pub(crate) fn which_result_to_json(result: &WhichResult) -> serde_json::Value {
    match result {
        WhichResult::Expanded {
            key,
            expansion,
            rule_index,
            satisfied_conditions,
            skipped,
        } => serde_json::json!({
            "result": "expanded",
            "key": key,
            "expansion": expansion,
            "rule_index": rule_index + 1,
            "satisfied_conditions": satisfied_conditions,
            "skipped": skipped.iter().map(|(i, r)| serde_json::json!([i + 1, r])).collect::<Vec<_>>(),
        }),
        WhichResult::AllSkipped { token, skipped } => serde_json::json!({
            "result": "all_skipped",
            "token": token,
            "skipped": skipped.iter().map(|(i, r)| serde_json::json!([i + 1, r])).collect::<Vec<_>>(),
        }),
        WhichResult::NoMatch { token } => serde_json::json!({
            "result": "no_match",
            "token": token,
        }),
    }
}

pub(crate) fn format_dry_run_result(token: &str, result: &WhichResult) -> String {
    let mut out = String::new();
    out.push_str(&format!("token: {}\n", sanitize_for_display(token)));
    match result {
        WhichResult::Expanded {
            key,
            expansion,
            rule_index,
            satisfied_conditions,
            skipped,
        } => {
            for (i, reason) in skipped {
                match reason {
                    expand::SkipReason::SelfLoop => {
                        out.push_str(&format!("rule #{} skipped: self-loop\n", i + 1));
                    }
                    expand::SkipReason::ConditionFailed { found_commands, missing_commands } => {
                        out.push_str(&format!("rule #{} skipped: when_command_exists\n", i + 1));
                        for cmd in found_commands {
                            out.push_str(&format!("  {}: found\n", sanitize_for_display(cmd)));
                        }
                        for cmd in missing_commands {
                            out.push_str(&format!("  {}: NOT FOUND\n", sanitize_for_display(cmd)));
                        }
                    }
                }
            }
            out.push_str(&format!(
                "matched rule #{} (key = '{}')\n",
                rule_index + 1,
                sanitize_for_display(key)
            ));
            if satisfied_conditions.is_empty() {
                out.push_str("conditions: none\n");
            } else {
                out.push_str("conditions:\n");
                for cmd in satisfied_conditions {
                    out.push_str(&format!(
                        "  when_command_exists '{}': found\n",
                        sanitize_for_display(cmd)
                    ));
                }
            }
            out.push_str(&format!(
                "result: expanded  ->  {}\n",
                sanitize_for_display(expansion)
            ));
        }
        WhichResult::AllSkipped { token, skipped } => {
            for (i, reason) in skipped {
                match reason {
                    expand::SkipReason::SelfLoop => {
                        out.push_str(&format!("rule #{} skipped: self-loop\n", i + 1));
                    }
                    expand::SkipReason::ConditionFailed { found_commands, missing_commands } => {
                        out.push_str(&format!("rule #{} skipped: when_command_exists\n", i + 1));
                        for cmd in found_commands {
                            out.push_str(&format!("  {}: found\n", sanitize_for_display(cmd)));
                        }
                        for cmd in missing_commands {
                            out.push_str(&format!("  {}: NOT FOUND\n", sanitize_for_display(cmd)));
                        }
                    }
                }
            }
            out.push_str(&format!(
                "no rule for '{}' passed all conditions\n",
                sanitize_for_display(token)
            ));
            out.push_str("result: pass-through\n");
        }
        WhichResult::NoMatch { token } => {
            out.push_str(&format!(
                "no rule matched '{}'\n",
                sanitize_for_display(token)
            ));
            out.push_str("result: pass-through\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use runex_core::expand;

    #[test]
    fn format_check_line_colors_only_tag_text() {
        let check = Check {
            name: "config_file".into(),
            status: CheckStatus::Warn,
            detail: "detail".into(),
        };

        let line = format_check_line(&check);
        assert!(line.starts_with(&format!("[{ANSI_YELLOW}WARN{ANSI_RESET}]")));
        assert!(line.contains("config_file: detail"));
    }

    #[test]
    fn version_line_contains_pkg_version() {
        let line = version_line();
        assert!(line.starts_with(&format!("runex {}", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn format_which_result_expanded_strips_control_chars() {
        let result = WhichResult::Expanded {
            key: "key\x1b[2J".to_string(),
            expansion: "exp\x07anded".to_string(),
            rule_index: 0,
            satisfied_conditions: vec![],
            skipped: vec![],
        };
        let s = format_which_result(&result, false);
        assert!(!s.contains('\x1b'), "format_which_result: ESC in key must be stripped: {s:?}");
        assert!(!s.contains('\x07'), "format_which_result: BEL in expansion must be stripped: {s:?}");
    }

    #[test]
    fn format_which_result_why_strips_control_chars_from_cmd() {
        let result = WhichResult::AllSkipped {
            token: "ls".to_string(),
            skipped: vec![(0, expand::SkipReason::ConditionFailed {
                found_commands: vec![],
                missing_commands: vec!["cmd\x1b[31mevil\x1b[0m".to_string()],
            })],
        };
        let s = format_which_result(&result, true);
        assert!(!s.contains('\x1b'), "format_which_result --why: ESC in cmd must be stripped: {s:?}");
    }

    #[test]
    fn format_dry_run_result_strips_control_chars() {
        let result = WhichResult::Expanded {
            key: "k\x1bey".to_string(),
            expansion: "ex\x07pand".to_string(),
            rule_index: 0,
            satisfied_conditions: vec!["cmd\x1b[0m".to_string()],
            skipped: vec![],
        };
        let s = format_dry_run_result("tok", &result);
        assert!(!s.contains('\x1b'), "format_dry_run_result: ESC must be stripped: {s:?}");
        assert!(!s.contains('\x07'), "format_dry_run_result: BEL must be stripped: {s:?}");
    }

    #[test]
    fn format_dry_run_no_match() {
        let config = runex_core::model::Config {
            version: 1,
            keybind: runex_core::model::KeybindConfig::default(),
            abbr: vec![],
        };
        let result = expand::which_abbr(&config, "xyz", |_| true);
        let out = format_dry_run_result("xyz", &result);
        assert!(out.contains("token: xyz"));
        assert!(out.contains("no rule matched"));
        assert!(out.contains("pass-through"));
    }

    #[test]
    fn format_dry_run_expanded() {
        let config = runex_core::model::Config {
            version: 1,
            keybind: runex_core::model::KeybindConfig::default(),
            abbr: vec![runex_core::model::Abbr {
                key: "gcm".into(),
                expand: "git commit -m".into(),
                when_command_exists: None,
            }],
        };
        let result = expand::which_abbr(&config, "gcm", |_| true);
        let out = format_dry_run_result("gcm", &result);
        assert!(out.contains("token: gcm"));
        assert!(out.contains("expanded  ->  git commit -m"));
        assert!(out.contains("conditions: none"));
    }

    #[test]
    fn format_dry_run_condition_failed() {
        let config = runex_core::model::Config {
            version: 1,
            keybind: runex_core::model::KeybindConfig::default(),
            abbr: vec![runex_core::model::Abbr {
                key: "ls".into(),
                expand: "lsd".into(),
                when_command_exists: Some(vec!["lsd".into()]),
            }],
        };
        let result = expand::which_abbr(&config, "ls", |_| false);
        let out = format_dry_run_result("ls", &result);
        assert!(out.contains("lsd: NOT FOUND"), "out: {out}");
        assert!(out.contains("pass-through"), "out: {out}");
    }

    #[test]
    fn format_dry_run_duplicate_key_fallthrough() {
        let config = runex_core::model::Config {
            version: 1,
            keybind: runex_core::model::KeybindConfig::default(),
            abbr: vec![
                runex_core::model::Abbr {
                    key: "ls".into(),
                    expand: "ls".into(),
                    when_command_exists: None,
                },
                runex_core::model::Abbr {
                    key: "ls".into(),
                    expand: "lsd".into(),
                    when_command_exists: None,
                },
            ],
        };
        let result = expand::which_abbr(&config, "ls", |_| true);
        let out = format_dry_run_result("ls", &result);
        assert!(out.contains("rule #1 skipped"), "out: {out}");
        assert!(out.contains("expanded  ->  lsd"), "out: {out}");
    }
}
