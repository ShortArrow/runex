use std::fmt;
use std::str::FromStr;

use crate::domain::sanitize::{double_quote_escape, is_nu_drop_char, is_unicode_line_separator, is_unsafe_for_display};

// Shell is defined in model to avoid circular dependency; re-export it here
// so callers that do `use crate::domain::shell::Shell` still work.
pub(crate) use crate::domain::model::Shell;

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

/// The lowercase name the CLI accepts for the shell (`runex init
/// <shell>`, `--shell <shell>`), so user-facing output can name a
/// shell in the form the user would type back. Round-trips through
/// [`FromStr`].
impl fmt::Display for Shell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Pwsh => "pwsh",
            Shell::Clink => "clink",
            Shell::Nu => "nu",
        })
    }
}

/// Error returned when a shell name string cannot be parsed into a [`Shell`] variant.
///
/// The `Display` impl sanitizes the raw shell name before embedding it in the message:
/// ASCII control characters and Unicode visual-deception characters (directional overrides,
/// BOM, zero-width chars) are stripped to prevent terminal injection via crafted error output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellParseError(pub String);

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


/// Quote `token` for use as a Bash `case` pattern.
///
/// Uses the same escaping as [`bash_quote_string`]: single-quoted with `'\''`
/// for embedded single quotes.  ASCII control characters and Unicode
/// line/paragraph separators are dropped.
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

/// Quote `value` as a PowerShell double-quoted string that evaluates back
/// to exactly `value`.
///
/// This is the literal for *buffer text* (`$__RUNEX_LINE`), where the
/// hook's cursor is measured against the full line and dropping a
/// character would land the cursor off by one (issue #21). Paths use
/// [`pwsh_quote_string`] instead.
///
/// - `` ` ``, `$` and the four characters PowerShell's tokenizer accepts
///   as a double quote (`"`, U+201C, U+201D, U+201E) get a backtick.
/// - LF and CR become `` `n `` / `` `r ``: pwsh splits native stdout on
///   them before the wrapper re-joins with a bare LF, so raw ones would
///   not survive the round trip. Every other character is inert inside
///   a double-quoted literal and is emitted as is — including the
///   single-quote delimiters U+2018 / U+2019 / U+201A / U+201B, which
///   only end a *single*-quoted literal.
///
/// Only backtick escapes that Windows PowerShell 5.1 understands are
/// used, since the same bootstrap runs there. The literal round-trips
/// on 5.1; what does not is 5.1's native-argument passing, which strips
/// a `"` from the buffer before `runex hook` ever receives it. That is
/// upstream of this function and unchanged by it.
pub(crate) fn pwsh_double_quote_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\n' => out.push_str("`n"),
            '\r' => out.push_str("`r"),
            '`' | '$' | '"' | '\u{201C}' | '\u{201D}' | '\u{201E}' => {
                out.push('`');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
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
pub(crate) fn nu_quote_string_embedded(value: &str) -> String {
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

/// Quote `value` as a Lua double-quoted string that evaluates back to
/// exactly `value`.
///
/// This is the literal for *buffer text* (`return { line = ... }`), and
/// the cursor the hook returns alongside it is a char count measured on
/// the buffer runex was given. Dropping a character would therefore
/// shorten the line and leave the cursor past the position it names
/// (issue #36, the clink counterpart of #21). Nothing is dropped:
///
/// - `\`, `"` → `\\`, `\"`
/// - `\n`, `\r`, `\t` → two-character escape sequences
/// - every character `is_unsafe_for_display` rejects — NUL and the
///   other ASCII controls, the Unicode line/paragraph separators
///   (U+0085, U+2028, U+2029), and the deceptive-Unicode set (RLO,
///   BOM, ZWSP and friends) — is emitted as its UTF-8 bytes in
///   three-digit decimal `\DDD` escapes.
///
/// Escaping rather than dropping also keeps the visual-deception
/// guarantee that dropping was there for: an escaped byte sequence is
/// plainly visible in the emitted text, so a crafted buffer cannot
/// produce a literal that reads as something other than what Lua will
/// build from it.
///
/// `\DDD` and not `\u{...}`: clink embeds Lua 5.2, which accepts `\ddd`
/// and `\xXX` but rejects `\u{...}` — the whole chunk would fail to
/// load and clink would fall back to inserting a bare space. Zero-
/// padding is required for the same reason it always was: without it
/// `\1` followed by `0` reads as `\10` (LF) rather than SOH followed by
/// `"0"`.
pub(crate) fn lua_quote_string(value: &str) -> String {
    let mut out = String::from("\"");
    let mut buf = [0u8; 4];
    for ch in value.chars() {
        if let Some(esc) = double_quote_escape(ch) {
            out.push_str(esc);
        } else if is_unsafe_for_display(ch) {
            for byte in ch.encode_utf8(&mut buf).as_bytes() {
                out.push_str(&format!("\\{byte:03}"));
            }
        } else {
            out.push(ch);
        }
    }
    out.push('"');
    out
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

    // `nu_hook_invocation_uses_separate_line_and_cursor_args` and
    // `nu_bin_newline_does_not_inject_into_cmd_block` moved to
    // `app::shell_export::tests` (Phase D D2b) — they assert on
    // `export_script` output, which is an app concern.

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

    // bash_quote_pattern tests dropped — the helper and its callers (the
    // case-arm builder) are gone now that abbreviations aren't embedded in
    // shell code.

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


    #[test]
    fn pwsh_double_quote_string_wraps_in_double_quotes() {
        assert_eq!(pwsh_double_quote_string("gs end"), "\"gs end\"");
    }

    #[test]
    fn pwsh_double_quote_string_escapes_backtick_quote_and_dollar() {
        assert_eq!(pwsh_double_quote_string("a`b"), "\"a``b\"");
        assert_eq!(pwsh_double_quote_string("a\"b"), "\"a`\"b\"");
        assert_eq!(pwsh_double_quote_string("a$b"), "\"a`$b\"");
    }

    #[test]
    fn pwsh_double_quote_string_escapes_typographic_double_quotes() {
        // PowerShell's tokenizer treats U+201C, U+201D and U+201E as
        // string delimiters, so unescaped they would end the literal.
        assert_eq!(pwsh_double_quote_string("a\u{201C}b"), "\"a`\u{201C}b\"");
        assert_eq!(pwsh_double_quote_string("a\u{201D}b"), "\"a`\u{201D}b\"");
        assert_eq!(pwsh_double_quote_string("a\u{201E}b"), "\"a`\u{201E}b\"");
    }

    #[test]
    fn pwsh_double_quote_string_encodes_newline_and_carriage_return() {
        // pwsh splits native stdout on CR / LF before the wrapper joins the
        // lines back with a bare LF, so either one raw in the eval text
        // would come back altered. The backtick escapes survive the trip.
        assert_eq!(pwsh_double_quote_string("a\nb"), "\"a`nb\"");
        assert_eq!(pwsh_double_quote_string("a\r\nb"), "\"a`r`nb\"");
    }

    #[test]
    fn pwsh_double_quote_string_keeps_every_other_char_verbatim() {
        let line = r"scp -r \\srv\開発\帳票（レポート）\x 'y'";
        assert_eq!(pwsh_double_quote_string(line), format!("\"{line}\""));
    }

    } // mod quote_functions

    mod shell_name {
        use super::*;

        /// `Display` must print exactly the token `FromStr` accepts, so
        /// a shell named in output (`runex init <shell>`) can be typed
        /// back verbatim.
        #[test]
        fn display_round_trips_through_from_str() {
            for shell in [Shell::Bash, Shell::Zsh, Shell::Pwsh, Shell::Clink, Shell::Nu] {
                let name = shell.to_string();
                assert_eq!(name, name.to_ascii_lowercase(), "names are lowercase: {name}");
                assert_eq!(name.parse::<Shell>().unwrap(), shell);
            }
        }
    }


    mod unicode_edge_cases {
        use super::*;

    #[test]
    fn lua_quote_string_drops_del() {
        let s = lua_quote_string("run\x7fex");
        assert!(!s.contains('\x7f'), "lua_quote_string must drop DEL: {s:?}");
    }

    /// The clink cursor is a char count measured on the buffer runex
    /// received, so a renderer that drops a character moves the cursor
    /// (issue #36, mirroring #21). Separators are escaped as their
    /// UTF-8 bytes in decimal, never dropped.
    #[test]
    fn lua_quote_string_escapes_unicode_line_separators() {
        assert_eq!(lua_quote_string("run\u{0085}ex"), "\"run\\194\\133ex\"");
        assert_eq!(lua_quote_string("run\u{2028}ex"), "\"run\\226\\128\\168ex\"");
        assert_eq!(lua_quote_string("run\u{2029}ex"), "\"run\\226\\128\\169ex\"");
    }

    /// A zero-width space must survive the literal: dropping it both
    /// shortens the line and leaves the cursor past its end (#36).
    #[test]
    fn lua_quote_string_escapes_zero_width_space_as_utf8_decimal_bytes() {
        assert_eq!(lua_quote_string("ab\u{200B}c"), "\"ab\\226\\128\\139c\"");
    }

    #[test]
    fn lua_quote_string_escapes_rlo_and_bom_as_utf8_decimal_bytes() {
        assert_eq!(lua_quote_string("\u{202E}x"), "\"\\226\\128\\174x\"");
        assert_eq!(lua_quote_string("\u{FEFF}x"), "\"\\239\\187\\191x\"");
    }

    #[test]
    fn lua_quote_string_escapes_nul_as_three_digit_zero() {
        assert_eq!(lua_quote_string("a\0b"), "\"a\\000b\"");
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
    // The bash case-pattern injection tests were specific to the legacy
    // design where abbreviation keys were embedded as `case` arms inside the
    // bash bootstrap. With the new hook-based bootstrap, keys are never
    // emitted into shell code, so glob-like keys (`*`, `?`, `[...]`) pose no
    // shell-expansion risk at export time. The equivalent safety is now
    // enforced by Rust-side key validation in `config::validate_abbr_key`
    // (see runex-core/src/config.rs).

    // Zsh case-pattern injection tests removed for the same reason as bash:
    // the new hook-based bootstrap does not embed keys in shell code. See the
    // comment block above for the bash equivalent.

    // The "pwsh switch must have exactly one `default {` clause" regression
    // test was specific to the legacy token-embedding design; with the new
    // bootstrap there is no switch statement at all.

    } // mod case_pattern_globs
}
