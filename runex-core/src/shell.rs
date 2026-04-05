use std::fmt;
use std::str::FromStr;

use crate::model::{Config, TriggerKey};
use crate::sanitize::{double_quote_escape, is_nu_drop_char, is_unicode_line_separator, is_unsafe_for_display};

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
        // Sanitize the user-supplied shell name before embedding it in the error message.
        // Strip ASCII control characters (terminal escape sequences, cursor movement, etc.)
        // and Unicode visual-deception characters (invisible chars, directional overrides,
        // BOM, zero-width chars) that could cause the displayed message to look different
        // from its actual byte content.
        let safe: String = self
            .0
            .chars()
            .filter(|&c| !is_unsafe_for_display(c))
            .collect();
        write!(
            f,
            "unknown shell '{}' (expected: bash, zsh, pwsh, clink, nu)",
            safe
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

/// Quote `token` for use as a Bash `case` pattern.
///
/// Uses the same escaping as [`bash_quote_string`]: single-quoted with `'\''`
/// for embedded single quotes.  ASCII control characters and Unicode
/// line/paragraph separators are dropped.
fn bash_quote_pattern(token: &str) -> String {
    let mut out = String::from("'");
    for ch in token.chars() {
        match ch {
            '\'' => out.push_str(r"'\''"),
            c if c.is_ascii_control() || is_unicode_line_separator(c) => {}
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

/// Quote `value` as a Bash single-quoted string.
///
/// Single quotes are escaped as `'\''` (close, escaped quote, reopen).
/// ASCII control characters and Unicode line/paragraph separators are dropped:
/// valid executable paths never contain them, and embedding `$'\n'` inside
/// `eval "$(...)"` would cause command-splitting injection.
pub(crate) fn bash_quote_string(value: &str) -> String {
    let mut out = String::from("'");
    for ch in value.chars() {
        match ch {
            '\'' => out.push_str(r"'\''"),
            c if c.is_ascii_control() || is_unicode_line_separator(c) => {}
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

/// Generate the `case` pattern body for POSIX-compatible shells (bash, zsh).
///
/// When the config is absent or has no rules, returns a wildcard arm that always
/// returns 0 (treat every token as a known abbreviation).  Otherwise generates one
/// arm per rule plus a wildcard arm that returns 1 (unknown).
fn posix_known_cases(config: Option<&Config>) -> String {
    let Some(config) = config else {
        return "        *) return 0 ;;".to_string();
    };
    if config.abbr.is_empty() {
        return "        *) return 0 ;;".to_string();
    }
    let mut lines = Vec::with_capacity(config.abbr.len() + 1);
    for abbr in &config.abbr {
        lines.push(format!("        {}) return 0 ;;", bash_quote_pattern(&abbr.key)));
    }
    lines.push("        *) return 1 ;;".to_string());
    lines.join("\n")
}

fn bash_known_cases(config: Option<&Config>) -> String {
    posix_known_cases(config)
}

fn zsh_known_cases(config: Option<&Config>) -> String {
    posix_known_cases(config)
}

fn pwsh_chord(trigger: TriggerKey) -> &'static str {
    match trigger {
        TriggerKey::Space => " ",
        TriggerKey::Tab => "Tab",
        TriggerKey::AltSpace => "Alt+Spacebar",
    }
}

/// Quote `token` as a PowerShell single-quoted string.
///
/// Single quotes are escaped as `''`.  ASCII control characters and Unicode
/// line/paragraph separators are dropped: valid executable paths never contain them,
/// and backtick concatenation (`'a'`n'b'`) risks token-splitting in some PS contexts.
pub(crate) fn pwsh_quote_string(token: &str) -> String {
    let mut out = String::from("'");
    for ch in token.chars() {
        match ch {
            '\'' => out.push_str("''"),
            c if c.is_ascii_control() || is_unicode_line_separator(c) => {}
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

/// Quote `value` for use as an external Nu shell command invocation (`^"..."`).
///
/// The `^` prefix forces Nu to execute the string as an external command rather
/// than treating it as a string literal.  Inside the double-quoted form:
/// - `\` → `\\`, `"` → `\"`, `$` → `\$` (suppresses variable interpolation)
/// - `\n`, `\r`, `\t` are escaped as their two-character sequences
/// - NUL, DEL, remaining ASCII control characters, and Unicode line/paragraph
///   separators are dropped
pub(crate) fn nu_quote_string(value: &str) -> String {
    let mut out = String::from("^\"");
    for ch in value.chars() {
        if let Some(esc) = double_quote_escape(ch) {
            out.push_str(esc);
        } else if ch == '$' {
            out.push_str("\\$");
        } else if is_nu_drop_char(ch) {
        } else {
            out.push(ch);
        }
    }
    out.push('"');
    out
}

/// Like [`nu_quote_string`], but safe for embedding inside an outer Nu double-quoted
/// string (e.g. `cmd: "..."`).
///
/// Each `\` and `"` in the standalone form must be escaped one more level so the outer
/// Nu parser sees them as literals.  The two-character sequence `\$` (produced by
/// [`nu_quote_string`] to suppress variable interpolation) is kept atomic — converting
/// `\` to `\\` here would yield `\\$`, which the outer parser reads as a literal `\`
/// followed by variable interpolation (unsafe).
///
/// Standalone: `^"runex"`  →  Embedded: `^\"runex\"`
fn nu_quote_string_embedded(value: &str) -> String {
    let standalone = nu_quote_string(value);
    let mut out = String::with_capacity(standalone.len() + 8);
    let mut chars = standalone.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if chars.peek() == Some(&'$') {
                    // \$ from nu_quote_string: keep as \$ so the outer Nu string sees \$
                    // (literal $, no interpolation).
                    out.push('\\');
                    out.push('$');
                    chars.next(); // consume the '$'
                } else {
                    out.push_str("\\\\");
                }
            }
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out
}

/// Quote `value` as a Lua double-quoted string.
///
/// - `\`, `"` → `\\`, `\"`
/// - `\n`, `\r`, `\t` → two-character escape sequences
/// - NUL is dropped (Lua uses C strings; NUL truncates them)
/// - Unicode line/paragraph separators are dropped
/// - Remaining ASCII control characters use three-digit decimal `\DDD` escapes.
///   Zero-padding is required: without it `\1` followed by `0` would be read as
///   `\10` (LF) rather than SOH followed by `"0"`.
pub(crate) fn lua_quote_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        if let Some(esc) = double_quote_escape(ch) {
            out.push_str(esc);
        } else if ch == '\0' || is_unicode_line_separator(ch) {
        } else if ch.is_ascii_control() {
            out.push_str(&format!("\\{:03}", ch as u8));
        } else {
            out.push(ch);
        }
    }
    out.push('"');
    out
}

fn pwsh_known_cases(config: Option<&Config>) -> String {
    let Some(config) = config else {
        return String::new();
    };

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
    let mut lines = Vec::new();
    // Only unbind and rebind the key that runex is configured to use.
    if let Some(trigger) = trigger {
        lines.push(format!(
            r#"bind -r "{}" 2>/dev/null || true"#,
            bash_chord(trigger)
        ));
        lines.push(format!("bind -x '\"{}\": __runex_expand'", bash_chord(trigger)));
    }
    lines.join("\n")
}

fn zsh_bind_lines(trigger: Option<TriggerKey>) -> String {
    let mut lines = Vec::new();
    // Only unbind and rebind the key that runex is configured to use.
    if let Some(trigger) = trigger {
        lines.push(format!(
            r#"bindkey -r "{}" 2>/dev/null"#,
            zsh_chord(trigger)
        ));
        lines.push(format!(r#"bindkey "{}" __runex_expand"#, zsh_chord(trigger)));
    }
    lines.join("\n")
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
    if trigger == Some(TriggerKey::Space) {
        lines.push(
            "    Set-PSReadLineKeyHandler -Chord 'Shift+Spacebar' -Function SelfInsert"
                .to_string(),
        );
    }
    lines.join("\n")
}

fn nu_bindings(trigger: Option<TriggerKey>, bin: &str) -> String {
    let mut blocks = Vec::new();
    if let Some(trigger) = trigger {
        blocks.push(
            include_str!("templates/nu_expand_binding.nu")
                .replace("{NU_BIN}", &nu_quote_string_embedded(bin))
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

    mod shell_parse {
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

    /// `Shell::from_str` is called with user-supplied input. Embedding raw ANSI sequences
    /// (e.g. `"bash\x1b[2J"`) in an error message printed to stderr causes terminal injection.
    /// The `Display` impl must sanitize the shell name before embedding it.
    #[test]
    fn shell_parse_error_display_strips_esc_sequences() {
        let err = Shell::from_str("bash\x1b[2Jevil").unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains('\x1b'),
            "ShellParseError Display must not contain raw ESC: {msg:?}"
        );
    }

    #[test]
    fn shell_parse_error_display_strips_bel() {
        let err = Shell::from_str("bash\x07evil").unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains('\x07'),
            "ShellParseError Display must not contain raw BEL: {msg:?}"
        );
    }

    #[test]
    fn shell_parse_error_display_strips_del() {
        let err = Shell::from_str("bash\x7fevil").unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains('\x7f'),
            "ShellParseError Display must not contain DEL: {msg:?}"
        );
    }

    #[test]
    fn shell_parse_error_display_strips_rlo() {
        let err = Shell::from_str("bash\u{202E}lve").unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains('\u{202E}'),
            "ShellParseError Display must not contain RLO U+202E: {msg:?}"
        );
    }

    #[test]
    fn shell_parse_error_display_strips_bom() {
        let err = Shell::from_str("bash\u{FEFF}evil").unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains('\u{FEFF}'),
            "ShellParseError Display must not contain BOM U+FEFF: {msg:?}"
        );
    }

    #[test]
    fn shell_parse_error_display_strips_zwsp() {
        let err = Shell::from_str("ba\u{200B}sh").unwrap_err();
        let msg = err.to_string();
        assert!(
            !msg.contains('\u{200B}'),
            "ShellParseError Display must not contain ZWSP U+200B: {msg:?}"
        );
    }

    #[test]
    fn parse_unknown_errors() {
        let err = Shell::from_str("fish").unwrap_err();
        assert_eq!(err.0, "fish");
    }

    } // mod shell_parse

    mod script_generation {
        use super::*;

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
        // Space trigger → only the space keybind should be removed before rebinding.
        assert!(s.contains(r#"bind -r "\x20""#), "bash script must remove the space binding before rebinding");
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
            !s.contains("Set-PSReadLineKeyHandler -Chord 'Tab' -Function Complete"),
            "pwsh script must not clobber the user's Tab binding"
        );
        assert!(s.contains("$expanded = & 'runex' expand"), "pwsh script must quote the executable");
        assert!(s.contains("$cursor -lt $line.Length"), "pwsh script must guard mid-line insertion");
        assert!(s.contains("EditMode"), "pwsh script must handle PSReadLine edit mode");
        assert!(s.contains("__runex_is_command_position"), "pwsh script must detect command position");
        assert!(!s.contains("{PWSH_REGISTER_LINES}"), "pwsh script must resolve register lines");
    }

    #[test]
    fn pwsh_script_short_circuits_non_candidates() {
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
        assert!(
            s.contains("function __runex_get_expand_candidate"),
            "pwsh script must define a fast precheck helper"
        );
        assert!(
            s.contains("$candidate = __runex_get_expand_candidate $line $cursor"),
            "pwsh handler must skip full expansion logic for non-candidates"
        );
        assert!(
            s.contains("[Microsoft.PowerShell.PSConsoleReadLine]::Insert(' ')"),
            "pwsh handler must insert a plain space on the fast path"
        );
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
    fn export_script_placeholder_bin_does_not_cause_second_order_substitution() {
        // If bin contains a placeholder string that exists in the same template
        // (e.g. bash bin="{BASH_BIN}", zsh bin="{ZSH_BIN}"), the quoting functions
        // wrap it in single quotes so the subsequent .replace() calls do NOT match.
        // This test documents that invariant for each shell's own placeholder.
        use crate::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig { trigger: Some(TriggerKey::Space), ..Default::default() },
            abbr: vec![],
        };

        // Each shell: bin = that shell's own BIN placeholder.
        // After quoting it becomes '...' and must not be replaced by the real binary path.
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
            keybind: crate::model::KeybindConfig { trigger: Some(TriggerKey::Space), ..Default::default() },
            abbr: vec![],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(
            !s.contains("eval \"$runex_debug_trap\"") && !s.contains("eval '$runex_debug_trap'"),
            "bash script must not eval the captured debug trap: {s}"
        );
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
        assert!(!s.contains(r#"bind -r"#), "bash script should not remove keybinds when no trigger is configured");

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
            !s.contains("Set-PSReadLineKeyHandler -Chord ' ' -Function SelfInsert"),
            "pwsh script should not clobber default key handlers when no trigger is configured"
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
        assert!(!s.contains("echo INJECTED"), "nu: bin value must be quoted");
    }

    /// In Nu, quoting a command name as `"runex"` makes it a string, not a command.
    /// The correct external-command syntax is `^"runex"` — the `^` forces external execution.
    /// Inside the `cmd: "..."` heredoc string, the quotes must be escaped: `^\"runex\"`.
    /// The `{NU_BIN}` placeholder is only emitted when a trigger keybind is configured.
    #[test]
    fn nu_bin_uses_caret_external_command_syntax() {
        use crate::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig { trigger: Some(TriggerKey::Space), ..Default::default() },
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
        use crate::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig { trigger: Some(TriggerKey::Space), ..Default::default() },
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
        use crate::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig { trigger: Some(TriggerKey::Space), ..Default::default() },
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

    mod quote_functions {
        use super::*;

    #[test]
    fn nu_quote_string_escapes_newline() {
        let s = nu_quote_string("run\nex");
        assert!(!s.contains('\n'), "nu_quote_string must escape newline: {s}");
        assert!(s.contains("\\n"), "expected \\n escape: {s}");
    }

    #[test]
    fn nu_quote_string_escapes_carriage_return() {
        let s = nu_quote_string("run\rex");
        assert!(!s.contains('\r'), "nu_quote_string must escape CR: {s}");
        assert!(s.contains("\\r"), "expected \\r escape: {s}");
    }

    /// `expand --token $token` (space-separated) is vulnerable to argument injection:
    /// if `$token` is `"--dry-run"`, Clap receives `["expand", "--token", "--dry-run"]` and
    /// may treat `"--dry-run"` as a flag rather than the value for `--token`.
    /// The safe form `expand --token=($token)` passes the value as part of the same argument.
    /// Note: `($token)` is Nu's parenthesized expression, not string interpolation —
    /// Nu evaluates it and passes `--token=<value>` as a single argument.
    #[test]
    fn nu_token_uses_equals_form_to_prevent_argument_injection() {
        use crate::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig { trigger: Some(TriggerKey::Space), ..Default::default() },
            abbr: vec![],
        };
        let s = export_script(Shell::Nu, "runex", Some(&config));
        // Must NOT use the space-separated form (argument injection risk)
        assert!(
            !s.contains("--token $token"),
            "Nu script must not use space-separated --token (argument injection risk): {s}"
        );
        // Must NOT use Nu string interpolation (code execution risk)
        assert!(
            !s.contains("$\"--token=($token)\"") && !s.contains("\"--token=("),
            "Nu script must not use string interpolation for --token: {s}"
        );
        // Must use the --token=($token) form (safe: value bound to flag, no interpolation)
        assert!(
            s.contains("--token=($token)"),
            "Nu script must use --token=($token) form to prevent argument injection: {s}"
        );
    }

    #[test]
    fn nu_bin_newline_does_not_inject_into_cmd_block() {
        // A newline in bin must not break out of the cmd: "..." block.
        // \n is escaped to \\n so no literal newline appears in the script.
        use crate::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig { trigger: Some(TriggerKey::Space), ..Default::default() },
            abbr: vec![],
        };
        let s = export_script(Shell::Nu, "runex\nsource /tmp/evil.nu\n", Some(&config));
        // "source /tmp/evil.nu" must not appear as a standalone line
        let lines: Vec<&str> = s.lines().collect();
        assert!(
            !lines.iter().any(|l| l.trim() == "source /tmp/evil.nu"),
            "newline must not create an injected source line: {s}"
        );
    }

    // --- bash_quote_string / pwsh_quote_string: control char handling ---

    #[test]
    fn bash_quote_string_drops_newline() {
        // Control chars are dropped; $'\n' inside eval "$(...)" causes command splitting injection.
        let s = bash_quote_string("run\nex");
        assert!(!s.contains('\n'), "bash_quote_string must drop newline: {s:?}");
        assert!(!s.contains("$'"), "dollar-quote ANSI-C form must not be used: {s:?}");
        assert!(s.contains("runex"), "remaining chars must be preserved: {s:?}");
    }

    #[test]
    fn bash_quote_string_drops_carriage_return() {
        let s = bash_quote_string("run\rex");
        assert!(!s.contains('\r'), "bash_quote_string must drop CR: {s:?}");
        assert!(s.contains("runex"), "remaining chars must be preserved: {s:?}");
    }

    #[test]
    fn bash_quote_string_escapes_nul() {
        let s = bash_quote_string("run\x00ex");
        assert!(!s.contains('\0'), "bash_quote_string must drop NUL: {s:?}");
    }

    #[test]
    fn pwsh_quote_string_drops_newline() {
        // Backtick-concat ('a'`n'b') risks token-splitting; control chars are dropped instead.
        let s = pwsh_quote_string("run\nex");
        assert!(!s.contains('\n'), "pwsh_quote_string must drop newline: {s:?}");
        assert!(!s.contains("'`"), "backtick-concat form must not be used: {s:?}");
        assert!(s.contains("runex"), "remaining chars must be preserved: {s:?}");
    }

    #[test]
    fn pwsh_quote_string_drops_carriage_return() {
        let s = pwsh_quote_string("run\rex");
        assert!(!s.contains('\r'), "pwsh_quote_string must drop CR: {s:?}");
        assert!(s.contains("runex"), "remaining chars must be preserved: {s:?}");
    }

    #[test]
    fn pwsh_quote_string_escapes_nul() {
        let s = pwsh_quote_string("run\x00ex");
        assert!(!s.contains('\0'), "pwsh_quote_string must drop NUL: {s:?}");
    }

    #[test]
    fn nu_quote_string_escapes_nul() {
        let s = nu_quote_string("run\x00ex");
        assert!(!s.contains('\0'), "nu_quote_string must drop NUL: {s:?}");
    }


    #[test]
    fn bash_quote_string_newline_safe_in_eval_context() {
        // $'\n' inside eval "$(...)" expands to a literal newline, acting as a command separator.
        // The safe approach is to drop control characters rather than use $'...' ANSI-C quoting.
        let line = bash_quote_string("runex\necho INJECTED");
        assert!(!line.contains('\n'), "literal newline must not appear: {line:?}");
        assert!(!line.contains("$'"), "dollar-quote ANSI-C form must not be used (eval injection risk): {line:?}");
    }

    #[test]
    fn bash_quote_string_cr_safe_in_eval_context() {
        let line = bash_quote_string("runex\recho INJECTED");
        assert!(!line.contains('\r'), "literal CR must not appear: {line:?}");
        assert!(!line.contains("$'"), "dollar-quote ANSI-C form must not be used: {line:?}");
    }

    // --- bash_quote_pattern: control char handling ---

    #[test]
    fn bash_quote_pattern_escapes_newline() {
        let s = bash_quote_pattern("key\nwith newline");
        assert!(!s.contains('\n'), "bash_quote_pattern must not produce literal newline: {s:?}");
    }

    #[test]
    fn bash_quote_pattern_escapes_carriage_return() {
        let s = bash_quote_pattern("key\rwith cr");
        assert!(!s.contains('\r'), "bash_quote_pattern must not produce literal CR: {s:?}");
    }

    // --- lua_quote_string: NUL and control char handling ---

    #[test]
    fn lua_quote_string_escapes_nul() {
        let s = lua_quote_string("run\x00ex");
        assert!(!s.contains('\0'), "lua_quote_string must not produce literal NUL: {s:?}");
    }

    #[test]
    fn lua_quote_string_escapes_tab() {
        let s = lua_quote_string("run\tex");
        assert!(!s.contains('\t'), "lua_quote_string must escape tab: {s:?}");
    }

    // --- nu_quote_string: NUL should be dropped, not embedded ---

    #[test]
    fn nu_quote_string_nul_is_dropped_not_embedded() {
        // Embedding \u{0000} and passing it to the OS via execve truncates the path silently.
        // Drop NUL instead.
        let s = nu_quote_string("run\x00ex");
        assert!(!s.contains("\\u{0000}"), "NUL must be dropped, not embedded as \\u{{0000}}: {s:?}");
        assert!(!s.contains('\0'), "literal NUL must not appear: {s:?}");
        assert!(s.contains("runex"), "remaining chars must be preserved: {s:?}");
    }

    #[test]
    fn nu_quote_string_embedded_preserves_non_ascii_unicode() {
        // nu_quote_string_embedded processes the output of nu_quote_string byte-by-byte.
        // If nu_quote_string passes through a non-ASCII character (e.g. U+00E9 = 'e' with accent),
        // the embedded form must contain the same valid UTF-8 character, not corrupted bytes.
        // This guards against the `bytes[i] as char` antipattern that produces garbage from
        // multi-byte UTF-8 continuation bytes (e.g. 0xC3 → 'A\u0303' instead of 'a\u0301').
        let input = "caf\u{00E9}"; // U+00E9 encodes as the two-byte sequence [0xC3, 0xA9] in UTF-8
        let embedded = nu_quote_string_embedded(input);
        // The embedded form must be valid UTF-8 (no split continuation bytes)
        assert!(
            std::str::from_utf8(embedded.as_bytes()).is_ok(),
            "nu_quote_string_embedded must produce valid UTF-8: {embedded:?}"
        );
        // The non-ASCII character must survive intact
        assert!(
            embedded.contains('\u{00E9}'),
            "nu_quote_string_embedded must preserve non-ASCII char U+00E9: {embedded:?}"
        );
    }

    // --- pwsh_quote_string: backtick-concat safety ---

    #[test]
    fn pwsh_quote_string_newline_not_using_backtick_concat() {
        // 'a'`n'b' risks token-splitting in some PowerShell execution contexts.
        // Control chars are dropped instead.
        let s = pwsh_quote_string("run\nex");
        assert!(!s.contains('\n'), "literal newline must not appear: {s:?}");
        assert!(!s.contains("'`"), "backtick-concat form must not be used (token split risk): {s:?}");
    }

    } // mod quote_functions

    mod regression_issues {
        use super::*;

    #[test]
    fn clink_script_double_quote_in_bin_does_not_inject_into_popen() {
        // RUNEX_BIN value with a double quote must not break the io.popen shell command.
        // The io.popen call wraps RUNEX_BIN in double quotes at runtime:
        //   '"' .. RUNEX_BIN .. '" expand ...'
        // If RUNEX_BIN contains a literal " it terminates the shell double-quote, injecting code.
        // Fix: use single-quote wrapping in the shell command, with ' escaped as '\''
        let s = export_script(Shell::Clink, "run\"ex", Some(&Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![],
        }));
        // The generated script must not produce a command string that embeds a bare "
        // in a position that would close the outer shell double-quote.
        // Simplest check: the io.popen command line must use single-quote wrapping.
        assert!(
            !s.contains(r#"'"' .. RUNEX_BIN .. '"'"#),
            "io.popen must not wrap RUNEX_BIN in shell double-quotes: {s}"
        );
    }

    #[test]
    fn clink_script_bin_with_double_quote_uses_single_quote_shell_wrapping() {
        // The io.popen command must wrap RUNEX_BIN with single quotes so that
        // a double quote in the bin value cannot terminate a shell double-quoted string.
        let s = export_script(Shell::Clink, "run\"ex", Some(&Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![],
        }));
        // The popen command should use single-quote shell wrapping via a helper
        assert!(
            s.contains("runex_shell_quote"),
            "clink script must use a shell-quoting helper for RUNEX_BIN in io.popen: {s}"
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
        // A '$' in bin would allow Nu variable interpolation inside a Nu double-quoted
        // string (e.g. $env.PATH). Must be escaped as \$ so Nu treats it as a literal.
        let s = nu_quote_string("run$exenv");
        // The raw '$' must not appear unescaped — only '\$' (backslash-dollar) is allowed.
        // We check that every '$' in the output is immediately preceded by '\'.
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

    #[test]
    fn nu_quote_string_embedded_escapes_dollar_sign() {
        // In a Nu double-quoted string (the outer `cmd: "..."` context):
        //   \\ → literal \
        //   \$ → literal $ (suppresses variable interpolation)
        //   \\$ → literal \ followed by variable interpolation of $var — UNSAFE
        //
        // nu_quote_string produces \$ for a literal $. When embedded, we must
        // represent \$ as \\\$ so the outer Nu parser sees \$ (literal $), not \\$.
        //
        // Chain for input "$":
        //   nu_quote_string("$") produces: ^"\$"
        //   The \$ sequence in the embedded form must become \\\$ so that the
        //   outer Nu double-quoted string delivers \$ to the inner context.
        let s = nu_quote_string_embedded("run$exenv");
        // The embedded form must NOT contain the two-character sequence \\ followed by $
        // because that would allow Nu variable interpolation in the outer cmd: string.
        let has_unsafe_dollar = s
            .as_bytes()
            .windows(2)
            .any(|w| w == b"\\$" && {
                // Check that the preceding char is also backslash (making it \\$)
                false // checked via windows(3) below
            });
        let _ = has_unsafe_dollar;
        // More precisely: find any $-preceded-only-by-even-number-of-backslashes
        // A $ is "unprotected" if the number of immediately preceding \ is even (incl. 0).
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

    #[test]
    fn nu_quote_string_drops_remaining_c0_control_chars() {
        // \x01–\x08, \x0b, \x0c, \x0e–\x1f must be dropped (not passed through raw).
        // \n(\x0a), \r(\x0d), \t(\x09) are escaped to \\n/\\r/\\t, not dropped.
        let dangerous_c0: &[char] = &[
            '\x01', '\x02', '\x03', '\x04', '\x05', '\x06', '\x07', // BEL
            '\x08', '\x0b', '\x0c', '\x0e', '\x0f',
            '\x10', '\x11', '\x12', '\x13', '\x14', '\x15', '\x16', '\x17',
            '\x18', '\x19', '\x1a', '\x1b', // ESC
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
    fn pwsh_script_binds_shift_space_to_self_insert_when_trigger_is_space() {
        // When the trigger key is Space, Shift+Space must be explicitly bound to
        // SelfInsert so that the user can bypass expansion by pressing Shift+Space.
        // PSReadLine does not automatically bind Shift+Spacebar when Spacebar is rebound.
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                trigger: Some(TriggerKey::Space),
                ..crate::model::KeybindConfig::default()
            },
            abbr: vec![],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(
            s.contains("Set-PSReadLineKeyHandler -Chord 'Shift+Spacebar' -Function SelfInsert"),
            "pwsh script must bind Shift+Spacebar to SelfInsert when trigger is Space: {s}"
        );
    }

    #[test]
    fn pwsh_script_does_not_bind_shift_space_when_trigger_is_not_space() {
        // When the trigger key is not Space (e.g. Tab), there is no need to
        // bind Shift+Spacebar — doing so would clobber the default behaviour for no benefit.
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                trigger: Some(TriggerKey::Tab),
                ..crate::model::KeybindConfig::default()
            },
            abbr: vec![],
        };
        let s = export_script(Shell::Pwsh, "runex", Some(&config));
        assert!(
            !s.contains("Set-PSReadLineKeyHandler -Chord 'Shift+Spacebar' -Function SelfInsert"),
            "pwsh script must not bind Shift+Spacebar when trigger is not Space: {s}"
        );
    }

    } // mod regression_issues

    mod unicode_edge_cases {
        use super::*;

    #[test]
    fn bash_quote_pattern_drops_unicode_line_separators() {
        for ch in ['\u{0085}', '\u{2028}', '\u{2029}'] {
            let input = format!("key{ch}end");
            let s = bash_quote_pattern(&input);
            assert!(!s.contains(ch), "bash_quote_pattern must drop U+{:04X}: {s:?}", ch as u32);
        }
    }


    #[test]
    fn lua_quote_string_drops_del() {
        let s = lua_quote_string("run\x7fex");
        assert!(!s.contains('\x7f'), "lua_quote_string must drop DEL: {s:?}");
    }

    #[test]
    fn lua_quote_string_drops_unicode_line_separators() {
        for ch in ['\u{0085}', '\u{2028}', '\u{2029}'] {
            let input = format!("run{ch}ex");
            let s = lua_quote_string(&input);
            assert!(!s.contains(ch), "lua_quote_string must drop U+{:04X}: {s:?}", ch as u32);
        }
    }

    #[test]
    fn lua_quote_string_decimal_escape_not_ambiguous_with_following_digit() {
        // \x01 followed by "0": naive format!("\\{}", 1) produces "\1" + "0" = "\10" in Lua (LF).
        // Must use 3-digit zero-padded form "\001" so Lua reads \001 + "0".
        let s = lua_quote_string("\x010");
        // "\10" is LF in Lua (decimal 10). The result must NOT contain that sequence.
        assert!(
            !s.contains("\\10"),
            "lua_quote_string: \\x01 + '0' must not produce ambiguous \\10: {s:?}"
        );
        // Must use the 3-digit form.
        assert!(
            s.contains("\\001"),
            "lua_quote_string: \\x01 must be escaped as \\001: {s:?}"
        );
    }

    } // mod unicode_edge_cases

    /// In bash/zsh `case` patterns, characters enclosed in single quotes are
    /// treated as literals, not glob wildcards. `'*'` matches only a literal
    /// asterisk, not every string. `bash_quote_pattern` wraps keys in single
    /// quotes, so `*`, `?`, `[...]` are all safe in case patterns.
    mod case_pattern_globs {
        use super::*;

    #[test]
    fn bash_case_pattern_star_key_matches_only_literal_star() {
        // key="*" produces `'*') return 0 ;;` — in bash case, single-quoted
        // '*' is a literal match, not a glob. Only the token "*" itself matches.
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "*".into(),
                expand: "echo star".into(),
                when_command_exists: None,
            }],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        // The quoted pattern must appear in the script.
        assert!(
            s.contains("        '*') return 0 ;;"),
            "bash case must embed the single-quoted star key: {s}"
        );
        // The catch-all fall-through must also be present.
        assert!(s.contains("*) return 1 ;;"), "bash case must have a catch-all *) return 1 ;; arm");
    }

    #[test]
    fn bash_case_pattern_question_key_is_literal() {
        // Single-quoted '?' in a bash case pattern matches only a literal '?'.
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "g?".into(),
                expand: "git".into(),
                when_command_exists: None,
            }],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(
            s.contains("        'g?') return 0 ;;"),
            "bash case must embed the single-quoted key with '?': {s}"
        );
    }

    #[test]
    fn bash_case_pattern_bracket_key_is_literal() {
        // Single-quoted '[cm]' in a bash case pattern is literal, not a character class.
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "g[cm]".into(),
                expand: "git".into(),
                when_command_exists: None,
            }],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(
            s.contains("        'g[cm]') return 0 ;;"),
            "bash case must embed the single-quoted bracket key literally: {s}"
        );
    }

    #[test]
    fn zsh_case_pattern_star_key_matches_only_literal_star() {
        // Same as bash: single-quoted '*' in zsh case is literal.
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "*".into(),
                expand: "echo star".into(),
                when_command_exists: None,
            }],
        };
        let s = export_script(Shell::Zsh, "runex", Some(&config));
        assert!(
            s.contains("        '*') return 0 ;;"),
            "zsh case must embed the single-quoted star key: {s}"
        );
        assert!(s.contains("*) return 1 ;;"), "zsh case must have a catch-all *) return 1 ;; arm");
    }

    #[test]
    fn pwsh_script_has_single_default_clause() {
        // Regression: empty abbr list used to emit a duplicate `default` clause
        // inside the switch statement, causing a PowerShell parse error.
        for abbr in [vec![], vec![crate::model::Abbr {
            key: "gcm".into(),
            expand: "git commit -m".into(),
            when_command_exists: None,
        }]] {
            let s = export_script(Shell::Pwsh, "runex", Some(&Config {
                version: 1,
                keybind: crate::model::KeybindConfig::default(),
                abbr,
            }));
            let default_count = s.matches("default {").count();
            assert_eq!(default_count, 1, "pwsh script must have exactly one default clause, got {default_count}");
        }
    }

    } // mod case_pattern_globs
}
