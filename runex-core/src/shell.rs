use std::fmt;
use std::str::FromStr;

use crate::model::{Config, TriggerKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Pwsh,
    Clink,
    Nu,
}

impl FromStr for Shell {
    type Err = ShellParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "bash" => Ok(Shell::Bash),
            "pwsh" => Ok(Shell::Pwsh),
            "clink" => Ok(Shell::Clink),
            "nu" => Ok(Shell::Nu),
            _ => Err(ShellParseError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellParseError(pub String);

impl fmt::Display for ShellParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown shell '{}' (expected: bash, pwsh, clink, nu)",
            self.0
        )
    }
}

impl std::error::Error for ShellParseError {}

fn trigger_for(shell: Shell, config: Option<&Config>) -> TriggerKey {
    let keybind = match config {
        Some(config) => &config.keybind,
        None => return TriggerKey::Space,
    };

    match shell {
        Shell::Bash => keybind.bash.or(keybind.trigger).unwrap_or(TriggerKey::Space),
        Shell::Pwsh => keybind.pwsh.or(keybind.trigger).unwrap_or(TriggerKey::Space),
        Shell::Nu => keybind.nu.or(keybind.trigger).unwrap_or(TriggerKey::Space),
        Shell::Clink => keybind.trigger.unwrap_or(TriggerKey::Space),
    }
}

fn bash_chord(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space => "\\x20",
        TriggerKey::Tab => "\\C-i",
        TriggerKey::AltSpace => "\\e ",
    }
}

fn bash_quote_pattern(token: &str) -> String {
    format!("'{}'", token.replace('\'', r#"'\''"#))
}

fn bash_known_cases(config: Option<&Config>) -> String {
    let Some(config) = config else {
        return "        *) return 0 ;;".to_string();
    };

    if config.abbr.is_empty() {
        return "        *) return 0 ;;".to_string();
    }

    let mut lines = Vec::with_capacity(config.abbr.len() + 1);
    for abbr in &config.abbr {
        lines.push(format!(
            "        {}) return 0 ;;",
            bash_quote_pattern(&abbr.key)
        ));
    }
    lines.push("        *) return 1 ;;".to_string());
    lines.join("\n")
}

fn pwsh_chord(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space => " ",
        TriggerKey::Tab => "Tab",
        TriggerKey::AltSpace => "Alt+Space",
    }
}

fn nu_modifier(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::AltSpace => "alt",
        TriggerKey::Space | TriggerKey::Tab => "none",
    }
}

fn nu_keycode(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space | TriggerKey::AltSpace => "space",
        TriggerKey::Tab => "tab",
    }
}

/// Generate a shell integration script.
///
/// `{BIN}` placeholders in the template are replaced with `bin`.
pub fn export_script(shell: Shell, bin: &str, config: Option<&Config>) -> String {
    let template = match shell {
        Shell::Bash => include_str!("templates/bash.sh"),
        Shell::Pwsh => include_str!("templates/pwsh.ps1"),
        Shell::Clink => include_str!("templates/clink.lua"),
        Shell::Nu => include_str!("templates/nu.nu"),
    };
    let trigger = trigger_for(shell, config);
    template
        .replace("\r\n", "\n")
        .replace("{BIN}", bin)
        .replace("{BASH_CHORD}", bash_chord(trigger))
        .replace("{BASH_KNOWN_CASES}", &bash_known_cases(config))
        .replace("{PWSH_CHORD}", pwsh_chord(trigger))
        .replace("{NU_MODIFIER}", nu_modifier(trigger))
        .replace("{NU_KEYCODE}", nu_keycode(trigger))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bash() {
        assert_eq!(Shell::from_str("bash").unwrap(), Shell::Bash);
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(Shell::from_str("PWSH").unwrap(), Shell::Pwsh);
        assert_eq!(Shell::from_str("Clink").unwrap(), Shell::Clink);
        assert_eq!(Shell::from_str("Nu").unwrap(), Shell::Nu);
    }

    #[test]
    fn parse_unknown_errors() {
        let err = Shell::from_str("fish").unwrap_err();
        assert_eq!(err.0, "fish");
    }

    #[test]
    fn export_script_contains_bin() {
        for shell in [Shell::Bash, Shell::Pwsh, Shell::Clink, Shell::Nu] {
            let script = export_script(shell, "my-runex", None);
            assert!(
                script.contains("my-runex"),
                "{shell:?} script must contain the bin name"
            );
        }
    }

    #[test]
    fn bash_script_has_bind() {
        let s = export_script(Shell::Bash, "runex", None);
        assert!(s.contains("bind"), "bash script must use bind");
        assert!(s.contains("READLINE_LINE"), "bash script must use READLINE_LINE");
        assert!(s.contains("READLINE_POINT"), "bash script must inspect the cursor");
    }

    #[test]
    fn pwsh_script_has_psreadline() {
        let s = export_script(Shell::Pwsh, "runex", None);
        assert!(s.contains("Set-PSReadLineKeyHandler"), "pwsh script must use PSReadLine");
        assert!(s.contains("$cursor -lt $line.Length"), "pwsh script must guard mid-line insertion");
        assert!(s.contains("EditMode"), "pwsh script must handle PSReadLine edit mode");
    }

    #[test]
    fn clink_script_has_clink() {
        let s = export_script(Shell::Clink, "runex", None);
        assert!(s.contains("clink"), "clink script must reference clink");
    }

    #[test]
    fn nu_script_has_keybindings() {
        let s = export_script(Shell::Nu, "runex", None);
        assert!(s.contains("keybindings"), "nu script must reference keybindings");
        assert!(s.contains("commandline get-cursor"), "nu script must inspect the cursor");
    }

    #[test]
    fn bash_script_uses_keybind_override() {
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                trigger: None,
                bash: Some(TriggerKey::AltSpace),
                pwsh: None,
                nu: None,
            },
            abbr: vec![],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(s.contains("\\e "), "bash script must use the configured key chord");
    }

    #[test]
    fn bash_script_embeds_known_tokens() {
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "gcm".into(),
                expand: "git commit -m".into(),
                when_command_exists: None,
            }],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(s.contains("'gcm') return 0 ;;"), "bash script must embed known tokens");
    }

    #[test]
    fn pwsh_script_uses_global_keybind() {
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                trigger: Some(TriggerKey::Tab),
                bash: None,
                pwsh: None,
                nu: None,
            },
            abbr: vec![],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(s.contains("Chord = 'Tab'"), "pwsh script must use the configured chord");
    }
}
