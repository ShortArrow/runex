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

fn is_unicode_line_separator(c: char) -> bool {
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
    fn is_unsafe_for_display_detects_ascii_control() {
        assert!(is_unsafe_for_display('\x1B')); // ESC
        assert!(is_unsafe_for_display('\x07')); // BEL
        assert!(is_unsafe_for_display('\x7F')); // DEL
    }

    #[test]
    fn is_unsafe_for_display_detects_unicode_deception() {
        assert!(is_unsafe_for_display('\u{202E}')); // RLO
        assert!(is_unsafe_for_display('\u{FEFF}')); // BOM
        assert!(is_unsafe_for_display('\u{200B}')); // ZWSP
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
}
