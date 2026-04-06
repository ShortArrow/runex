use std::path::Path;

use crate::model::{Config, TriggerKey};
use crate::sanitize::{sanitize_for_display, sanitize_multiline_for_display};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    /// Full detail shown only with --verbose. None when same as detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_verbose: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagResult {
    pub checks: Vec<Check>,
}

impl DiagResult {
    pub fn is_healthy(&self) -> bool {
        self.checks.iter().all(|c| c.status != CheckStatus::Error)
    }
}

fn check_config_file(config_path: &Path) -> Check {
    let exists = config_path.exists();
    Check {
        name: "config_file".into(),
        status: if exists { CheckStatus::Ok } else { CheckStatus::Error },
        detail: if exists {
            format!("found: {}", sanitize_for_display(&config_path.display().to_string()))
        } else {
            format!("not found: {}", sanitize_for_display(&config_path.display().to_string()))
        },
        detail_verbose: None,
    }
}

fn check_config_parse(config: Option<&Config>, parse_error: Option<&str>) -> Check {
    let (detail, detail_verbose) = if config.is_some() {
        ("config loaded successfully".into(), None)
    } else if let Some(e) = parse_error {
        let first_line = e.lines().next().unwrap_or(e);
        let short = format!("failed to load config: {}", sanitize_for_display(first_line));
        let full = format!("failed to load config: {}", sanitize_multiline_for_display(e));
        let verbose = if full != short { Some(full) } else { None };
        (short, verbose)
    } else {
        ("failed to load config".into(), None)
    };
    Check { name: "config_parse".into(), status: if config.is_some() { CheckStatus::Ok } else { CheckStatus::Error }, detail, detail_verbose }
}

fn check_abbr_quality(config: &Config) -> Vec<Check> {
    let mut checks = Vec::new();
    for (i, abbr) in config.abbr.iter().enumerate() {
        if abbr.key.is_empty() {
            checks.push(Check {
                name: format!("abbr[{i}].empty_key"),
                status: CheckStatus::Warn,
                detail: format!("rule #{n} has an empty key — it will never match", n = i + 1),
                detail_verbose: None,
            });
        }
        if abbr.key == abbr.expand {
            checks.push(Check {
                name: format!("abbr[{i}].self_loop"),
                status: CheckStatus::Warn,
                detail: format!(
                    "rule #{n} key == expand ('{key}') — this rule is always skipped",
                    n = i + 1,
                    key = sanitize_for_display(&abbr.key)
                ),
                detail_verbose: None,
            });
        }
    }
    checks
}

fn check_when_command_exists<F>(config: &Config, command_exists: &F) -> Vec<Check>
where
    F: Fn(&str) -> bool,
{
    let mut checks = Vec::new();
    for abbr in &config.abbr {
        if let Some(cmds) = &abbr.when_command_exists {
            for cmd in cmds {
                let exists = command_exists(cmd);
                checks.push(Check {
                    name: format!("command:{}", sanitize_for_display(cmd)),
                    status: if exists { CheckStatus::Ok } else { CheckStatus::Warn },
                    detail: if exists {
                        format!("'{}' found (required by '{}')", sanitize_for_display(cmd), sanitize_for_display(&abbr.key))
                    } else {
                        format!("'{}' not found (required by '{}')", sanitize_for_display(cmd), sanitize_for_display(&abbr.key))
                    },
                    detail_verbose: None,
                });
            }
        }
    }
    checks
}

fn check_keybind(config: &Config) -> Vec<Check> {
    let mut checks = Vec::new();
    let si = &config.keybind.self_insert;
    let bash_si = si.bash.or(si.default);
    let zsh_si = si.zsh.or(si.default);
    if bash_si == Some(TriggerKey::ShiftSpace) || zsh_si == Some(TriggerKey::ShiftSpace) {
        checks.push(Check {
            name: "keybind.self_insert".into(),
            status: CheckStatus::Warn,
            detail:
                "self_insert = \"shift-space\" has no effect in bash/zsh (Shift+Space is terminal-dependent); use \"alt-space\" for cross-shell support".into(),
            detail_verbose: None,
        });
    }
    checks
}

/// Run environment diagnostics.
///
/// `config` is `None` when config loading failed (parse error, etc.).
/// `parse_error` carries the error message when `config` is `None` due to a parse failure.
/// `command_exists` is injected for testability.
pub fn diagnose<F>(config_path: &Path, config: Option<&Config>, parse_error: Option<&str>, command_exists: F) -> DiagResult
where
    F: Fn(&str) -> bool,
{
    let mut checks = Vec::new();
    checks.push(check_config_file(config_path));
    checks.push(check_config_parse(config, parse_error));
    if let Some(cfg) = config {
        checks.extend(check_keybind(cfg));
        checks.extend(check_abbr_quality(cfg));
        checks.extend(check_when_command_exists(cfg, &command_exists));
    }
    DiagResult { checks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Abbr, Config};
    use std::io::Write;

    fn test_config(abbrs: Vec<Abbr>) -> Config {
        Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: abbrs,
        }
    }

    fn abbr_when(key: &str, exp: &str, cmds: Vec<&str>) -> Abbr {
        Abbr {
            key: key.into(),
            expand: exp.into(),
            when_command_exists: Some(cmds.into_iter().map(String::from).collect()),
        }
    }

    fn abbr(key: &str, exp: &str) -> Abbr {
        Abbr { key: key.into(), expand: exp.into(), when_command_exists: None }
    }

    mod diagnostics {
        use super::*;

    #[test]
    fn all_healthy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "version = 1").unwrap();

        let cfg = test_config(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let result = diagnose(&path, Some(&cfg), None, |_| true);

        assert!(result.is_healthy());
        assert_eq!(result.checks[0].status, CheckStatus::Ok); // file exists
        assert_eq!(result.checks[1].status, CheckStatus::Ok); // config parsed
        assert_eq!(result.checks[2].status, CheckStatus::Ok); // command found
    }

    #[test]
    fn config_file_missing() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let result = diagnose(&path, None, None, |_| true);

        assert!(!result.is_healthy());
        assert_eq!(result.checks[0].status, CheckStatus::Error);
        assert_eq!(result.checks[1].status, CheckStatus::Error);
    }

    #[test]
    fn config_parse_error_detail_shown() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let result = diagnose(&path, None, Some("TOML parse error at line 4"), |_| true);

        let parse_check = result.checks.iter().find(|c| c.name == "config_parse").unwrap();
        assert_eq!(parse_check.status, CheckStatus::Error);
        assert!(parse_check.detail.contains("TOML parse error at line 4"),
            "detail must include the parse error message: {:?}", parse_check.detail);
    }

    #[test]
    fn config_parse_multiline_error_splits_detail_and_verbose() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let multiline = "TOML parse error at line 4, column 11\n  |\n4 | trigger = \"space\"\n  |           ^^^^^^^\ninvalid type";
        let result = diagnose(&path, None, Some(multiline), |_| true);

        let parse_check = result.checks.iter().find(|c| c.name == "config_parse").unwrap();
        assert_eq!(parse_check.status, CheckStatus::Error);

        let detail_lines: Vec<&str> = parse_check.detail.lines().collect();
        assert_eq!(detail_lines.len(), 1,
            "detail must be a single line, got: {:?}", parse_check.detail);
        assert!(parse_check.detail.contains("TOML parse error at line 4, column 11"),
            "detail must contain the first line: {:?}", parse_check.detail);

        let verbose = parse_check.detail_verbose.as_deref()
            .expect("detail_verbose must be Some for multiline errors");
        assert!(verbose.contains("invalid type"),
            "detail_verbose must contain later lines: {:?}", verbose);
    }

    #[test]
    fn config_parse_single_line_error_has_no_verbose() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let result = diagnose(&path, None, Some("unsupported version: 99"), |_| true);

        let parse_check = result.checks.iter().find(|c| c.name == "config_parse").unwrap();
        assert!(parse_check.detail_verbose.is_none(),
            "detail_verbose must be None when error is single-line: {:?}", parse_check.detail_verbose);
    }

    #[test]
    fn command_not_found_is_warn() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "version = 1").unwrap();

        let cfg = test_config(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let result = diagnose(&path, Some(&cfg), None, |_| false);

        assert!(result.is_healthy());
        assert_eq!(result.checks[2].status, CheckStatus::Warn);
        assert!(result.checks[2].detail.contains("not found"));
    }

    #[test]
    fn doctor_warns_empty_key() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![abbr("", "git commit -m")]);
        let result = diagnose(&path, Some(&cfg), None, |_| true);
        assert!(
            result.checks.iter().any(|c| c.name.contains("empty_key") && c.status == CheckStatus::Warn),
            "must warn on empty key: {:?}", result.checks
        );
    }

    #[test]
    fn doctor_warns_self_loop() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![abbr("ls", "ls")]);
        let result = diagnose(&path, Some(&cfg), None, |_| true);
        assert!(
            result.checks.iter().any(|c| c.name.contains("self_loop") && c.status == CheckStatus::Warn),
            "must warn on self-loop: {:?}", result.checks
        );
    }

    #[test]
    fn diag_result_is_healthy_with_error() {
        let result = DiagResult {
            checks: vec![Check {
                name: "test".into(),
                status: CheckStatus::Error,
                detail: "bad".into(),
                detail_verbose: None,
            }],
        };
        assert!(!result.is_healthy());
    }

    #[test]
    fn doctor_warns_shift_space_self_insert() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    bash: Some(crate::model::TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            abbr: vec![],
        };
        let result = diagnose(&path, Some(&cfg), None, |_| true);
        assert!(
            result.checks.iter().any(|c| c.name == "keybind.self_insert" && c.status == CheckStatus::Warn),
            "must warn when self_insert.bash = shift-space: {:?}", result.checks
        );
    }

    #[test]
    fn doctor_ok_alt_space_self_insert() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    pwsh: Some(crate::model::TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            abbr: vec![],
        };
        let result = diagnose(&path, Some(&cfg), None, |_| true);
        assert!(
            !result.checks.iter().any(|c| c.name == "keybind.self_insert" && c.status == CheckStatus::Warn),
            "must not warn when only self_insert.pwsh = shift-space: {:?}", result.checks
        );
    }

    #[test]
    fn doctor_warns_when_default_self_insert_is_shift_space() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    default: Some(crate::model::TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            abbr: vec![],
        };
        let result = diagnose(&path, Some(&cfg), None, |_| true);
        assert!(
            result.checks.iter().any(|c| c.name == "keybind.self_insert" && c.status == CheckStatus::Warn),
            "must warn when default self_insert = shift-space (propagates to bash/zsh): {:?}", result.checks
        );
    }

    #[test]
    fn doctor_ok_when_only_pwsh_self_insert_is_shift_space() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    pwsh: Some(crate::model::TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            abbr: vec![],
        };
        let result = diagnose(&path, Some(&cfg), None, |_| true);
        assert!(
            !result.checks.iter().any(|c| c.name == "keybind.self_insert" && c.status == CheckStatus::Warn),
            "must not warn when only pwsh self_insert = shift-space: {:?}", result.checks
        );
    }

    } // mod diagnostics

    /// Detail strings embed user-controlled values (keys, cmd names, config paths).
    /// If these contain ANSI escape sequences or other control characters, they will
    /// be printed raw to the terminal — enabling screen clearing, cursor movement,
    /// or other terminal injection attacks. All detail and name fields must be sanitized.
    mod sanitization {
        use super::*;

    /// A key containing a BEL (\x07) control character (valid TOML via \uXXXX escape)
    /// must not appear raw in the detail string, which is printed to the terminal.
    #[test]
    fn doctor_self_loop_detail_strips_control_chars_from_key() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![abbr("key\x07evil", "key\x07evil")]);
        let result = diagnose(&path, Some(&cfg), None, |_| true);
        let self_loop = result.checks.iter().find(|c| c.name.contains("self_loop"));
        let check = self_loop.expect("must produce a self_loop check for a self-loop key");
        assert!(
            !check.detail.contains('\x07'),
            "detail must not contain raw control char BEL: {:?}", check.detail
        );
    }

    /// A cmd in `when_command_exists` containing a control char must not appear raw in detail.
    #[test]
    fn doctor_command_check_detail_strips_control_chars_from_cmd() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![crate::model::Abbr {
            key: "ls".into(),
            expand: "lsd".into(),
            when_command_exists: Some(vec!["cmd\x07inject".into()]),
        }]);
        let result = diagnose(&path, Some(&cfg), None, |_| false);
        let cmd_check = result.checks.iter().find(|c| c.name.contains("command:"));
        let check = cmd_check.expect("must produce a command check");
        assert!(
            !check.detail.contains('\x07'),
            "detail must not contain raw control char from cmd: {:?}", check.detail
        );
    }

    /// `--config` path containing ANSI escape sequences must not appear raw in
    /// the `config_file` check detail. Attack: a path with ESC sequences could clear the screen.
    #[test]
    fn doctor_config_file_detail_strips_control_chars_from_path() {
        let path = std::path::PathBuf::from("/home/user/\x1b[2Jevil.toml");
        let result = diagnose(&path, None, None, |_| true);
        let config_check = result.checks.iter().find(|c| c.name == "config_file");
        let check = config_check.expect("must produce a config_file check");
        assert!(
            !check.detail.contains('\x1b'),
            "config_file detail must not contain raw ESC from path: {:?}", check.detail
        );
    }

    /// The `name` field `"command:{cmd}"` is printed to the terminal.
    /// A cmd containing ANSI escape sequences (e.g. ESC+`[2J` = clear screen) must
    /// not appear raw in `check.name` — sanitized the same way as `detail`.
    #[test]
    fn doctor_command_check_name_strips_control_chars() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![crate::model::Abbr {
            key: "ls".into(),
            expand: "lsd".into(),
            when_command_exists: Some(vec!["cmd\x1b[2Jevil".into()]),
        }]);
        let result = diagnose(&path, Some(&cfg), None, |_| false);
        let cmd_check = result.checks.iter().find(|c| c.name.starts_with("command:"));
        let check = cmd_check.expect("must produce a command check");
        assert!(
            !check.name.contains('\x1b'),
            "check.name must not contain raw ESC (ANSI injection risk): {:?}", check.name
        );
    }

    } // mod sanitization
}
