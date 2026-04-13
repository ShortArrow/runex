use std::fmt;
use std::str::FromStr;

use crate::model::{Config, TriggerKey};
use crate::sanitize::{double_quote_escape, is_nu_drop_char, is_unicode_line_separator, is_unsafe_for_display};

// Shell is defined in model to avoid circular dependency; re-export it here
// so callers that do `use runex_core::shell::Shell` still work.
pub use crate::model::Shell;

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

/// Error returned when a shell name string cannot be parsed into a [`Shell`] variant.
///
/// The `Display` impl sanitizes the raw shell name before embedding it in the message:
/// ASCII control characters and Unicode visual-deception characters (directional overrides,
/// BOM, zero-width chars) are stripped to prevent terminal injection via crafted error output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellParseError(pub String);

impl fmt::Display for ShellParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
        TriggerKey::ShiftSpace => unreachable!("ShiftSpace cannot be used as a trigger in pwsh"),
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
                    out.push('\\');
                    out.push('$');
                    chars.next();
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
        TriggerKey::ShiftSpace => "shift",
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

/// Generate the `bind` lines for bash, removing the old binding before adding the new one.
/// Only the configured trigger key is touched; other keys are left as-is.
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
            include_str!("templates/nu_expand_binding.nu")
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
    include_str!("templates/nu_self_insert_binding.nu")
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
pub fn export_script(shell: Shell, bin: &str, config: Option<&Config>) -> String {
    let template = match shell {
        Shell::Bash => include_str!("templates/bash.sh"),
        Shell::Zsh => include_str!("templates/zsh.zsh"),
        Shell::Pwsh => include_str!("templates/pwsh.ps1"),
        Shell::Clink => include_str!("templates/clink.lua"),
        Shell::Nu => include_str!("templates/nu.nu"),
    };
    let trigger = trigger_for(shell, config);
    let self_insert = self_insert_for(shell, config);
    template
        .replace("\r\n", "\n")
        .replace("{BASH_BIN}", &bash_quote_string(bin))
        .replace("{BASH_BIND_LINES}", &bash_bind_lines(trigger))
        .replace("{BASH_SELF_INSERT_LINES}", &bash_self_insert_lines(self_insert))
        .replace("{BASH_KNOWN_CASES}", &bash_known_cases(config))
        .replace("{ZSH_BIN}", &bash_quote_string(bin))
        .replace("{ZSH_BIND_LINES}", &zsh_bind_lines(trigger))
        .replace("{ZSH_SELF_INSERT_LINES}", &zsh_self_insert_lines(self_insert))
        .replace("{ZSH_KNOWN_CASES}", &zsh_known_cases(config))
        .replace("{CLINK_BIN}", &lua_quote_string(bin))
        .replace("{CLINK_BINDING}", &clink_binding(trigger))
        .replace("{CLINK_KNOWN_CASES}", &clink_known_cases(config))
        .replace("{PWSH_BIN}", &pwsh_quote_string(bin))
        .replace("{PWSH_REGISTER_LINES}", &pwsh_register_lines(trigger))
        .replace("{PWSH_SELF_INSERT_LINES}", &pwsh_self_insert_lines(self_insert))
        .replace("{PWSH_KNOWN_CASES}", &pwsh_known_cases(config))
        .replace("{NU_BINDINGS}", &nu_bindings(trigger, bin))
        .replace("{NU_SELF_INSERT_BINDINGS}", &nu_self_insert_lines(self_insert))
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
                trigger: crate::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
                    trigger: crate::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::model::KeybindConfig::default()
                },
                precache: crate::model::PrecacheConfig::default(),
                abbr: vec![],
            }),
        );
        assert!(s.contains("bind -x"), "bash script must use bind");
        assert!(s.contains(r#"bind -r "\x20""#), "bash script must remove the space binding before rebinding");
        assert!(s.contains("raw=$('runex' expand"), "bash script must quote the executable");
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
                    trigger: crate::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::model::KeybindConfig::default()
                },
                precache: crate::model::PrecacheConfig::default(),
                abbr: vec![],
            }),
        );
        assert!(s.contains("Set-PSReadLineKeyHandler"), "pwsh script must use PSReadLine");
        assert!(
            !s.contains("Set-PSReadLineKeyHandler -Chord 'Tab' -Function Complete"),
            "pwsh script must not clobber the user's Tab binding"
        );
        assert!(s.contains("$raw = & 'runex' expand"), "pwsh script must quote the executable");
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
                    trigger: crate::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::model::KeybindConfig::default()
                },
                precache: crate::model::PrecacheConfig::default(),
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
                    trigger: crate::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::model::KeybindConfig::default()
                },
                precache: crate::model::PrecacheConfig::default(),
                abbr: vec![],
            }),
        );
        assert!(s.contains("zle -N __runex_expand"), "zsh script must register a zle widget");
        assert!(s.contains(r#"bindkey " " __runex_expand"#), "zsh script must bind the trigger key");
        assert!(s.contains("__runex_expand_buffer"), "zsh script must expose a testable helper");
        assert!(s.contains("LBUFFER"), "zsh script must inspect the text before the cursor");
        assert!(s.contains("RBUFFER"), "zsh script must inspect the text after the cursor");
        assert!(s.contains("raw=$('runex' expand"), "zsh script must quote the executable");
    }

    #[test]
    fn clink_script_has_clink() {
        let s = export_script(
            Shell::Clink,
            "runex",
            Some(&Config {
                version: 1,
                keybind: crate::model::KeybindConfig {
                    trigger: crate::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::model::KeybindConfig::default()
                },
                precache: crate::model::PrecacheConfig::default(),
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
                trigger: crate::model::PerShellKey {
                    default: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
                    trigger: crate::model::PerShellKey {
                        default: Some(TriggerKey::Space),
                        ..Default::default()
                    },
                    ..crate::model::KeybindConfig::default()
                },
                precache: crate::model::PrecacheConfig::default(),
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
                trigger: crate::model::PerShellKey {
                    bash: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
        use crate::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig {
                trigger: crate::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                trigger: crate::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "gcm".into(),
                expand: crate::model::PerShellString::All("git commit -m".into()),
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
                trigger: crate::model::PerShellKey {
                    default: Some(TriggerKey::Tab),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
                trigger: crate::model::PerShellKey {
                    pwsh: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "gcm".into(),
                expand: crate::model::PerShellString::All("git commit -m".into()),
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
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![],
        }));
        assert!(!s.contains("bind -x"), "bash script should not bind keys by default");
        assert!(!s.contains(r#"bind -r"#), "bash script should not remove keybinds when no trigger is configured");

        let s = export_script(Shell::Pwsh, "runex", Some(&Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            precache: crate::model::PrecacheConfig::default(),
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
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: KeybindConfig {
                trigger: crate::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: KeybindConfig {
                trigger: crate::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: KeybindConfig {
                trigger: crate::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: KeybindConfig {
                trigger: crate::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Nu, "runex", Some(&config));
        assert!(
            !s.contains("--token $token"),
            "Nu script must not use space-separated --token (argument injection risk): {s}"
        );
        assert!(
            !s.contains("$\"--token=($token)\"") && !s.contains("\"--token=("),
            "Nu script must not use string interpolation for --token: {s}"
        );
        assert!(
            s.contains("--token=($token)"),
            "Nu script must use --token=($token) form to prevent argument injection: {s}"
        );
    }

    #[test]
    fn nu_bin_newline_does_not_inject_into_cmd_block() {
        use crate::model::{Config, KeybindConfig, TriggerKey};
        let config = Config {
            version: 1,
            keybind: KeybindConfig {
                trigger: crate::model::PerShellKey { default: Some(TriggerKey::Space), ..Default::default() },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let s = export_script(Shell::Nu, "runex\nsource /tmp/evil.nu\n", Some(&config));
        let lines: Vec<&str> = s.lines().collect();
        assert!(
            !lines.iter().any(|l| l.trim() == "source /tmp/evil.nu"),
            "newline must not create an injected source line: {s}"
        );
    }

    #[test]
    fn bash_quote_string_drops_newline() {
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

    #[test]
    fn nu_quote_string_nul_is_dropped_not_embedded() {
        let s = nu_quote_string("run\x00ex");
        assert!(!s.contains("\\u{0000}"), "NUL must be dropped, not embedded as \\u{{0000}}: {s:?}");
        assert!(!s.contains('\0'), "literal NUL must not appear: {s:?}");
        assert!(s.contains("runex"), "remaining chars must be preserved: {s:?}");
    }

    /// Guards against the `bytes[i] as char` antipattern: processing byte-by-byte splits
    /// multi-byte UTF-8 sequences (e.g. U+00E9 = [0xC3, 0xA9]), producing corrupted output.
    #[test]
    fn nu_quote_string_embedded_preserves_non_ascii_unicode() {
        let input = "caf\u{00E9}";
        let embedded = nu_quote_string_embedded(input);
        assert!(
            std::str::from_utf8(embedded.as_bytes()).is_ok(),
            "nu_quote_string_embedded must produce valid UTF-8: {embedded:?}"
        );
        assert!(
            embedded.contains('\u{00E9}'),
            "nu_quote_string_embedded must preserve non-ASCII char U+00E9: {embedded:?}"
        );
    }

    #[test]
    fn pwsh_quote_string_newline_not_using_backtick_concat() {
        let s = pwsh_quote_string("run\nex");
        assert!(!s.contains('\n'), "literal newline must not appear: {s:?}");
        assert!(!s.contains("'`"), "backtick-concat form must not be used (token split risk): {s:?}");
    }

    } // mod quote_functions

    mod regression_issues {
        use super::*;

    /// A `"` in bin must not terminate the shell double-quoted string inside `io.popen`.
    /// The fix is single-quote wrapping (with `'\''` for embedded single quotes).
    #[test]
    fn clink_script_double_quote_in_bin_does_not_inject_into_popen() {
        let s = export_script(Shell::Clink, "run\"ex", Some(&Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig::default(),
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![],
        }));
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
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    pwsh: Some(TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    pwsh: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                trigger: crate::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    nu: Some(TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    nu: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                trigger: crate::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    bash: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                trigger: crate::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    zsh: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                trigger: crate::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                trigger: crate::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    bash: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                trigger: crate::model::PerShellKey {
                    default: Some(TriggerKey::Tab),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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
            keybind: crate::model::KeybindConfig {
                trigger: crate::model::PerShellKey {
                    default: Some(TriggerKey::Space),
                    bash: Some(TriggerKey::AltSpace),
                    ..Default::default()
                },
                ..Default::default()
            },
            precache: crate::model::PrecacheConfig::default(),
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

    /// Naive `format!("\\{}", 1)` produces `"\1"` which Lua reads as `"\10"` (LF) when
    /// followed by `"0"`. Three-digit zero-padded `"\001"` avoids the ambiguity.
    #[test]
    fn lua_quote_string_decimal_escape_not_ambiguous_with_following_digit() {
        let s = lua_quote_string("\x010");
        assert!(
            !s.contains("\\10"),
            "lua_quote_string: \\x01 + '0' must not produce ambiguous \\10: {s:?}"
        );
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
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "*".into(),
                expand: crate::model::PerShellString::All("echo star".into()),
                when_command_exists: None,
            }],
        };
        let s = export_script(Shell::Bash, "runex", Some(&config));
        assert!(
            s.contains("        '*') return 0 ;;"),
            "bash case must embed the single-quoted star key: {s}"
        );
        assert!(s.contains("*) return 1 ;;"), "bash case must have a catch-all *) return 1 ;; arm");
    }

    #[test]
    fn bash_case_pattern_question_key_is_literal() {
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "g?".into(),
                expand: crate::model::PerShellString::All("git".into()),
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
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "g[cm]".into(),
                expand: crate::model::PerShellString::All("git".into()),
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
        let config = Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![crate::model::Abbr {
                key: "*".into(),
                expand: crate::model::PerShellString::All("echo star".into()),
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

    /// Regression: an empty abbr list previously emitted a duplicate `default` clause,
    /// causing a PowerShell parse error.
    #[test]
    fn pwsh_script_has_single_default_clause() {
        for abbr in [vec![], vec![crate::model::Abbr {
            key: "gcm".into(),
            expand: crate::model::PerShellString::All("git commit -m".into()),
            when_command_exists: None,
        }]] {
            let s = export_script(Shell::Pwsh, "runex", Some(&Config {
                version: 1,
                keybind: crate::model::KeybindConfig::default(),
            precache: crate::model::PrecacheConfig::default(),
                abbr,
            }));
            let default_count = s.matches("default {").count();
            assert_eq!(default_count, 1, "pwsh script must have exactly one default clause, got {default_count}");
        }
    }

    } // mod case_pattern_globs
}
