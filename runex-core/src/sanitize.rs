fn is_soft_hyphen(c: char) -> bool {
    c == '\u{00AD}'
}

fn is_combining_grapheme_joiner(c: char) -> bool {
    c == '\u{034F}'
}

fn is_arabic_letter_mark(c: char) -> bool {
    c == '\u{061C}'
}

fn is_hangul_filler(c: char) -> bool {
    matches!(c, '\u{115F}'..='\u{1160}' | '\u{3164}' | '\u{FFA0}')
}

fn is_khmer_invisible_vowel(c: char) -> bool {
    matches!(c, '\u{17B4}'..='\u{17B5}')
}

fn is_mongolian_free_variation_selector(c: char) -> bool {
    matches!(c, '\u{180B}'..='\u{180D}' | '\u{180F}')
}

fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}')
}

fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}')
}

fn is_invisible_operator(c: char) -> bool {
    matches!(c, '\u{2060}'..='\u{206F}')
}

fn is_variation_selector(c: char) -> bool {
    matches!(c, '\u{FE00}'..='\u{FE0F}')
}

fn is_bom(c: char) -> bool {
    c == '\u{FEFF}'
}

fn is_interlinear_annotation(c: char) -> bool {
    matches!(c, '\u{FFF9}'..='\u{FFFB}')
}

fn is_tag(c: char) -> bool {
    matches!(c, '\u{E0000}'..='\u{E007F}')
}

/// Returns true if `c` is a Unicode line or paragraph separator.
///
/// Covers NEL (U+0085), Line Separator (U+2028), and Paragraph Separator (U+2029).
/// These behave like newlines in some runtimes and must be dropped from shell
/// string literals where they cannot be safely escaped.
pub fn is_unicode_line_separator(c: char) -> bool {
    matches!(c, '\u{0085}' | '\u{2028}'..='\u{2029}')
}

/// Returns true if `c` is a Unicode visual-deception character.
///
/// These characters are invisible or visually ambiguous in terminal output and
/// can be used to mislead users about the content of a string (e.g., RLO for
/// right-to-left override, BOM, zero-width spaces).
pub fn is_deceptive_unicode(c: char) -> bool {
    is_soft_hyphen(c)
        || is_combining_grapheme_joiner(c)
        || is_arabic_letter_mark(c)
        || is_hangul_filler(c)
        || is_khmer_invisible_vowel(c)
        || is_mongolian_free_variation_selector(c)
        || is_zero_width(c)
        || is_bidi_control(c)
        || is_invisible_operator(c)
        || is_variation_selector(c)
        || is_bom(c)
        || is_interlinear_annotation(c)
        || is_tag(c)
}

/// Returns true if `c` should be removed before printing to a terminal.
///
/// This is a superset of [`is_deceptive_unicode`]: it also covers ASCII control
/// characters (which can move the cursor, clear the screen, etc.) and Unicode
/// line/paragraph separators that behave like newlines.
pub fn is_unsafe_for_display(c: char) -> bool {
    c.is_ascii_control()
        || is_unicode_line_separator(c)
        || is_deceptive_unicode(c)
}

/// Strip characters unsafe for terminal display from a string.
///
/// Removes all characters for which [`is_unsafe_for_display`] returns `true`.
pub fn sanitize_for_display(s: &str) -> String {
    s.chars().filter(|&c| !is_unsafe_for_display(c)).collect()
}

/// Strip characters unsafe for terminal display, preserving newlines and tabs.
///
/// Like [`sanitize_for_display`] but allows `\n`, `\r`, and `\t` so that
/// multi-line messages (e.g. TOML parse errors) remain readable.
pub fn sanitize_multiline_for_display(s: &str) -> String {
    s.chars()
        .filter(|&c| c == '\n' || c == '\r' || c == '\t' || !is_unsafe_for_display(c))
        .collect()
}

/// Map a character to its double-quoted string escape sequence.
///
/// Returns `Some(escaped)` for the five characters that need escaping inside a
/// double-quoted string literal (`\`, `"`, `\n`, `\r`, `\t`), and `None` for
/// everything else.  Used by Nu and Lua quote functions to avoid repeating the
/// same five `match` arms.
pub fn double_quote_escape(c: char) -> Option<&'static str> {
    match c {
        '\\' => Some("\\\\"),
        '"' => Some("\\\""),
        '\n' => Some("\\n"),
        '\r' => Some("\\r"),
        '\t' => Some("\\t"),
        _ => None,
    }
}

/// Returns true if `c` should be silently dropped when building a Nu double-quoted
/// string (`"..."` or `^"..."`).
///
/// Nu string escaping handles `\n`, `\r`, and `\t` as explicit two-character
/// sequences, so those three are excluded here.  Everything else that is unsafe
/// for terminal display is dropped rather than escaped.
pub fn is_nu_drop_char(c: char) -> bool {
    !matches!(c, '\n' | '\r' | '\t') && is_unsafe_for_display(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_deceptive_unicode_detects_rlo() {
        assert!(is_deceptive_unicode('\u{202E}'));
    }

    #[test]
    fn is_deceptive_unicode_detects_bom() {
        assert!(is_deceptive_unicode('\u{FEFF}'));
    }

    #[test]
    fn is_deceptive_unicode_detects_zwsp() {
        assert!(is_deceptive_unicode('\u{200B}'));
    }

    #[test]
    fn is_deceptive_unicode_allows_normal_chars() {
        assert!(!is_deceptive_unicode('a'));
        assert!(!is_deceptive_unicode(' '));
        assert!(!is_deceptive_unicode('é'));
    }

    #[test]
    fn is_unsafe_for_display_detects_esc() {
        assert!(is_unsafe_for_display('\x1B'));
    }

    #[test]
    fn is_unsafe_for_display_detects_bel() {
        assert!(is_unsafe_for_display('\x07'));
    }

    #[test]
    fn is_unsafe_for_display_detects_del() {
        assert!(is_unsafe_for_display('\x7F'));
    }

    #[test]
    fn is_unsafe_for_display_detects_rlo() {
        assert!(is_unsafe_for_display('\u{202E}'));
    }

    #[test]
    fn is_unsafe_for_display_detects_bom() {
        assert!(is_unsafe_for_display('\u{FEFF}'));
    }

    #[test]
    fn is_unsafe_for_display_detects_zwsp() {
        assert!(is_unsafe_for_display('\u{200B}'));
    }

    #[test]
    fn is_unsafe_for_display_allows_normal_chars() {
        assert!(!is_unsafe_for_display('a'));
        assert!(!is_unsafe_for_display(' '));
        assert!(!is_unsafe_for_display('é'));
    }

    #[test]
    fn sanitize_for_display_strips_control_chars() {
        assert_eq!(sanitize_for_display("he\x1Bllo"), "hello");
    }

    #[test]
    fn sanitize_for_display_strips_rlo() {
        assert_eq!(sanitize_for_display("he\u{202E}llo"), "hello");
    }

    #[test]
    fn sanitize_for_display_preserves_normal_text() {
        assert_eq!(sanitize_for_display("hello world"), "hello world");
    }

    #[test]
    fn is_nu_drop_char_drops_all_ascii_control() {
        for b in 0u8..=0x1F {
            let c = b as char;
            if matches!(c, '\n' | '\r' | '\t') {
                assert!(!is_nu_drop_char(c), "U+{:04X} must not be dropped (it is escaped)", b);
            } else {
                assert!(is_nu_drop_char(c), "U+{:04X} must be dropped", b);
            }
        }
        assert!(is_nu_drop_char('\x7F'), "DEL must be dropped");
    }

    #[test]
    fn is_nu_drop_char_drops_nel() {
        assert!(is_nu_drop_char('\u{0085}'));
    }

    #[test]
    fn is_nu_drop_char_drops_line_separator() {
        assert!(is_nu_drop_char('\u{2028}'));
    }

    #[test]
    fn is_nu_drop_char_drops_paragraph_separator() {
        assert!(is_nu_drop_char('\u{2029}'));
    }

    #[test]
    fn is_nu_drop_char_drops_rlo() {
        assert!(is_nu_drop_char('\u{202E}'));
    }

    #[test]
    fn is_nu_drop_char_drops_bom() {
        assert!(is_nu_drop_char('\u{FEFF}'));
    }

    #[test]
    fn is_nu_drop_char_drops_zwsp() {
        assert!(is_nu_drop_char('\u{200B}'));
    }

    #[test]
    fn is_nu_drop_char_preserves_newline() {
        assert!(!is_nu_drop_char('\n'));
    }

    #[test]
    fn is_nu_drop_char_preserves_carriage_return() {
        assert!(!is_nu_drop_char('\r'));
    }

    #[test]
    fn is_nu_drop_char_preserves_tab() {
        assert!(!is_nu_drop_char('\t'));
    }

    #[test]
    fn double_quote_escape_escapes_backslash() {
        assert!(double_quote_escape('\\').is_some());
    }

    #[test]
    fn double_quote_escape_escapes_double_quote() {
        assert!(double_quote_escape('"').is_some());
    }

    #[test]
    fn double_quote_escape_escapes_newline() {
        assert!(double_quote_escape('\n').is_some());
    }

    #[test]
    fn double_quote_escape_escapes_carriage_return() {
        assert!(double_quote_escape('\r').is_some());
    }

    #[test]
    fn double_quote_escape_escapes_tab() {
        assert!(double_quote_escape('\t').is_some());
    }

    #[test]
    fn double_quote_escape_ignores_letter() {
        assert!(double_quote_escape('a').is_none());
    }

    #[test]
    fn double_quote_escape_ignores_dollar() {
        assert!(double_quote_escape('$').is_none());
    }

    #[test]
    fn double_quote_escape_ignores_nul() {
        assert!(double_quote_escape('\0').is_none());
    }
}
