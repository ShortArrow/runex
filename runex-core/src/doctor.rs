use std::path::Path;

use crate::model::Config;
use crate::sanitize::sanitize_for_display;
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
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagResult {
    pub checks: Vec<Check>,
}

impl DiagResult {
    pub fn is_healthy(&self) -> bool {
        self.checks
            .iter()
            .all(|c| c.status == CheckStatus::Ok || c.status == CheckStatus::Warn)
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
    }
}

fn check_config_parse(config: Option<&Config>) -> Check {
    Check {
        name: "config_parse".into(),
        status: if config.is_some() { CheckStatus::Ok } else { CheckStatus::Error },
        detail: if config.is_some() {
            "config loaded successfully".into()
        } else {
            "failed to load config".into()
        },
    }
}

fn check_abbr_quality(config: &Config) -> Vec<Check> {
    let mut checks = Vec::new();
    for (i, abbr) in config.abbr.iter().enumerate() {
        if abbr.key.is_empty() {
            checks.push(Check {
                name: format!("abbr[{i}].empty_key"),
                status: CheckStatus::Warn,
                detail: format!("rule #{n} has an empty key — it will never match", n = i + 1),
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
                });
            }
        }
    }
    checks
}

/// Run environment diagnostics.
///
/// `config` is `None` when config loading failed (parse error, etc.).
/// `command_exists` is injected for testability.
pub fn diagnose<F>(config_path: &Path, config: Option<&Config>, command_exists: F) -> DiagResult
where
    F: Fn(&str) -> bool,
{
    let mut checks = Vec::new();
    checks.push(check_config_file(config_path));
    checks.push(check_config_parse(config));
    if let Some(cfg) = config {
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

    #[test]
    fn all_healthy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "version = 1").unwrap();

        let cfg = test_config(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let result = diagnose(&path, Some(&cfg), |_| true);

        assert!(result.is_healthy());
        assert_eq!(result.checks[0].status, CheckStatus::Ok); // file exists
        assert_eq!(result.checks[1].status, CheckStatus::Ok); // config parsed
        assert_eq!(result.checks[2].status, CheckStatus::Ok); // command found
    }

    #[test]
    fn config_file_missing() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let result = diagnose(&path, None, |_| true);

        assert!(!result.is_healthy());
        assert_eq!(result.checks[0].status, CheckStatus::Error);
        assert_eq!(result.checks[1].status, CheckStatus::Error);
    }

    #[test]
    fn command_not_found_is_warn() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "version = 1").unwrap();

        let cfg = test_config(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let result = diagnose(&path, Some(&cfg), |_| false);

        // is_healthy returns true for Warn
        assert!(result.is_healthy());
        assert_eq!(result.checks[2].status, CheckStatus::Warn);
        assert!(result.checks[2].detail.contains("not found"));
    }

    fn abbr(key: &str, exp: &str) -> Abbr {
        Abbr { key: key.into(), expand: exp.into(), when_command_exists: None }
    }

    #[test]
    fn doctor_warns_empty_key() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![abbr("", "git commit -m")]);
        let result = diagnose(&path, Some(&cfg), |_| true);
        assert!(
            result.checks.iter().any(|c| c.name.contains("empty_key") && c.status == CheckStatus::Warn),
            "must warn on empty key: {:?}", result.checks
        );
    }

    #[test]
    fn doctor_warns_self_loop() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![abbr("ls", "ls")]);
        let result = diagnose(&path, Some(&cfg), |_| true);
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
            }],
        };
        assert!(!result.is_healthy());
    }

    // ─── detail string must not contain control characters from user-controlled input ──

    #[test]
    fn doctor_self_loop_detail_strips_control_chars_from_key() {
        // A key containing a BEL (\x07) control character (valid TOML via \uXXXX escape)
        // must not appear raw in the detail string, which is printed to the terminal.
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![abbr("key\x07evil", "key\x07evil")]);
        let result = diagnose(&path, Some(&cfg), |_| true);
        let self_loop = result.checks.iter().find(|c| c.name.contains("self_loop"));
        let check = self_loop.expect("must produce a self_loop check for a self-loop key");
        assert!(
            !check.detail.contains('\x07'),
            "detail must not contain raw control char BEL: {:?}", check.detail
        );
    }

    #[test]
    fn doctor_command_check_detail_strips_control_chars_from_cmd() {
        // A cmd in when_command_exists containing a control char must not appear raw in detail.
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![crate::model::Abbr {
            key: "ls".into(),
            expand: "lsd".into(),
            when_command_exists: Some(vec!["cmd\x07inject".into()]),
        }]);
        let result = diagnose(&path, Some(&cfg), |_| false);
        let cmd_check = result.checks.iter().find(|c| c.name.contains("command:"));
        let check = cmd_check.expect("must produce a command check");
        assert!(
            !check.detail.contains('\x07'),
            "detail must not contain raw control char from cmd: {:?}", check.detail
        );
    }

    #[test]
    fn doctor_config_file_detail_strips_control_chars_from_path() {
        // --config path containing ANSI escape sequences must not appear raw in
        // the config_file check detail, which is printed to the terminal.
        // Attack: --config $'\x1b[2J/evil.toml' would clear the screen.
        let path = std::path::PathBuf::from("/home/user/\x1b[2Jevil.toml");
        let result = diagnose(&path, None, |_| true);
        let config_check = result.checks.iter().find(|c| c.name == "config_file");
        let check = config_check.expect("must produce a config_file check");
        assert!(
            !check.detail.contains('\x1b'),
            "config_file detail must not contain raw ESC from path: {:?}", check.detail
        );
    }

    #[test]
    fn doctor_command_check_name_strips_control_chars() {
        // The `name` field "command:{cmd}" is printed to the terminal in human output.
        // A cmd containing ANSI escape sequences (e.g. \x1b[2J = clear screen) must
        // not appear raw in check.name — it must be sanitized the same way detail is.
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![crate::model::Abbr {
            key: "ls".into(),
            expand: "lsd".into(),
            // ESC [ 2 J = clear screen, a common terminal injection payload
            when_command_exists: Some(vec!["cmd\x1b[2Jevil".into()]),
        }]);
        let result = diagnose(&path, Some(&cfg), |_| false);
        let cmd_check = result.checks.iter().find(|c| c.name.starts_with("command:"));
        let check = cmd_check.expect("must produce a command check");
        assert!(
            !check.name.contains('\x1b'),
            "check.name must not contain raw ESC (ANSI injection risk): {:?}", check.name
        );
    }
}
