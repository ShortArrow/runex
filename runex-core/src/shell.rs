use std::fmt;
use std::str::FromStr;

use crate::model::{Config, TriggerKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Pwsh,
    Clink,
    Nu,
}

impl FromStr for Shell {
    type Err = ShellParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "bash" => Ok(Shell::Bash),
            "zsh" => Ok(Shell::Zsh),
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
            "unknown shell '{}' (expected: bash, zsh, pwsh, clink, nu)",
            self.0
        )
    }
}

impl std::error::Error for ShellParseError {}

fn trigger_for(shell: Shell, config: Option<&Config>) -> Option<TriggerKey> {
    let keybind = match config {
        Some(config) => &config.keybind,
        None => return None,
    };

    match shell {
        Shell::Bash => keybind.bash.or(keybind.trigger),
        Shell::Zsh => keybind.zsh.or(keybind.trigger),
        Shell::Pwsh => keybind.pwsh.or(keybind.trigger),
        Shell::Nu => keybind.nu.or(keybind.trigger),
        Shell::Clink => keybind.trigger,
    }
}

fn bash_chord(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space => "\\x20",
        TriggerKey::Tab => "\\C-i",
        TriggerKey::AltSpace => "\\e ",
    }
}

fn zsh_chord(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space => " ",
        TriggerKey::Tab => "^I",
        TriggerKey::AltSpace => "^[ ",
    }
}

fn bash_quote_pattern(token: &str) -> String {
    format!("'{}'", token.replace('\'', r#"'\''"#))
}

fn bash_quote_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
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

fn zsh_known_cases(config: Option<&Config>) -> String {
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
        TriggerKey::AltSpace => "Alt+Spacebar",
    }
}

fn pwsh_quote_string(token: &str) -> String {
    format!("'{}'", token.replace('\'', "''"))
}

fn lua_quote_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn pwsh_known_cases(config: Option<&Config>) -> String {
    let Some(config) = config else {
        return "        default { return $true }".to_string();
    };

    if config.abbr.is_empty() {
        return "        default { return $true }".to_string();
    }

    let mut lines = Vec::with_capacity(config.abbr.len());
    for abbr in &config.abbr {
        lines.push(format!(
            "        {} {{ return $true }}",
            pwsh_quote_string(&abbr.key)
        ));
    }
    lines.join("\n")
}

fn nu_modifier(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::AltSpace => "alt",
        TriggerKey::Space | TriggerKey::Tab => "none",
    }
}

fn clink_known_cases(config: Option<&Config>) -> String {
    let Some(config) = config else {
        return String::new();
    };

    config
        .abbr
        .iter()
        .map(|abbr| format!("    [{}] = true,", lua_quote_string(&abbr.key)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn nu_keycode(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space | TriggerKey::AltSpace => "space",
        TriggerKey::Tab => "tab",
    }
}

fn clink_key_sequence(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space => r#"" ""#,
        TriggerKey::Tab => r#""\t""#,
        TriggerKey::AltSpace => r#""\e ""#,
    }
}

fn bash_bind_lines(trigger: Option<TriggerKey>) -> String {
    let mut lines = vec![
        r#"bind -r "\x20" 2>/dev/null || true"#.to_string(),
        r#"bind -r "\C-i" 2>/dev/null || true"#.to_string(),
        r#"bind -r "\e " 2>/dev/null || true"#.to_string(),
    ];
    if let Some(trigger) = trigger {
        lines.push(format!("bind -x '\"{}\": __runex_expand'", bash_chord(trigger)));
    }
    lines.join("\n")
}

fn zsh_bind_lines(trigger: Option<TriggerKey>) -> String {
    let mut lines = vec![
        r#"bindkey -r " " 2>/dev/null"#.to_string(),
        r#"bindkey -r "^I" 2>/dev/null"#.to_string(),
        r#"bindkey -r "^[ " 2>/dev/null"#.to_string(),
    ];
    if let Some(trigger) = trigger {
        lines.push(format!(r#"bindkey "{}" __runex_expand"#, zsh_chord(trigger)));
    }
    lines.join("\n")
}

fn pwsh_register_lines(trigger: Option<TriggerKey>) -> String {
    let mut lines = vec![
        "    Set-PSReadLineKeyHandler -Chord ' ' -Function SelfInsert".to_string(),
        "    Set-PSReadLineKeyHandler -Chord 'Tab' -Function Complete".to_string(),
        "    Set-PSReadLineKeyHandler -Chord 'Alt+Spacebar' -Function SelfInsert".to_string(),
    ];
    if let Some(trigger) = trigger {
        lines.push(format!(
            "    __runex_register_expand_handler '{}'",
            pwsh_chord(trigger)
        ));
    }
    let mut vi_lines = Vec::new();
    if let Some(trigger) = trigger {
        vi_lines.push(format!(
            "        __runex_register_expand_handler '{}' Insert",
            pwsh_chord(trigger)
        ));
    }
    if !vi_lines.is_empty() {
        lines.push("    if ((Get-PSReadLineOption).EditMode -eq 'Vi') {".to_string());
        lines.extend(vi_lines);
        lines.push("    }".to_string());
    }
    lines.join("\n")
}

fn nu_bindings(trigger: Option<TriggerKey>, bin: &str) -> String {
    let mut blocks = Vec::new();
    if let Some(trigger) = trigger {
        blocks.push(
            include_str!("templates/nu_expand_binding.nu")
                .replace("{BIN}", bin)
                .replace("{NU_MODIFIER}", nu_modifier(trigger))
                .replace("{NU_KEYCODE}", nu_keycode(trigger)),
        );
    }
    blocks.join(" | append ")
}

fn clink_binding(trigger: Option<TriggerKey>) -> String {
    let Some(trigger) = trigger else {
        return String::new();
    };

    let key = clink_key_sequence(trigger);
    [
        format!(
            r#"pcall(rl.setbinding, [[{key}]], [["luafunc:runex_expand"]], "emacs")"#,
            key = key
        ),
        format!(
            r#"pcall(rl.setbinding, [[{key}]], [["luafunc:runex_expand"]], "vi-insert")"#,
            key = key
        ),
    ]
    .join("\n")
}

/// Generate a shell integration script.
///
/// `{BIN}` placeholders in the template are replaced with `bin`.
pub fn export_script(shell: Shell, bin: &str, config: Option<&Config>) -> String {
    let template = match shell {
        Shell::Bash => include_str!("templates/bash.sh"),
        Shell::Zsh => include_str!("templates/zsh.zsh"),
        Shell::Pwsh => include_str!("templates/pwsh.ps1"),
        Shell::Clink => include_str!("templates/clink.lua"),
        Shell::Nu => include_str!("templates/nu.nu"),
    };
    let trigger = trigger_for(shell, config);
    template
        .replace("\r\n", "\n")
        .replace("{BIN}", bin)
        .replace("{BASH_BIN}", &bash_quote_string(bin))
        .replace("{BASH_BIND_LINES}", &bash_bind_lines(trigger))
        .replace("{BASH_KNOWN_CASES}", &bash_known_cases(config))
        .replace("{ZSH_BIN}", &bash_quote_string(bin))
        .replace("{ZSH_BIND_LINES}", &zsh_bind_lines(trigger))
        .replace("{ZSH_KNOWN_CASES}", &zsh_known_cases(config))
        .replace("{CLINK_BIN}", &lua_quote_string(bin))
        .replace("{CLINK_BINDING}", &clink_binding(trigger))
        .replace("{CLINK_KNOWN_CASES}", &clink_known_cases(config))
        .replace("{PWSH_BIN}", &pwsh_quote_string(bin))
        .replace("{PWSH_REGISTER_LINES}", &pwsh_register_lines(trigger))
        .replace("{PWSH_KNOWN_CASES}", &pwsh_known_cases(config))
        .replace("{NU_BINDINGS}", &nu_bindings(trigger, bin))
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
        assert_eq!(Shell::from_str("Zsh").unwrap(), Shell::Zsh);
    }

    #[test]
    fn parse_unknown_errors() {
        let err = Shell::from_str("fish").unwrap_err();
        assert_eq!(err.0, "fish");
    }

    #[test]
    fn export_script_contains_bin() {
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                trigger: Some(TriggerKey::Space),
                ..crate::model::KeybindConfig::default()
            },
            abbr: vec![],
        };
        for shell in [Shell::Bash, Shell::Zsh, Shell::Pwsh, Shell::Clink, Shell::Nu] {
            let script = export_script(shell, "my-runex", Some(&config));
            assert!(
                script.contains("my-runex"),
                "{shell:?} script must contain the bin name"
            );
        }
    }

    #[test]
    fn bash_script_has_bind() {
        let s = export_script(
            Shell::Bash,
            "runex",
            Some(&Config {
                version: 1,
                keybind: crate::model::KeybindConfig {
                    trigger: Some(TriggerKey::Space),
                    ..crate::model::KeybindConfig::default()
                },
                abbr: vec![],
            }),
        );
        assert!(s.contains("bind -x"), "bash script must use bind");
        assert!(s.contains(r#"bind -r "\x20""#), "bash script must clean up prior bindings");
        assert!(s.contains("expanded=$('runex' expand"), "bash script must quote the executable");
        assert!(s.contains("READLINE_LINE"), "bash script must use READLINE_LINE");
        assert!(s.contains("READLINE_POINT"), "bash script must inspect the cursor");
        assert!(!s.contains("{BASH_BIND_LINES}"), "bash script must resolve bind lines");
    }

    #[test]
    fn pwsh_script_has_psreadline() {
        let s = export_script(
            Shell::Pwsh,
            "runex",
            Some(&Config {
                version: 1,
                keybind: crate::model::KeybindConfig {
                    trigger: Some(TriggerKey::Space),
                    ..crate::model::KeybindConfig::default()
                },
                abbr: vec![],
            }),
        );
        assert!(s.contains("Set-PSReadLineKeyHandler"), "pwsh script must use PSReadLine");
        assert!(
            s.contains("Set-PSReadLineKeyHandler -Chord 'Tab' -Function Complete"),
            "pwsh script must restore default handlers before adding custom ones"
        );
        assert!(s.contains("$expanded = & 'runex' expand"), "pwsh script must quote the executable");
        assert!(s.contains("$cursor -lt $line.Length"), "pwsh script must guard mid-line insertion");
        assert!(s.contains("EditMode"), "pwsh script must handle PSReadLine edit mode");
        assert!(s.contains("__runex_is_command_position"), "pwsh script must detect command position");
        assert!(!s.contains("{PWSH_REGISTER_LINES}"), "pwsh script must resolve register lines");
    }

    #[test]
    fn zsh_script_has_zle_widget() {
        let s = export_script(
            Shell::Zsh,
            "runex",
            Some(&Config {
                version: 1,
                keybind: crate::model::KeybindConfig {
                    trigger: Some(TriggerKey::Space),
                    ..crate::model::KeybindConfig::default()
                },
                abbr: vec![],
            }),
        );
        assert!(s.contains("zle -N __runex_expand"), "zsh script must register a zle widget");
        assert!(s.contains(r#"bindkey " " __runex_expand"#), "zsh script must bind the trigger key");
        assert!(s.contains("__runex_expand_buffer"), "zsh script must expose a testable helper");
        assert!(s.contains("LBUFFER"), "zsh script must inspect the text before the cursor");
        assert!(s.contains("RBUFFER"), "zsh script must inspect the text after the cursor");
        assert!(s.contains("expanded=$('runex' expand"), "zsh script must quote the executable");
    }

    #[test]
    fn clink_script_has_clink() {
        let s = export_script(
            Shell::Clink,
            "runex",
            Some(&Config {
                version: 1,
                keybind: crate::model::KeybindConfig {
                    trigger: Some(TriggerKey::Space),
                    ..crate::model::KeybindConfig::default()
                },
                abbr: vec![],
            }),
        );
        assert!(s.contains("clink"), "clink script must reference clink");
        assert!(s.contains("local RUNEX_BIN = \"runex\""), "clink script must quote the executable");
        assert!(s.contains("local RUNEX_KNOWN = {"), "clink script must embed known keys");
        assert!(s.contains(r#"pcall(rl.setbinding, [[" "]], [["luafunc:runex_expand"]], "emacs")"#), "clink script must bind the trigger key in emacs mode");
        assert!(s.contains(r#"pcall(rl.setbinding, [[" "]], [["luafunc:runex_expand"]], "vi-insert")"#), "clink script must bind the trigger key in vi insert mode");
        assert!(s.contains("rl_buffer:getcursor()"), "clink script must inspect the cursor");
        assert!(!s.contains("clink.onfilterinput"), "clink script must not use onfilterinput for realtime expansion");
    }

    #[test]
    fn clink_script_uses_alt_space_sequence() {
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                trigger: Some(TriggerKey::AltSpace),
                ..crate::model::KeybindConfig::default()
            },
            abbr: vec![],
        };
        let s = export_script(Shell::Clink, "runex", Some(&config));
        assert!(
            s.contains(r#"pcall(rl.setbinding, [["\e "]], [["luafunc:runex_expand"]], "emacs")"#),
            "clink script must use the alt-space sequence"
        );
    }

    #[test]
    fn nu_script_has_keybindings() {
        let s = export_script(
            Shell::Nu,
            "runex",
            Some(&Config {
                version: 1,
                keybind: crate::model::KeybindConfig {
                    trigger: Some(TriggerKey::Space),
                    ..crate::model::KeybindConfig::default()
                },
                abbr: vec![],
            }),
        );
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
                zsh: None,
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
                zsh: None,
                pwsh: None,
                nu: None,
            },
            abbr: vec![],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(
            s.contains("__runex_register_expand_handler 'Tab'"),
            "pwsh script must use the configured chord"
        );
    }

    #[test]
    fn pwsh_script_uses_spacebar_name_for_alt_space() {
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                trigger: None,
                bash: None,
                zsh: None,
                pwsh: Some(TriggerKey::AltSpace),
                nu: None,
            },
            abbr: vec![],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(
            s.contains("Set-PSReadLineKeyHandler -Chord 'Alt+Spacebar' -Function SelfInsert"),
            "pwsh script must use PowerShell's Spacebar key name"
        );
        assert!(
            s.contains("__runex_register_expand_handler 'Alt+Spacebar'"),
            "pwsh script must register Alt+Space using Spacebar"
        );
    }

    #[test]
    fn pwsh_script_embeds_known_tokens() {
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "gcm".into(),
                expand: "git commit -m".into(),
                when_command_exists: None,
            }],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(s.contains("'gcm' { return $true }"), "pwsh script must embed known tokens");
    }

    #[test]
    fn no_keybinds_means_no_handlers() {
        let s = export_script(Shell::Bash, "runex", Some(&Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![],
        }));
        assert!(!s.contains("bind -x"), "bash script should not bind keys by default");
        assert!(s.contains(r#"bind -r "\x20""#), "bash cleanup should still be emitted");

        let s = export_script(Shell::Pwsh, "runex", Some(&Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![],
        }));
        assert!(
            !s.contains("__runex_register_expand_handler '"),
            "pwsh script should not register expand handlers by default"
        );
        assert!(
            s.contains("Set-PSReadLineKeyHandler -Chord ' ' -Function SelfInsert"),
            "pwsh script should restore defaults even without custom handlers"
        );

        let s = export_script(Shell::Clink, "runex", Some(&Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![],
        }));
        assert!(
            !s.contains("rl.setbinding("),
            "clink script should not register handlers by default"
        );
    }
}
