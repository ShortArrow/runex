use serde::{Deserialize, Serialize};

/// Identifies which shell is running.
///
/// This lives in `model` (not `shell`) to avoid a circular dependency:
/// `shell.rs` uses `model::Config`/`TriggerKey`, and `model.rs` needs `Shell`
/// for the per-shell expand/condition helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Pwsh,
    Clink,
    Nu,
}

/// Expansion string that can be uniform across all shells or per-shell.
///
/// ```toml
/// expand = "lsd"                              # All — same for every shell
/// expand = { default = "7z", pwsh = "7z.exe" } # ByShell — per-shell override
/// ```
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PerShellString {
    /// Same expansion for every shell.
    All(String),
    /// Per-shell overrides; `default` is the fallback.
    ByShell {
        default: Option<String>,
        bash:    Option<String>,
        zsh:     Option<String>,
        pwsh:    Option<String>,
        nu:      Option<String>,
    },
}

impl PerShellString {
    /// Return the expansion string for `shell`, or `None` if no entry applies.
    ///
    /// For `ByShell`, the shell-specific field takes priority over `default`.
    /// `Shell::Clink` always uses `default` (no clink-specific field).
    /// Returns `None` when neither the shell-specific field nor `default` is set.
    pub fn for_shell(&self, shell: Shell) -> Option<&str> {
        match self {
            PerShellString::All(s) => Some(s.as_str()),
            PerShellString::ByShell { default, bash, zsh, pwsh, nu } => {
                let specific = match shell {
                    Shell::Bash  => bash.as_deref(),
                    Shell::Zsh   => zsh.as_deref(),
                    Shell::Pwsh  => pwsh.as_deref(),
                    Shell::Nu    => nu.as_deref(),
                    Shell::Clink => None, // clink has no dedicated field
                };
                specific.or(default.as_deref())
            }
        }
    }

    /// Iterate over all non-None string values (used for validation).
    pub fn all_values(&self) -> Vec<&str> {
        match self {
            PerShellString::All(s) => vec![s.as_str()],
            PerShellString::ByShell { default, bash, zsh, pwsh, nu } => {
                [default, bash, zsh, pwsh, nu]
                    .iter()
                    .filter_map(|v| v.as_deref())
                    .collect()
            }
        }
    }
}

/// `when_command_exists` list that can be uniform or per-shell.
///
/// ```toml
/// when_command_exists = ["lsd"]
/// when_command_exists = { default = ["7z"], pwsh = ["7z.exe"] }
/// ```
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PerShellCmds {
    /// Same command list for every shell.
    All(Vec<String>),
    /// Per-shell overrides; `default` is the fallback.
    ByShell {
        default: Option<Vec<String>>,
        bash:    Option<Vec<String>>,
        zsh:     Option<Vec<String>>,
        pwsh:    Option<Vec<String>>,
        nu:      Option<Vec<String>>,
    },
}

impl PerShellCmds {
    /// Return the command list for `shell`, or `None` if no entry applies.
    pub fn for_shell(&self, shell: Shell) -> Option<&[String]> {
        match self {
            PerShellCmds::All(v) => Some(v.as_slice()),
            PerShellCmds::ByShell { default, bash, zsh, pwsh, nu } => {
                let specific: Option<&Vec<String>> = match shell {
                    Shell::Bash  => bash.as_ref(),
                    Shell::Zsh   => zsh.as_ref(),
                    Shell::Pwsh  => pwsh.as_ref(),
                    Shell::Nu    => nu.as_ref(),
                    Shell::Clink => None,
                };
                specific.or(default.as_ref()).map(Vec::as_slice)
            }
        }
    }

    /// Iterate over all non-None vec values (used for validation).
    pub fn all_values(&self) -> Vec<&[String]> {
        match self {
            PerShellCmds::All(v) => vec![v.as_slice()],
            PerShellCmds::ByShell { default, bash, zsh, pwsh, nu } => {
                [default, bash, zsh, pwsh, nu]
                    .iter()
                    .filter_map(|v| v.as_deref())
                    .collect()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TriggerKey {
    #[default]
    Space,
    Tab,
    AltSpace,
    ShiftSpace,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct PerShellKey {
    pub default: Option<TriggerKey>,
    pub bash: Option<TriggerKey>,
    pub zsh: Option<TriggerKey>,
    pub pwsh: Option<TriggerKey>,
    pub nu: Option<TriggerKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct KeybindConfig {
    #[serde(default)]
    pub trigger: PerShellKey,
    #[serde(default)]
    pub self_insert: PerShellKey,
}

/// A single abbreviation rule: rune → cast.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Abbr {
    pub key: String,
    pub expand: PerShellString,
    pub when_command_exists: Option<PerShellCmds>,
}

/// Top-level configuration.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub keybind: KeybindConfig,
    #[serde(default)]
    pub abbr: Vec<Abbr>,
}

/// Result of an expand operation.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpandResult {
    /// Token was expanded. `cursor_offset` is the byte position within `text`
    /// where the cursor should be placed (from the `{}` placeholder).
    /// `None` means cursor goes to end of expansion (default).
    Expanded { text: String, cursor_offset: Option<usize> },
    PassThrough(String),
}

/// Cursor placeholder marker in expansion text.
pub const CURSOR_PLACEHOLDER: &str = "{}";

#[cfg(test)]
mod tests {
    use super::*;
    use super::Shell;

    // ── PerShellString ──────────────────────────────────────────────────────

    #[test]
    fn per_shell_string_all_always_returns_value() {
        let v = PerShellString::All("lsd".into());
        assert_eq!(v.for_shell(Shell::Bash), Some("lsd"));
        assert_eq!(v.for_shell(Shell::Pwsh), Some("lsd"));
        assert_eq!(v.for_shell(Shell::Nu),   Some("lsd"));
    }

    #[test]
    fn per_shell_string_for_shell_returns_shell_specific() {
        let v = PerShellString::ByShell {
            default: Some("7z".into()),
            pwsh:    Some("7z.exe".into()),
            bash: None, zsh: None, nu: None,
        };
        assert_eq!(v.for_shell(Shell::Pwsh), Some("7z.exe"));
        assert_eq!(v.for_shell(Shell::Bash), Some("7z")); // default fallback
        assert_eq!(v.for_shell(Shell::Nu),   Some("7z"));
    }

    #[test]
    fn per_shell_string_none_when_no_entry() {
        let v = PerShellString::ByShell {
            default: None,
            pwsh:    Some("7z.exe".into()),
            bash: None, zsh: None, nu: None,
        };
        assert_eq!(v.for_shell(Shell::Bash), None); // no default, no bash
        assert_eq!(v.for_shell(Shell::Pwsh), Some("7z.exe"));
    }

    #[test]
    fn per_shell_string_clink_uses_default() {
        let v = PerShellString::ByShell {
            default: Some("cmd".into()),
            bash: None, zsh: None, pwsh: None, nu: None,
        };
        assert_eq!(v.for_shell(Shell::Clink), Some("cmd"));
    }

    // ── PerShellCmds ────────────────────────────────────────────────────────

    #[test]
    fn per_shell_cmds_all_always_returns_value() {
        let v = PerShellCmds::All(vec!["lsd".into()]);
        assert_eq!(v.for_shell(Shell::Bash), Some(["lsd".to_string()].as_slice()));
        assert_eq!(v.for_shell(Shell::Pwsh), Some(["lsd".to_string()].as_slice()));
    }

    #[test]
    fn per_shell_cmds_for_shell_returns_shell_specific() {
        let v = PerShellCmds::ByShell {
            default: Some(vec!["7z".into()]),
            pwsh:    Some(vec!["7z.exe".into()]),
            bash: None, zsh: None, nu: None,
        };
        assert_eq!(v.for_shell(Shell::Pwsh), Some(["7z.exe".to_string()].as_slice()));
        assert_eq!(v.for_shell(Shell::Bash), Some(["7z".to_string()].as_slice()));
    }

    #[test]
    fn per_shell_cmds_none_when_no_entry() {
        let v = PerShellCmds::ByShell {
            default: None,
            pwsh:    Some(vec!["7z.exe".into()]),
            bash: None, zsh: None, nu: None,
        };
        assert_eq!(v.for_shell(Shell::Bash), None);
        assert_eq!(v.for_shell(Shell::Pwsh), Some(["7z.exe".to_string()].as_slice()));
    }

    #[test]
    fn abbr_fields() {
        let a = Abbr {
            key: "gcm".into(),
            expand: PerShellString::All("git commit -m".into()),
            when_command_exists: None,
        };
        assert_eq!(a.key, "gcm");
        assert_eq!(a.expand, PerShellString::All("git commit -m".into()));
        assert!(a.when_command_exists.is_none());
    }

    #[test]
    fn abbr_with_when_command_exists() {
        let a = Abbr {
            key: "ls".into(),
            expand: PerShellString::All("lsd".into()),
            when_command_exists: Some(PerShellCmds::All(vec!["lsd".into()])),
        };
        match a.when_command_exists.unwrap() {
            PerShellCmds::All(v) => assert_eq!(v, vec!["lsd".to_string()]),
            _ => panic!("expected All"),
        }
    }

    #[test]
    fn config_fields() {
        let c = Config {
            version: 1,
            keybind: KeybindConfig::default(),
            abbr: vec![],
        };
        assert_eq!(c.version, 1);
        assert_eq!(c.keybind, KeybindConfig::default());
        assert!(c.abbr.is_empty());
    }

    #[test]
    fn keybind_config_fields() {
        let k = KeybindConfig {
            trigger: PerShellKey {
                default: Some(TriggerKey::Space),
                bash: Some(TriggerKey::AltSpace),
                zsh: Some(TriggerKey::Space),
                pwsh: Some(TriggerKey::Tab),
                nu: None,
            },
            self_insert: PerShellKey::default(),
        };
        assert_eq!(k.trigger.default, Some(TriggerKey::Space));
        assert_eq!(k.trigger.bash, Some(TriggerKey::AltSpace));
        assert_eq!(k.trigger.zsh, Some(TriggerKey::Space));
        assert_eq!(k.trigger.pwsh, Some(TriggerKey::Tab));
        assert_eq!(k.trigger.nu, None);
        assert_eq!(k.self_insert, PerShellKey::default());
    }

    #[test]
    fn parse_config_accepts_self_insert_shift_space() {
        let toml = r#"
version = 1
[keybind.self_insert]
pwsh = "shift-space"
"#;
        let config: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(
            config.keybind.self_insert.pwsh,
            Some(TriggerKey::ShiftSpace),
            "self_insert.pwsh should deserialize to ShiftSpace"
        );
    }

    #[test]
    fn parse_config_keybind_entirely_absent() {
        let toml = "version = 1\n";
        let config: Config = toml::from_str(toml).expect("should parse");
        assert_eq!(config.keybind.trigger, PerShellKey::default());
        assert_eq!(config.keybind.self_insert, PerShellKey::default());
    }

    #[test]
    fn expand_result_variants() {
        let expanded = ExpandResult::Expanded { text: "git commit -m".into(), cursor_offset: None };
        let pass = ExpandResult::PassThrough("unknown".into());
        assert_eq!(expanded, ExpandResult::Expanded { text: "git commit -m".into(), cursor_offset: None });
        assert_eq!(pass, ExpandResult::PassThrough("unknown".into()));
    }
}
