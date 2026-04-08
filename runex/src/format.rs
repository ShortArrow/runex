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

pub(crate) fn format_check_line(check: &Check, verbose: bool) -> String {
    let detail = if verbose {
        check.detail_verbose.as_deref().unwrap_or(&check.detail)
    } else {
        &check.detail
    };
    format!(
        "{:>CHECK_TAG_WIDTH$}  {}: {}",
        format_check_tag(&check.status),
        check.name,
        detail
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
        expand::SkipReason::NoShellEntry => {
            format!("\n  rule #{} skipped: no expand entry for current shell", i + 1)
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
    let has_no_shell_entry = skipped
        .iter()
        .any(|(_, r)| matches!(r, expand::SkipReason::NoShellEntry));
    match (has_condition_fail, has_self_loop, has_no_shell_entry) {
        (true, _, _) => {
            let all_missing = collect_all_missing_commands(skipped);
            if all_missing.is_empty() {
                format!("{token}  [skipped: condition failed]")
            } else {
                format!("{token}  [skipped: {} not found]", all_missing.join(", "))
            }
        }
        (false, true, false) => format!("{token}  [no-op: key and expansion are identical]"),
        (false, false, true) => format!("{token}  [skipped: no entry for current shell]"),
        (false, true, true) => format!("{token}  [skipped: self-loop or no entry for current shell]"),
        (false, false, false) => format!("{token}: no rule found"),
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
                    expand::SkipReason::NoShellEntry => {
                        out.push_str(&format!("rule #{} skipped: no entry for current shell\n", i + 1));
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
                    expand::SkipReason::NoShellEntry => {
                        out.push_str(&format!("rule #{} skipped: no entry for current shell\n", i + 1));
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

pub(crate) fn format_duration(d: std::time::Duration) -> String {
    let us = d.as_micros();
    if us < 1_000 {
        format!("{us}us")
    } else if us < 1_000_000 {
        format!("{:.2}ms", us as f64 / 1_000.0)
    } else {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    }
}

pub(crate) fn format_timings_table(timings: &runex_core::timings::Timings) -> String {
    let mut out = String::new();
    out.push_str(&format!(" {:<28} {}\n", "Phase", "Duration"));
    out.push_str(&format!(" {}\n", "─".repeat(38)));

    for phase in timings.phases() {
        out.push_str(&format!(" {:<28} {}\n", phase.name, format_duration(phase.duration)));
    }
    for call in timings.command_exists_calls() {
        let label = format!("  command_exists: {}", call.command);
        out.push_str(&format!(" {:<28} {}\n", label, format_duration(call.duration)));
    }

    out.push_str(&format!(" {}\n", "─".repeat(38)));
    out.push_str(&format!(" {:<28} {}\n", "Total", format_duration(timings.total_duration())));
    out
}

pub(crate) fn format_timings_json(timings: &runex_core::timings::Timings) -> serde_json::Value {
    let phases: Vec<serde_json::Value> = timings.phases().iter().map(|p| {
        serde_json::json!({
            "name": p.name,
            "duration_us": p.duration.as_micros() as u64,
        })
    }).collect();

    let calls: Vec<serde_json::Value> = timings.command_exists_calls().iter().map(|c| {
        serde_json::json!({
            "command": c.command,
            "found": c.found,
            "duration_us": c.duration.as_micros() as u64,
        })
    }).collect();

    serde_json::json!({
        "phases": phases,
        "command_exists_calls": calls,
        "total_us": timings.total_duration().as_micros() as u64,
    })
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
            detail_verbose: None,
        };

        let line = format_check_line(&check, false);
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

    fn make_abbr(key: &str, exp: &str) -> runex_core::model::Abbr {
        runex_core::model::Abbr {
            key: key.into(),
            expand: runex_core::model::PerShellString::All(exp.into()),
            when_command_exists: None,
        }
    }

    fn make_abbr_when(key: &str, exp: &str, cmds: Vec<&str>) -> runex_core::model::Abbr {
        runex_core::model::Abbr {
            key: key.into(),
            expand: runex_core::model::PerShellString::All(exp.into()),
            when_command_exists: Some(runex_core::model::PerShellCmds::All(
                cmds.into_iter().map(String::from).collect(),
            )),
        }
    }

    #[test]
    fn format_dry_run_no_match() {
        let config = runex_core::model::Config {
            version: 1,
            keybind: runex_core::model::KeybindConfig::default(),
            abbr: vec![],
        };
        let result = expand::which_abbr(&config, "xyz", runex_core::shell::Shell::Bash, |_| true);
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
            abbr: vec![make_abbr("gcm", "git commit -m")],
        };
        let result = expand::which_abbr(&config, "gcm", runex_core::shell::Shell::Bash, |_| true);
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
            abbr: vec![make_abbr_when("ls", "lsd", vec!["lsd"])],
        };
        let result = expand::which_abbr(&config, "ls", runex_core::shell::Shell::Bash, |_| false);
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
                make_abbr("ls", "ls"),
                make_abbr("ls", "lsd"),
            ],
        };
        let result = expand::which_abbr(&config, "ls", runex_core::shell::Shell::Bash, |_| true);
        let out = format_dry_run_result("ls", &result);
        assert!(out.contains("rule #1 skipped"), "out: {out}");
        assert!(out.contains("expanded  ->  lsd"), "out: {out}");
    }

    // ── timings formatting tests ────────────────────────────────────────

    use runex_core::timings::Timings;
    use std::time::Duration;

    #[test]
    fn format_duration_units() {
        assert_eq!(format_duration(Duration::from_micros(500)), "500us");
        assert_eq!(format_duration(Duration::from_micros(1500)), "1.50ms");
        assert_eq!(format_duration(Duration::from_micros(1_500_000)), "1.50s");
    }

    #[test]
    fn format_timings_table_shows_phases() {
        let mut t = Timings::new();
        t.record_phase("config_load", Duration::from_micros(1230));
        t.record_phase("expand", Duration::from_micros(5670));
        let out = format_timings_table(&t);
        assert!(out.contains("config_load"), "out: {out}");
        assert!(out.contains("expand"), "out: {out}");
        assert!(out.contains("Total"), "out: {out}");
    }

    #[test]
    fn format_timings_table_shows_command_exists_indented() {
        let mut t = Timings::new();
        t.record_phase("expand", Duration::from_micros(5670));
        t.record_command_exists("git", true, Duration::from_micros(2340));
        let out = format_timings_table(&t);
        assert!(out.contains("  command_exists: git"), "cmd call must be indented: {out}");
    }

    #[test]
    fn format_timings_json_structure() {
        let mut t = Timings::new();
        t.record_phase("config_load", Duration::from_micros(1230));
        t.record_command_exists("git", true, Duration::from_micros(2340));
        let v = format_timings_json(&t);
        assert!(v.get("phases").unwrap().is_array());
        assert!(v.get("command_exists_calls").unwrap().is_array());
        assert!(v.get("total_us").unwrap().is_number());
        let phase = &v["phases"][0];
        assert_eq!(phase["name"], "config_load");
        assert_eq!(phase["duration_us"], 1230);
    }
}
