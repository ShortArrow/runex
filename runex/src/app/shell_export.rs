//! `runex export <shell>` script generation and the trigger /
//! self-insert resolution that drives it.
//!
//! Composes a shell-specific integration script by interpolating
//! the resolved `[keybind.trigger]` / `[keybind.self_insert]`
//! settings into the per-shell template under
//! `domain/templates/`. The pure quoting helpers
//! (`bash_quote_string`, `lua_quote_string`, `pwsh_quote_string`,
//! `nu_quote_string`) live in `domain::shell` and are reached
//! through that module here.
//!
//! Phase D D2 split this out of `domain::shell`. The original
//! co-location was an accident of where the code was first
//! written: `export_script` takes a `&Config` and selects template
//! fragments based on user config, which is the shape of an
//! app-layer use-case, not pure quoting logic.

use crate::domain::model::{Config, TriggerKey};
use crate::domain::shell::{
    bash_quote_string, lua_quote_string, nu_quote_string, nu_quote_string_embedded,
    pwsh_quote_string, Shell,
};

fn trigger_for(shell: Shell, config: Option<&Config>) -> Option<TriggerKey> {
    let keybind = match config {
        Some(config) => &config.keybind,
        None => return None,
    };

    match shell {
        Shell::Bash => keybind.trigger.bash.or(keybind.trigger.default),
        Shell::Zsh => keybind.trigger.zsh.or(keybind.trigger.default),
        Shell::Pwsh => keybind.trigger.pwsh.or(keybind.trigger.default),
        Shell::Nu => keybind.trigger.nu.or(keybind.trigger.default),
        Shell::Clink => keybind.trigger.default,
    }
}

fn self_insert_for(shell: Shell, config: Option<&Config>) -> Option<TriggerKey> {
    let keybind = match config {
        Some(config) => &config.keybind,
        None => return None,
    };

    match shell {
        Shell::Bash => keybind.self_insert.bash.or(keybind.self_insert.default),
        Shell::Zsh => keybind.self_insert.zsh.or(keybind.self_insert.default),
        Shell::Pwsh => keybind.self_insert.pwsh.or(keybind.self_insert.default),
        Shell::Nu => keybind.self_insert.nu.or(keybind.self_insert.default),
        Shell::Clink => None,
    }
}

fn bash_chord(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space => "\\x20",
        TriggerKey::Tab => "\\C-i",
        TriggerKey::AltSpace => "\\e ",
        TriggerKey::ShiftSpace => unreachable!("ShiftSpace cannot be used as a trigger in bash"),
    }
}

fn zsh_chord(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space => " ",
        TriggerKey::Tab => "^I",
        TriggerKey::AltSpace => "^[ ",
        TriggerKey::ShiftSpace => unreachable!("ShiftSpace cannot be used as a trigger in zsh"),
    }
}
fn pwsh_chord(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space => " ",
        TriggerKey::Tab => "Tab",
        TriggerKey::AltSpace => "Alt+Spacebar",
        TriggerKey::ShiftSpace => unreachable!("ShiftSpace cannot be used as a trigger in pwsh"),
    }
}
fn nu_modifier(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::AltSpace => "alt",
        TriggerKey::ShiftSpace => "shift",
        TriggerKey::Space | TriggerKey::Tab => "none",
    }
}

fn nu_keycode(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space | TriggerKey::AltSpace | TriggerKey::ShiftSpace => "space",
        TriggerKey::Tab => "tab",
    }
}

fn clink_key_sequence(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space => r#"" ""#,
        TriggerKey::Tab => r#""\t""#,
        TriggerKey::AltSpace => r#""\e ""#,
        TriggerKey::ShiftSpace => unreachable!("ShiftSpace cannot be used as a trigger in clink"),
    }
}

fn bash_bind_lines(trigger: Option<TriggerKey>) -> String {
    let mut lines = Vec::new();
    if let Some(trigger) = trigger {
        lines.push(format!(
            r#"bind -r "{}" 2>/dev/null || true"#,
            bash_chord(trigger)
        ));
        lines.push(format!("bind -x '\"{}\": __runex_expand'", bash_chord(trigger)));
    }
    lines.join("\n")
}

/// Generate the `bindkey` lines for zsh, removing the old binding before adding the new one.
/// Only the configured trigger key is touched; other keys are left as-is.
fn zsh_bind_lines(trigger: Option<TriggerKey>) -> String {
    let mut lines = Vec::new();
    if let Some(trigger) = trigger {
        lines.push(format!(
            r#"bindkey -r "{}" 2>/dev/null"#,
            zsh_chord(trigger)
        ));
        lines.push(format!(r#"bindkey "{}" __runex_expand"#, zsh_chord(trigger)));
    }
    lines.join("\n")
}

fn bash_self_insert_lines(self_insert: Option<TriggerKey>) -> String {
    match self_insert {
        Some(TriggerKey::AltSpace) => [
            r#"bind -r "\e " 2>/dev/null || true"#,
            r#"bind '"\e ": self-insert'"#,
        ]
        .join("\n"),
        _ => String::new(),
    }
}

fn zsh_self_insert_lines(self_insert: Option<TriggerKey>) -> String {
    match self_insert {
        Some(TriggerKey::AltSpace) => [
            r#"bindkey -r "^[ " 2>/dev/null"#,
            r#"bindkey "^[ " self-insert"#,
        ]
        .join("\n"),
        _ => String::new(),
    }
}

fn pwsh_register_lines(trigger: Option<TriggerKey>) -> String {
    let mut lines = Vec::new();
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

fn pwsh_self_insert_lines(self_insert: Option<TriggerKey>) -> String {
    match self_insert {
        Some(TriggerKey::ShiftSpace) => {
            "    Set-PSReadLineKeyHandler -Chord 'Shift+Spacebar' -Function SelfInsert"
                .to_string()
        }
        Some(TriggerKey::AltSpace) => {
            "    Set-PSReadLineKeyHandler -Chord 'Alt+Spacebar' -Function SelfInsert".to_string()
        }
        _ => String::new(),
    }
}

fn nu_bindings(trigger: Option<TriggerKey>, bin: &str) -> String {
    let mut blocks = Vec::new();
    if let Some(trigger) = trigger {
        blocks.push(
            include_str!("../domain/templates/nu_expand_binding.nu")
                .replace("{NU_BIN}", &nu_quote_string_embedded(bin))
                .replace("{NU_MODIFIER}", nu_modifier(trigger))
                .replace("{NU_KEYCODE}", nu_keycode(trigger)),
        );
    }
    blocks.join(" | append ")
}

fn nu_self_insert_lines(self_insert: Option<TriggerKey>) -> String {
    let key = match self_insert {
        Some(TriggerKey::ShiftSpace) => Some(("shift", "space")),
        Some(TriggerKey::AltSpace) => Some(("alt", "space")),
        _ => None,
    };
    let Some((modifier, keycode)) = key else {
        return String::new();
    };
    include_str!("../domain/templates/nu_self_insert_binding.nu")
        .replace("{NU_SI_MODIFIER}", modifier)
        .replace("{NU_SI_KEYCODE}", keycode)
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
pub(crate) fn export_script(shell: Shell, bin: &str, config: Option<&Config>) -> String {
    let template = match shell {
        Shell::Bash => include_str!("../domain/templates/bash.sh"),
        Shell::Zsh => include_str!("../domain/templates/zsh.zsh"),
        Shell::Pwsh => include_str!("../domain/templates/pwsh.ps1"),
        Shell::Clink => include_str!("../domain/templates/clink.lua"),
        Shell::Nu => include_str!("../domain/templates/nu.nu"),
    };
    let trigger = trigger_for(shell, config);
    let self_insert = self_insert_for(shell, config);
    template
        .replace("\r\n", "\n")
        .replace("{BASH_BIN}", &bash_quote_string(bin))
        .replace("{BASH_BIND_LINES}", &bash_bind_lines(trigger))
        .replace("{BASH_SELF_INSERT_LINES}", &bash_self_insert_lines(self_insert))
        .replace("{ZSH_BIN}", &bash_quote_string(bin))
        .replace("{ZSH_BIND_LINES}", &zsh_bind_lines(trigger))
        .replace("{ZSH_SELF_INSERT_LINES}", &zsh_self_insert_lines(self_insert))
        .replace("{CLINK_BIN}", &lua_quote_string(bin))
        .replace("{CLINK_BINDING}", &clink_binding(trigger))
        .replace("{PWSH_BIN}", &pwsh_quote_string(bin))
        .replace("{PWSH_REGISTER_LINES}", &pwsh_register_lines(trigger))
        .replace("{PWSH_SELF_INSERT_LINES}", &pwsh_self_insert_lines(self_insert))
        .replace("{NU_BIN}", &nu_quote_string(bin))
        .replace("{NU_BINDINGS}", &nu_bindings(trigger, bin))
        .replace("{NU_SELF_INSERT_BINDINGS}", &nu_self_insert_lines(self_insert))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod script_generation {
        use super::*;

    #[test]
    fn export_script_contains_bin() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
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
                keybind: crate::domain::model::KeybindConfig {
                    trigger: crate::domain::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::domain::model::KeybindConfig::default()
                },
                precache: crate::domain::model::PrecacheConfig::default(),
                abbr: vec![],
            }),
        );
        // New design: the bootstrap is a thin wrapper that calls
        // `runex hook --shell bash` at keypress time. It should still bind the
        // trigger key via `bind -x`, but the expansion logic itself now lives
        // in the Rust binary — so there must be no inline `expand` call or
        // READLINE inspection in the template (the hook output handles both).
        assert!(s.contains("bind -x"), "bash bootstrap must use bind -x");
        assert!(
            s.contains("hook --shell bash"),
            "bash bootstrap must invoke `runex hook --shell bash`"
        );
        assert!(
            s.contains("'runex' hook --shell bash"),
            "bash bootstrap must quote the executable name"
        );
        assert!(!s.contains("{BASH_BIND_LINES}"), "bash script must resolve bind lines");
    }

    #[test]
    fn pwsh_script_has_psreadline() {
        let s = export_script(
            Shell::Pwsh,
            "runex",
            Some(&Config {
                version: 1,
                keybind: crate::domain::model::KeybindConfig {
                    trigger: crate::domain::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::domain::model::KeybindConfig::default()
                },
                precache: crate::domain::model::PrecacheConfig::default(),
                abbr: vec![],
            }),
        );
        assert!(s.contains("Set-PSReadLineKeyHandler"), "pwsh script must use PSReadLine");
        assert!(
            !s.contains("Set-PSReadLineKeyHandler -Chord 'Tab' -Function Complete"),
            "pwsh script must not clobber the user's Tab binding"
        );
        assert!(
            s.contains("'runex' @hookArgs") || s.contains("'runex' hook"),
            "pwsh bootstrap must invoke runex with hook args"
        );
        assert!(
            s.contains("hook"),
            "pwsh bootstrap must invoke `runex hook`"
        );
        assert!(!s.contains("{PWSH_REGISTER_LINES}"), "pwsh script must resolve register lines");
    }

    #[test]
    fn pwsh_script_has_paste_guard() {
        // The paste-detection reflection is the one piece of logic that has
        // to stay in the bootstrap — PSReadLine's `_queuedKeys` can only be
        // inspected from inside the PSReadLine process. Guard against it
        // being accidentally removed when the template is further trimmed.
        let s = export_script(
            Shell::Pwsh,
            "runex",
            Some(&Config {
                version: 1,
                keybind: crate::domain::model::KeybindConfig {
                    trigger: crate::domain::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::domain::model::KeybindConfig::default()
                },
                precache: crate::domain::model::PrecacheConfig::default(),
                abbr: vec![],
            }),
        );
        assert!(s.contains("__runex_queued_key_count"), "pwsh must retain paste guard helper");
        assert!(s.contains("_queuedKeys"), "pwsh must probe PSReadLine's _queuedKeys field");
        assert!(s.contains("--paste-pending"), "pwsh must forward paste state to `runex hook`");
    }

    #[test]
    fn zsh_script_has_zle_widget() {
        let s = export_script(
            Shell::Zsh,
            "runex",
            Some(&Config {
                version: 1,
                keybind: crate::domain::model::KeybindConfig {
                    trigger: crate::domain::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::domain::model::KeybindConfig::default()
                },
                precache: crate::domain::model::PrecacheConfig::default(),
                abbr: vec![],
            }),
        );
        assert!(s.contains("zle -N __runex_expand"), "zsh script must register a zle widget");
        assert!(s.contains(r#"bindkey " " __runex_expand"#), "zsh script must bind the trigger key");
        assert!(s.contains("LBUFFER"), "zsh script must inspect the text before the cursor");
        assert!(s.contains("RBUFFER"), "zsh script must inspect the text after the cursor");
        assert!(
            s.contains("'runex' hook --shell zsh"),
            "zsh bootstrap must invoke `runex hook --shell zsh`"
        );
    }

    #[test]
    fn clink_script_has_clink() {
        let s = export_script(
            Shell::Clink,
            "runex",
            Some(&Config {
                version: 1,
                keybind: crate::domain::model::KeybindConfig {
                    trigger: crate::domain::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::domain::model::KeybindConfig::default()
                },
                precache: crate::domain::model::PrecacheConfig::default(),
                abbr: vec![],
            }),
        );
        assert!(s.contains("clink"), "clink script must reference clink");
        assert!(s.contains("local RUNEX_BIN = \"runex\""), "clink script must quote the executable");
        assert!(
            s.contains("hook --shell clink"),
            "clink bootstrap must invoke `runex hook --shell clink`"
        );
        assert!(
            !s.contains("local RUNEX_KNOWN"),
            "clink bootstrap must not embed token lookup table (moved to `runex hook`)"
        );
        assert!(s.contains(r#"pcall(rl.setbinding, [[" "]], [["luafunc:runex_expand"]], "emacs")"#), "clink script must bind the trigger key in emacs mode");
        assert!(s.contains(r#"pcall(rl.setbinding, [[" "]], [["luafunc:runex_expand"]], "vi-insert")"#), "clink script must bind the trigger key in vi insert mode");
        assert!(s.contains("rl_buffer:getcursor()"), "clink script must inspect the cursor");
        assert!(!s.contains("clink.onfilterinput"), "clink script must not use onfilterinput for realtime expansion");
    }

    #[test]
    fn clink_script_uses_alt_space_sequence() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    default: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
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
                keybind: crate::domain::model::KeybindConfig {
                    trigger: crate::domain::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::domain::model::KeybindConfig::default()
                },
                precache: crate::domain::model::PrecacheConfig::default(),
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
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    bash: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(s.contains("\\e "), "bash script must use the configured key chord");
    }

    /// A bin value that is itself a template placeholder (e.g. `{BASH_BIN}`) must not cause
    /// a second substitution pass. Quoting wraps it in single quotes, so `.replace()` never
    /// matches it as a placeholder.
    #[test]
    fn export_script_placeholder_bin_does_not_cause_second_order_substitution() {
        use crate::domain::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig {
                trigger: crate::domain::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };

        let cases: &[(&str, Shell, &str)] = &[
            ("{BASH_BIN}", Shell::Bash, "'{BASH_BIN}'"),
            ("{ZSH_BIN}", Shell::Zsh, "'{ZSH_BIN}'"),
            ("{PWSH_BIN}", Shell::Pwsh, "'{PWSH_BIN}'"),
        ];
        for (placeholder, shell, expected_quoted) in cases {
            let s = export_script(*shell, placeholder, Some(&config));
            assert!(
                s.contains(expected_quoted),
                "bin={placeholder:?} for {shell:?} must appear as quoted literal {expected_quoted:?} in script"
            );
        }
    }

    /// `eval "$runex_debug_trap"` allows arbitrary code execution if bash-preexec or
    /// another framework installed a DEBUG trap with an attacker-controlled string.
    /// The script must NOT use eval to restore the trap.
    #[test]
    fn bash_script_does_not_eval_debug_trap() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(
            !s.contains("eval \"$runex_debug_trap\"") && !s.contains("eval '$runex_debug_trap'"),
            "bash script must not eval the captured debug trap: {s}"
        );
    }

    #[test]
    fn bash_script_does_not_embed_known_tokens() {
        // New design: the abbreviation list is consulted at keypress time by
        // `runex hook`, not baked into the bootstrap as a `case` block. This
        // keeps the emitted script independent of user-supplied key strings —
        // which, besides being simpler, avoids a whole class of injection
        // concerns (quoting gcm's key into a `case` arm).
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig::default(),
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![crate::domain::model::Abbr {
                key: "gcm".into(),
                expand: crate::domain::model::PerShellString::All("git commit -m".into()),
                when_command_exists: None,
            }],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(!s.contains("'gcm'"), "bash bootstrap must not embed tokens anymore");
        assert!(!s.contains("__runex_is_known_token"), "legacy helper removed");
    }

    #[test]
    fn pwsh_script_uses_global_keybind() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    default: Some(TriggerKey::Tab),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
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
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    pwsh: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(
            s.contains("__runex_register_expand_handler 'Alt+Spacebar'"),
            "pwsh script must register Alt+Space using Spacebar"
        );
    }

    #[test]
    fn pwsh_script_does_not_embed_known_tokens() {
        // Same rationale as bash_script_does_not_embed_known_tokens: the
        // hook-based bootstrap consults the config at keypress time.
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig::default(),
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![crate::domain::model::Abbr {
                key: "gcm".into(),
                expand: crate::domain::model::PerShellString::All("git commit -m".into()),
                when_command_exists: None,
            }],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(!s.contains("'gcm' { return $true }"), "pwsh must not embed tokens");
        assert!(!s.contains("__runex_is_known_token"), "legacy helper removed");
    }

    #[test]
    fn no_keybinds_means_no_handlers() {
        let s = export_script(Shell::Bash, "runex", Some(&Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig::default(),
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        }));
        assert!(!s.contains("bind -x"), "bash script should not bind keys by default");
        assert!(!s.contains(r#"bind -r"#), "bash script should not remove keybinds when no trigger is configured");

        let s = export_script(Shell::Pwsh, "runex", Some(&Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig::default(),
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        }));
        assert!(
            !s.contains("__runex_register_expand_handler '"),
            "pwsh script should not register expand handlers by default"
        );
        assert!(
            !s.contains("Set-PSReadLineKeyHandler -Chord ' ' -Function SelfInsert"),
            "pwsh script should not clobber default key handlers when no trigger is configured"
        );

        let s = export_script(Shell::Clink, "runex", Some(&Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig::default(),
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        }));
        assert!(
            !s.contains("rl.setbinding("),
            "clink script should not register handlers by default"
        );
    }

    /// These tests verify that a bin value containing shell metacharacters does
    /// not break out of the quoted context it is embedded in.
    /// The dangerous case is a quote character that closes the surrounding literal
    /// and allows arbitrary code to follow on the same line.
    #[test]
    fn bin_single_quote_is_escaped_in_bash() {
        let s = export_script(Shell::Bash, "run'ex", None);
        assert!(s.contains(r"'run'\''ex'"), "bash: single quote must be escaped as '\\''");
    }

    #[test]
    fn bin_single_quote_is_escaped_in_zsh() {
        let s = export_script(Shell::Zsh, "run'ex", None);
        assert!(s.contains(r"'run'\''ex'"), "zsh: single quote must be escaped as '\\''");
    }

    #[test]
    fn bin_single_quote_is_escaped_in_pwsh() {
        let s = export_script(Shell::Pwsh, "run'ex", None);
        assert!(s.contains("'run''ex'"), "pwsh: single quote must be doubled");
    }

    #[test]
    fn bin_double_quote_is_escaped_in_clink() {
        let s = export_script(Shell::Clink, r#"run"ex"#, None);
        assert!(s.contains(r#""run\"ex""#), "clink: double quote must be escaped");
    }

    #[test]
    fn bin_with_special_chars_is_safe_in_nu() {
        let s = export_script(Shell::Nu, "runex; echo INJECTED", None);
        // The bin value must appear only inside double quotes, never as a
        // naked command. `^"..."` runs the quoted external command literally.
        assert!(
            !s.contains("; echo INJECTED") || s.contains(r#"^"runex; echo INJECTED""#),
            "nu: bin value must be quoted; got:\n{s}"
        );
        // Paranoia: ensure no unquoted `echo INJECTED` appears at start of a line.
        for line in s.lines() {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with("echo INJECTED"),
                "nu: unquoted injection detected: {line}"
            );
        }
    }

    /// In Nu, quoting a command name as `"runex"` makes it a string, not a command.
    /// The correct external-command syntax is `^"runex"` — the `^` forces external execution.
    /// Inside the `cmd: "..."` heredoc string, the quotes must be escaped: `^\"runex\"`.
    /// The `{NU_BIN}` placeholder is only emitted when a trigger keybind is configured.
    #[test]
    fn nu_bin_uses_caret_external_command_syntax() {
        use crate::domain::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig {
                trigger: crate::domain::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Nu, "runex", Some(&config));
        assert!(
            s.contains("^\\\"runex\\\""),
            "nu: bin inside cmd string must use ^\\\"...\\\" syntax, got snippet: {:?}",
            s.lines().find(|l| l.contains("runex")).unwrap_or("<not found>")
        );
    }

    #[test]
    fn nu_bin_with_special_chars_uses_caret_syntax() {
        use crate::domain::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig {
                trigger: crate::domain::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Nu, "my\"app", Some(&config));
        assert!(s.contains("^\\\"my\\\\\\\"app\\\""), "nu: special chars must be escaped in embedded context: {s}");
    }

    /// REGRESSION: `{NU_BIN}` is substituted inside a `cmd: "..."` double-quoted Nu string.
    /// If the substitution produces `^"runex"`, the `"` terminates the outer string → syntax error.
    /// The substitution must use `\"` (escaped) inside the cmd context: `^\"runex\"`.
    #[test]
    fn nu_bin_in_cmd_string_does_not_break_outer_quotes() {
        use crate::domain::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig {
                trigger: crate::domain::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Nu, "runex", Some(&config));
        let cmd_start = s.find("cmd: \"").expect("cmd: block not found");
        let cmd_block = &s[cmd_start..];
        assert!(
            cmd_block.contains("^\\\"runex\\\""),
            "nu: bin inside cmd string must use ^\\\"...\\\" syntax (escaped quotes), got:\n{}",
            cmd_block.lines().find(|l| l.contains("runex")).unwrap_or("<not found>")
        );
    }

    } // mod script_generation
    mod regression_issues {
        use super::*;

    /// A `"` in bin must not terminate the shell double-quoted string inside `io.popen`.
    /// The fix is single-quote wrapping (with `'\''` for embedded single quotes).
    #[test]
    fn clink_script_double_quote_in_bin_does_not_inject_into_popen() {
        let s = export_script(Shell::Clink, "run\"ex", Some(&Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig::default(),
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        }));
        assert!(
            !s.contains(r#"'"' .. RUNEX_BIN .. '"'"#),
            "io.popen must not wrap RUNEX_BIN in shell double-quotes: {s}"
        );
    }

    #[test]
    fn clink_script_bin_with_double_quote_uses_single_quote_shell_wrapping() {
        let s = export_script(Shell::Clink, "run\"ex", Some(&Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig::default(),
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        }));
        assert!(
            s.contains("runex_shell_quote"),
            "clink script must use a shell-quoting helper for RUNEX_BIN in io.popen: {s}"
        );
    }


    /// Regression: cmd.exe (which Lua's `io.popen` invokes via `cmd /c
    /// <string>` on Windows) heuristically strips the *outermost* pair of
    /// double quotes when the string starts AND ends with `"`. The clink
    /// template therefore must wrap the entire io.popen command in an
    /// extra pair of `"` so the inner quoting around argv0 (which can
    /// contain spaces, e.g. `C:\Program Files\...`) and `--line "..."`
    /// survives. Without this, cmd reports `'... is not recognized as an
    /// internal or external command'` and the hook never executes.
    #[test]
    fn clink_io_popen_command_is_wrapped_in_extra_pair_of_quotes() {
        let s = export_script(Shell::Clink, "runex", None);
        assert!(
            s.contains("local cmd = '\"' .. runex_shell_quote(RUNEX_BIN)"),
            "clink script must prepend a literal '\"' before runex_shell_quote(RUNEX_BIN): {s}"
        );
        assert!(
            s.contains("' 2>&1\"'"),
            "clink script must append a literal '\"' after `2>&1`: {s}"
        );
    }

    /// Security: cmd.exe expands `%VAR%` even inside double-quoted argv,
    /// and `!VAR!` if any caller has SETLOCAL ENABLEDELAYEDEXPANSION
    /// active. The clink template must reject buffer content containing
    /// either, to prevent shell-buffer-driven injection through io.popen.
    /// See `runex-core/src/templates/clink.lua::runex_is_safe_line` for
    /// the rationale.
    #[test]
    fn clink_safe_line_check_rejects_cmd_metachars() {
        let s = export_script(Shell::Clink, "runex", None);
        // The lua regex literal must contain `%%` (escaped `%` in lua
        // pattern syntax) and `!` so both are rejected at the gate.
        assert!(
            s.contains("%%!") || s.contains("!%%"),
            "clink safe-line regex must reject `%` and `!`: {s}"
        );
    }

    #[test]
    fn nu_quote_string_escapes_tab() {
        let s = nu_quote_string("run\tex");
        assert!(!s.contains('\t'), "nu_quote_string must escape tab: {s:?}");
        assert!(s.contains("\\t"), "expected \\t escape: {s:?}");
    }


    #[test]
    fn bash_quote_string_drops_unicode_line_separator() {
        let s = bash_quote_string("run\u{2028}ex");
        assert!(!s.contains('\u{2028}'), "bash_quote_string must drop U+2028: {s:?}");
    }

    #[test]
    fn pwsh_quote_string_drops_unicode_line_separator() {
        let s = pwsh_quote_string("run\u{2028}ex");
        assert!(!s.contains('\u{2028}'), "pwsh_quote_string must drop U+2028: {s:?}");
    }

    #[test]
    fn nu_quote_string_drops_unicode_line_separator() {
        let s = nu_quote_string("run\u{2028}ex");
        assert!(!s.contains('\u{2028}'), "nu_quote_string must drop U+2028: {s:?}");
    }


    #[test]
    fn nu_quote_string_drops_del() {
        let s = nu_quote_string("run\x7fex");
        assert!(!s.contains('\x7f'), "nu_quote_string must drop DEL (\\x7f): {s:?}");
    }

    #[test]
    fn nu_quote_string_escapes_dollar_sign() {
        let s = nu_quote_string("run$exenv");
        let raw_dollar = s
            .char_indices()
            .filter(|(_, c)| *c == '$')
            .any(|(i, _)| i == 0 || s.as_bytes()[i - 1] != b'\\');
        assert!(
            !raw_dollar,
            "nu_quote_string must escape '$' to prevent Nu variable interpolation: {s:?}"
        );
        assert!(s.contains("\\$"), "expected \\$ escape sequence in: {s:?}");
    }

    /// In the outer `cmd: "..."` Nu string, `\\$` means literal `\` + variable interpolation
    /// (unsafe). `nu_quote_string` emits `\$` for a literal `$`; when embedded, `\$` must
    /// become `\\\$` so the outer parser still sees `\$` (suppressed interpolation), not `\\$`.
    /// Verified by asserting every `$` byte is preceded by an odd number of backslashes.
    #[test]
    fn nu_quote_string_embedded_escapes_dollar_sign() {
        let s = nu_quote_string_embedded("run$exenv");
        let bytes = s.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'$' {
                let mut preceding_backslashes = 0usize;
                let mut j = i;
                while j > 0 && bytes[j - 1] == b'\\' {
                    preceding_backslashes += 1;
                    j -= 1;
                }
                assert!(
                    preceding_backslashes % 2 == 1,
                    "nu_quote_string_embedded: '$' at byte {i} has {preceding_backslashes} preceding backslashes \
                     (even = Nu interpolation NOT suppressed). Full output: {s:?}"
                );
            }
        }
    }

    /// `\n`, `\r`, `\t` are escaped as two-character sequences; all other C0 control chars
    /// (`\x01`–`\x08`, `\x0b`, `\x0c`, `\x0e`–`\x1f`) are dropped entirely.
    #[test]
    fn nu_quote_string_drops_remaining_c0_control_chars() {
        let dangerous_c0: &[char] = &[
            '\x01', '\x02', '\x03', '\x04', '\x05', '\x06', '\x07',
            '\x08', '\x0b', '\x0c', '\x0e', '\x0f',
            '\x10', '\x11', '\x12', '\x13', '\x14', '\x15', '\x16', '\x17',
            '\x18', '\x19', '\x1a', '\x1b',
            '\x1c', '\x1d', '\x1e', '\x1f',
        ];
        for &ch in dangerous_c0 {
            let input = format!("run{}ex", ch);
            let s = nu_quote_string(&input);
            assert!(
                !s.contains(ch),
                "nu_quote_string must drop C0 control U+{:04X}: {s:?}",
                ch as u32
            );
        }
    }

    #[test]
    fn pwsh_self_insert_shift_space_when_configured() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                self_insert: crate::domain::model::PerShellKey {
                    pwsh: Some(TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(
            s.contains("Set-PSReadLineKeyHandler -Chord 'Shift+Spacebar' -Function SelfInsert"),
            "pwsh script must bind Shift+Spacebar to SelfInsert when self_insert = shift-space: {s}"
        );
    }

    #[test]
    fn pwsh_self_insert_alt_space_when_configured() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                self_insert: crate::domain::model::PerShellKey {
                    pwsh: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(
            s.contains("Set-PSReadLineKeyHandler -Chord 'Alt+Spacebar' -Function SelfInsert"),
            "pwsh script must bind Alt+Spacebar to SelfInsert when self_insert = alt-space: {s}"
        );
    }

    #[test]
    fn pwsh_no_self_insert_when_not_configured() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(
            !s.contains("SelfInsert"),
            "pwsh script must not bind SelfInsert when self_insert is not configured (even if trigger is Space): {s}"
        );
    }

    #[test]
    fn nu_self_insert_shift_space_when_configured() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                self_insert: crate::domain::model::PerShellKey {
                    nu: Some(TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Nu, "runex", Some(&config));
        assert!(
            s.contains("runex_self_insert") && s.contains("modifier: shift") && s.contains("keycode: space"),
            "nu script must include shift+space self-insert binding when self_insert = shift-space: {s}"
        );
    }

    #[test]
    fn nu_self_insert_alt_space_when_configured() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                self_insert: crate::domain::model::PerShellKey {
                    nu: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Nu, "runex", Some(&config));
        assert!(
            s.contains("runex_self_insert") && s.contains("modifier: alt") && s.contains("keycode: space"),
            "nu script must include alt+space self-insert binding when self_insert = alt-space: {s}"
        );
    }

    #[test]
    fn nu_no_self_insert_when_not_configured() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Nu, "runex", Some(&config));
        assert!(
            !s.contains("insertchar"),
            "nu script must not contain insertchar append block when self_insert is not configured: {s}"
        );
    }

    #[test]
    fn bash_self_insert_alt_space_when_configured() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                self_insert: crate::domain::model::PerShellKey {
                    bash: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(
            s.contains(r#"bind '"\e ": self-insert'"#),
            "bash script must bind Alt+Space to self-insert when self_insert = alt-space: {s}"
        );
    }

    #[test]
    fn bash_no_self_insert_when_not_configured() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(
            !s.contains("self-insert"),
            "bash script must not contain self-insert when self_insert is not configured: {s}"
        );
    }

    #[test]
    fn zsh_self_insert_alt_space_when_configured() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                self_insert: crate::domain::model::PerShellKey {
                    zsh: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Zsh, "runex", Some(&config));
        assert!(
            s.contains(r#"bindkey "^[ " self-insert"#),
            "zsh script must bind Alt+Space to self-insert when self_insert = alt-space: {s}"
        );
    }

    #[test]
    fn zsh_no_self_insert_when_not_configured() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    ..Default::default()
                },
                ..crate::domain::model::KeybindConfig::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Zsh, "runex", Some(&config));
        assert!(
            !s.contains("self-insert"),
            "zsh script must not contain self-insert when self_insert is not configured: {s}"
        );
    }

    #[test]
    fn trigger_for_shell_override_takes_precedence_over_default() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    bash: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        // bash-specific override (AltSpace) takes precedence over default (Space)
        let bash_s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(bash_s.contains("\\e "), "bash must use AltSpace override, not default Space");
        // zsh falls back to default (Space)
        let zsh_s = export_script(Shell::Zsh, "runex", Some(&config));
        assert!(zsh_s.contains(r#"bindkey " " __runex_expand"#), "zsh must fall back to default Space");
    }

    #[test]
    fn trigger_for_falls_back_to_default() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    default: Some(TriggerKey::Tab),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        // nu has no shell-specific override, must use default (Tab)
        let nu_s = export_script(Shell::Nu, "runex", Some(&config));
        assert!(nu_s.contains("tab"), "nu must fall back to default Tab trigger");
    }

    #[test]
    fn clink_ignores_shell_specific_trigger_fields() {
        let config = Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig {
                trigger: crate::domain::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    bash: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        // Clink only uses trigger.default, not bash/zsh/pwsh/nu
        let s = export_script(Shell::Clink, "runex", Some(&config));
        assert!(
            s.contains(r#"pcall(rl.setbinding, [[" "]], [["luafunc:runex_expand"]]"#),
            "clink must use trigger.default (Space), not the bash-specific AltSpace: {s}"
        );
    }

    } // mod regression_issues

    /// Migrated from `domain::shell::tests::quote_functions` in
    /// Phase D D2b — both tests assert on `export_script` output,
    /// which is an app concern.
    mod nu_export_regression {
        use super::*;
        use crate::domain::model::{Config, KeybindConfig, PerShellKey, PrecacheConfig, TriggerKey};

        #[test]
        fn nu_hook_invocation_uses_separate_line_and_cursor_args() {
            let config = Config {
                version: 1,
                keybind: KeybindConfig {
                    trigger: PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                    ..Default::default()
                },
                precache: PrecacheConfig::default(),
                abbr: vec![],
            };
            let s = export_script(Shell::Nu, "runex", Some(&config));
            assert!(
                s.contains("hook --shell nu --line $line --cursor $cursor"),
                "Nu bootstrap must pass buffer state as separate --line/--cursor args: {s}"
            );
            assert!(s.contains("from json"), "Nu bootstrap must parse hook output via `from json`: {s}");
        }

        #[test]
        fn nu_bin_newline_does_not_inject_into_cmd_block() {
            let config = Config {
                version: 1,
                keybind: KeybindConfig {
                    trigger: PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                    ..Default::default()
                },
                precache: PrecacheConfig::default(),
                abbr: vec![],
            };
            let s = export_script(Shell::Nu, "runex\nsource /tmp/evil.nu\n", Some(&config));
            let lines: Vec<&str> = s.lines().collect();
            assert!(
                !lines.iter().any(|l| l.trim() == "source /tmp/evil.nu"),
                "newline must not create an injected source line: {s}"
            );
        }
    }
}
