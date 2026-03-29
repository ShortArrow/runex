use std::path::Path;

use crate::model::Config;
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

/// Run environment diagnostics.
///
/// `config` is `None` when config loading failed (parse error, etc.).
/// `command_exists` is injected for testability.
pub fn diagnose<F>(config_path: &Path, config: Option<&Config>, command_exists: F) -> DiagResult
where
    F: Fn(&str) -> bool,
{
    let mut checks = Vec::new();

    // 1. Config file exists
    let config_exists = config_path.exists();
    checks.push(Check {
        name: "config_file".into(),
        status: if config_exists {
            CheckStatus::Ok
        } else {
            CheckStatus::Error
        },
        detail: if config_exists {
            format!("found: {}", config_path.display())
        } else {
            format!("not found: {}", config_path.display())
        },
    });

    // 2. Config parse
    checks.push(Check {
        name: "config_parse".into(),
        status: if config.is_some() {
            CheckStatus::Ok
        } else {
            CheckStatus::Error
        },
        detail: if config.is_some() {
            "config loaded successfully".into()
        } else {
            "failed to load config".into()
        },
    });

    // 3. Check abbr rule quality: empty key, self-loop
    if let Some(cfg) = config {
        for (i, abbr) in cfg.abbr.iter().enumerate() {
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
                        key = abbr.key
                    ),
                });
            }
        }
    }

    // 4. Check when_command_exists commands
    if let Some(cfg) = config {
        for abbr in &cfg.abbr {
            if let Some(cmds) = &abbr.when_command_exists {
                for cmd in cmds {
                    let exists = command_exists(cmd);
                    checks.push(Check {
                        name: format!("command:{cmd}"),
                        status: if exists {
                            CheckStatus::Ok
                        } else {
                            CheckStatus::Warn
                        },
                        detail: if exists {
                            format!("'{cmd}' found (required by '{}')", abbr.key)
                        } else {
                            format!("'{cmd}' not found (required by '{}')", abbr.key)
                        },
                    });
                }
            }
        }
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
}
