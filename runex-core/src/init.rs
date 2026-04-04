use std::path::PathBuf;

use crate::config::xdg_config_home;
use crate::sanitize::{double_quote_escape, is_nu_drop_char};
use crate::shell::{bash_quote_string, lua_quote_string, nu_quote_string, pwsh_quote_string, Shell};

/// Quote a filesystem path for embedding in a Nu shell string literal.
///
/// Uses Nu double-quoted string syntax (no `^` prefix — this is a string value,
/// not a command invocation).  Escapes `\`, `"`, and `$` (to suppress variable
/// interpolation of `$env.FOO` etc.).  NUL, DEL, ASCII control characters, Unicode
/// line/paragraph separators, and visual-deception characters are all dropped.
fn nu_quote_path(path: &str) -> String {
    let mut out = String::from("\"");
    for ch in path.chars() {
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

/// Marker comment written into rc files to enable idempotent init.
pub const RUNEX_INIT_MARKER: &str = "# runex-init";

/// Minimal config template written by `runex init`.
pub fn default_config_content() -> &'static str {
    r#"version = 1

# Add your abbreviations below.
# [[abbr]]
# key = "gcm"
# expand = "git commit -m"
"#
}

/// The single line appended to the shell rc file.
pub fn integration_line(shell: Shell, bin: &str) -> String {
    match shell {
        Shell::Bash => format!("eval \"$({} export bash)\"", bash_quote_string(bin)),
        Shell::Zsh => format!("eval \"$({} export zsh)\"", bash_quote_string(bin)),
        Shell::Pwsh => format!(
            "Invoke-Expression (& {} export pwsh | Out-String)",
            pwsh_quote_string(bin)
        ),
        Shell::Nu => {
            let cfg_dir = xdg_config_home()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "~/.config".to_string());
            let nu_bin = nu_quote_string(bin);
            let nu_path = nu_quote_path(&format!("{cfg_dir}/runex/runex.nu"));
            format!(
                "{nu_bin} export nu | save --force {nu_path}\nsource {nu_path}"
            )
        }
        Shell::Clink => format!(
            "-- add {} export clink output to your clink scripts directory",
            lua_quote_string(bin)
        ),
    }
}

/// The rc file path for a given shell (best-effort; may not exist yet).
pub fn rc_file_for(shell: Shell) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    match shell {
        Shell::Bash => Some(home.join(".bashrc")),
        Shell::Zsh => Some(home.join(".zshrc")),
        Shell::Pwsh => {
            // $PROFILE is a runtime variable; use the conventional Windows path.
            let base = if cfg!(windows) {
                home.join("Documents").join("PowerShell")
            } else {
                home.join(".config").join("powershell")
            };
            Some(base.join("Microsoft.PowerShell_profile.ps1"))
        }
        Shell::Nu => {
            let cfg = xdg_config_home().unwrap_or_else(|| home.join(".config"));
            Some(cfg.join("nushell").join("env.nu"))
        }
        Shell::Clink => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod integration_line {
        use super::*;

    #[test]
    fn default_config_content_has_version() {
        assert!(default_config_content().contains("version = 1"));
    }

    #[test]
    fn integration_line_bash() {
        assert_eq!(
            integration_line(Shell::Bash, "runex"),
            r#"eval "$('runex' export bash)""#
        );
    }

    #[test]
    fn integration_line_pwsh() {
        let line = integration_line(Shell::Pwsh, "runex");
        assert!(line.contains("Invoke-Expression"));
        assert!(line.contains("'runex' export pwsh"));
    }

    /// `bin = "run'ex"` must not break out of the eval context.
    #[test]
    fn integration_line_bash_escapes_single_quote_in_bin() {
        let line = integration_line(Shell::Bash, "run'ex");
        assert!(!line.contains("run'ex"), "unescaped quote in bash line: {line}");
        assert!(line.contains(r"run'\''ex"), "expected bash-escaped form: {line}");
    }

    #[test]
    fn integration_line_zsh_escapes_single_quote_in_bin() {
        let line = integration_line(Shell::Zsh, "run'ex");
        assert!(!line.contains("run'ex"), "unescaped quote in zsh line: {line}");
        assert!(line.contains(r"run'\''ex"), "expected zsh-escaped form: {line}");
    }

    /// PowerShell doubles single quotes inside single-quoted strings.
    #[test]
    fn integration_line_pwsh_escapes_single_quote_in_bin() {
        let line = integration_line(Shell::Pwsh, "run'ex");
        assert!(!line.contains("run'ex"), "unescaped quote in pwsh line: {line}");
        assert!(line.contains("run''ex"), "expected pwsh-escaped form: {line}");
    }

    /// `bin = "app; echo PWNED"` — semicolon must be enclosed in single quotes,
    /// neutralising `;` to prevent command injection.
    #[test]
    fn integration_line_bash_semicolon_does_not_inject() {
        let line = integration_line(Shell::Bash, "app; echo PWNED");
        assert!(
            line.contains("'app; echo PWNED'"),
            "bin must be single-quoted in bash line: {line}"
        );
    }

    /// `bin = "app; Write-Host PWNED"` — semicolon must be enclosed in single quotes.
    #[test]
    fn integration_line_pwsh_semicolon_does_not_inject() {
        let line = integration_line(Shell::Pwsh, "app; Write-Host PWNED");
        assert!(
            line.contains("'app; Write-Host PWNED'"),
            "bin must be single-quoted in pwsh line: {line}"
        );
    }

    /// Nu: quoting without `^` makes a string, not a command — must use `^"runex"`.
    #[test]
    fn integration_line_nu_uses_caret_external_command_syntax() {
        let line = integration_line(Shell::Nu, "runex");
        assert!(
            line.contains("^\"runex\""),
            "nu integration line must use ^\"...\" syntax: {line}"
        );
    }

    #[test]
    fn integration_line_nu_escapes_special_chars_in_bin() {
        let line = integration_line(Shell::Nu, "my\"app");
        assert!(line.contains("^\"my\\\"app\""), "nu: special chars must be escaped: {line}");
    }

    /// `nu_quote_path` must wrap the path in double quotes so Nu doesn't tokenize on spaces.
    #[test]
    fn integration_line_nu_quotes_cfg_dir_with_spaces() {
        let quoted = nu_quote_path("/home/my user/.config");
        assert_eq!(quoted, "\"/home/my user/.config\"");
        assert!(!quoted.starts_with('/'), "path must be quoted, not raw");
    }

    /// Windows paths: backslashes must be escaped inside Nu double-quoted strings.
    #[test]
    fn integration_line_nu_quotes_cfg_dir_with_backslash() {
        let quoted = nu_quote_path(r"C:\Users\my user\AppData");
        assert_eq!(quoted, r#""C:\\Users\\my user\\AppData""#);
    }

    /// The generated Nu integration line must quote the runex.nu path.
    /// Both `save` and `source` must use quoted paths (starting with `"`).
    #[test]
    fn integration_line_nu_save_path_is_quoted() {
        let line = integration_line(Shell::Nu, "runex");
        for fragment in ["save --force \"", "source \""] {
            assert!(
                line.contains(fragment),
                "nu line must contain `{fragment}`: {line}"
            );
        }
    }

    /// A single quote in bin must be passed through `lua_quote_string`.
    /// `lua_quote_string("run'ex")` = `"run'ex"` (single quotes need no escaping in Lua double-quoted strings).
    #[test]
    fn integration_line_clink_single_quote_in_bin_is_lua_quoted() {
        let line = integration_line(Shell::Clink, "run'ex");
        assert!(
            line.contains("\"run'ex\""),
            "bin must be lua-quoted in clink line: {line}"
        );
    }

    /// `lua_quote_string` escapes `\n` to `\\n`, preventing the Lua comment from being broken.
    #[test]
    fn integration_line_clink_newline_in_bin_does_not_inject() {
        let line = integration_line(Shell::Clink, "runex\nos.execute('evil')");
        assert!(
            !line.contains('\n'),
            "literal newline must be escaped in clink line: {line:?}"
        );
        assert!(
            line.contains("\\n"),
            "expected \\n escape sequence in clink line: {line:?}"
        );
    }

    } // mod integration_line

    /// Control-character and NUL handling in `nu_quote_path`.
    ///
    /// `nu_quote_path` embeds paths into Nu double-quoted string literals.
    /// Newlines, carriage returns, tabs, NUL, DEL, and Unicode line separators
    /// must be escaped or dropped so they cannot break out of the string context.
    mod nu_quote_path_escaping {
        use super::*;

    #[test]
    fn nu_quote_path_escapes_newline() {
        let quoted = nu_quote_path("/home/user/.config\nevil");
        assert!(!quoted.contains('\n'), "nu_quote_path must escape newline: {quoted}");
        assert!(quoted.contains("\\n"), "expected \\n escape: {quoted}");
    }

    #[test]
    fn nu_quote_path_escapes_carriage_return() {
        let quoted = nu_quote_path("/path\r/evil");
        assert!(!quoted.contains('\r'), "nu_quote_path must escape CR: {quoted}");
        assert!(quoted.contains("\\r"), "expected \\r escape: {quoted}");
    }

    /// If XDG_CONFIG_HOME contains a newline, it must not inject Nu statements into env.nu.
    #[test]
    fn integration_line_nu_newline_in_xdg_does_not_inject() {
        let quoted = nu_quote_path("/home/user/.config\nsource /tmp/evil.nu\n#");
        assert!(!quoted.contains('\n'), "newline injection must be escaped in nu path: {quoted}");
    }

    #[test]
    fn nu_quote_path_escapes_nul() {
        let quoted = nu_quote_path("path\x00evil");
        assert!(!quoted.contains('\0'), "nu_quote_path must not produce literal NUL: {quoted:?}");
        assert!(quoted.contains("path"), "path prefix must be preserved: {quoted:?}");
    }

    #[test]
    fn nu_quote_path_escapes_tab() {
        let quoted = nu_quote_path("path\t/evil");
        assert!(!quoted.contains('\t'), "nu_quote_path must escape tab: {quoted:?}");
        assert!(quoted.contains("\\t"), "expected \\t escape: {quoted:?}");
    }

    #[test]
    fn nu_quote_path_drops_del() {
        let quoted = nu_quote_path("path\x7fend");
        assert!(!quoted.contains('\x7f'), "nu_quote_path must drop DEL: {quoted:?}");
    }

    #[test]
    fn nu_quote_path_drops_unicode_line_separators() {
        for ch in ['\u{0085}', '\u{2028}', '\u{2029}'] {
            let input = format!("path{ch}end");
            let quoted = nu_quote_path(&input);
            assert!(!quoted.contains(ch), "nu_quote_path must drop U+{:04X}: {quoted:?}", ch as u32);
        }
    }

    #[test]
    fn rc_file_for_bash_ends_with_bashrc() {
        if let Some(path) = rc_file_for(Shell::Bash) {
            assert!(path.to_str().unwrap().ends_with(".bashrc"));
        }
    }

    /// C0 control chars other than `\n`, `\r`, `\t`, `\0`, `\x7f` must be dropped.
    #[test]
    fn nu_quote_path_drops_remaining_c0_control_chars() {
        let dangerous_c0: &[char] = &[
            '\x01', '\x02', '\x03', '\x04', '\x05', '\x06', '\x07',
            '\x08', '\x0b', '\x0c', '\x0e', '\x0f',
            '\x10', '\x11', '\x12', '\x13', '\x14', '\x15', '\x16', '\x17',
            '\x18', '\x19', '\x1a', '\x1b',
            '\x1c', '\x1d', '\x1e', '\x1f',
        ];
        for &ch in dangerous_c0 {
            let input = format!("path{}end", ch);
            let quoted = nu_quote_path(&input);
            assert!(
                !quoted.contains(ch),
                "nu_quote_path must drop C0 control U+{:04X}: {quoted:?}",
                ch as u32
            );
        }
    }

    } // mod nu_quote_path_escaping

    /// `nu_quote_path` embeds the XDG_CONFIG_HOME path into the `source "..."` line
    /// written to env.nu. If XDG_CONFIG_HOME contains Unicode visual-deception
    /// characters (RLO, BOM, ZWSP, etc.), the displayed `source` path would appear
    /// different from its actual content, potentially deceiving the user into
    /// thinking a safe path is being sourced. These characters must be dropped.
    mod nu_quote_path_deceptive {
        use super::*;

    /// U+202E (Right-to-Left Override) reverses display order in the terminal.
    #[test]
    fn nu_quote_path_drops_rlo() {
        let quoted = nu_quote_path("/home/user\u{202E}/.config");
        assert!(
            !quoted.contains('\u{202E}'),
            "nu_quote_path must drop U+202E (RLO): {quoted:?}"
        );
    }

    /// U+FEFF (BOM / zero-width no-break space) is invisible.
    #[test]
    fn nu_quote_path_drops_bom() {
        let quoted = nu_quote_path("/home/user\u{FEFF}/.config");
        assert!(
            !quoted.contains('\u{FEFF}'),
            "nu_quote_path must drop U+FEFF (BOM): {quoted:?}"
        );
    }

    /// U+200B (Zero-Width Space) is invisible.
    #[test]
    fn nu_quote_path_drops_zwsp() {
        let quoted = nu_quote_path("/home/user\u{200B}/.config");
        assert!(
            !quoted.contains('\u{200B}'),
            "nu_quote_path must drop U+200B (ZWSP): {quoted:?}"
        );
    }

    /// Non-deceptive Unicode (e.g. Japanese path components) must pass through.
    #[test]
    fn nu_quote_path_preserves_non_deceptive_unicode() {
        let quoted = nu_quote_path("/home/ユーザー/.config");
        assert!(
            quoted.contains("ユーザー"),
            "nu_quote_path must preserve non-deceptive Unicode: {quoted:?}"
        );
    }

    /// A `$` in XDG_CONFIG_HOME would allow Nu variable interpolation inside
    /// the double-quoted `source "..."` path. Must be escaped as `\$`.
    #[test]
    fn nu_quote_path_escapes_dollar_sign() {
        let quoted = nu_quote_path("/home/$USER/.config");
        // Every '$' must be preceded by an odd number of backslashes.
        let bytes = quoted.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'$' {
                let mut preceding = 0usize;
                let mut j = i;
                while j > 0 && bytes[j - 1] == b'\\' {
                    preceding += 1;
                    j -= 1;
                }
                assert!(
                    preceding % 2 == 1,
                    "nu_quote_path: '$' at byte {i} has {preceding} preceding backslashes \
                     (even = Nu interpolation not suppressed). Full output: {quoted:?}"
                );
            }
        }
        assert!(quoted.contains("\\$"), "expected \\$ in: {quoted:?}");
    }

    } // mod nu_quote_path_deceptive

}
