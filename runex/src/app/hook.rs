//! Use-case wrapper for the `hook` subcommand — the per-keystroke
//! dispatch that decides "insert a literal space", "expand the token
//! to the left of the cursor", or "no-op".
//!
//! Phase D D4a routes `cmd::hook` through this module so the cmd
//! layer doesn't import `crate::domain::hook` directly. See
//! `app::expand` for the rationale.
//!
//! The `HookAction` enum and the rendering function are re-exported
//! at `pub(crate)` so cmd code constructs the InsertSpace short-
//! circuit (oversized line, paste-pending, etc.) using a stable
//! re-export rather than a deep `crate::domain::hook::HookAction`
//! path.

use crate::domain::model::Config;
use crate::domain::shell::Shell;

pub(crate) use crate::domain::hook::HookAction;

/// Run the per-keystroke hook decision. The closure dispatches to
/// `domain::hook::hook` unchanged — the wrapper exists for layering,
/// not for behaviour.
pub(crate) fn run<F>(
    config: &Config,
    shell: Shell,
    line: &str,
    cursor: usize,
    command_exists: F,
) -> HookAction
where
    F: Fn(&str) -> bool,
{
    crate::domain::hook::hook(config, shell, line, cursor, command_exists)
}

/// Render the chosen [`HookAction`] as the shell-specific eval text
/// the wrapper script will pipe into the live shell.
///
/// The domain `HookAction` carries a byte-index cursor. Most shells
/// want the cursor back in their own unit (char count for bash/clink/
/// nu, UTF-16 code unit for pwsh). zsh is the exception — its render
/// `split_at`s the line by byte and emits substrings, so the cursor
/// number itself never reaches zsh and the conversion would be wrong.
///
/// We translate the byte cursor here at the use-case boundary so the
/// domain renderer keeps its byte-index invariant and the per-shell
/// `format!` arms stay trivial.
pub(crate) fn render(shell: Shell, action: &HookAction) -> String {
    let (line, byte_cursor) = match action {
        HookAction::Replace { line, cursor } | HookAction::InsertSpace { line, cursor } => {
            (line, *cursor)
        }
    };
    let shell_cursor = match shell {
        // zsh's renderer uses `split_at(cursor)` and emits LBUFFER /
        // RBUFFER substrings — the cursor number is never embedded
        // verbatim into the eval-text, so keep it as the byte index
        // split_at needs.
        Shell::Zsh => byte_cursor,
        Shell::Pwsh => byte_cursor_to_utf16(line, byte_cursor),
        Shell::Bash | Shell::Clink | Shell::Nu => byte_cursor_to_char(line, byte_cursor),
    };
    // Build a new action carrying the shell-native cursor so the
    // domain renderer's `format!` arms stay one-liners.
    let shell_action = match action {
        HookAction::Replace { line, .. } => HookAction::Replace {
            line: line.clone(),
            cursor: shell_cursor,
        },
        HookAction::InsertSpace { line, .. } => HookAction::InsertSpace {
            line: line.clone(),
            cursor: shell_cursor,
        },
    };
    crate::domain::hook::render_action(shell, &shell_action)
}

/// Convert the raw shell-provided cursor for the given shell into the
/// byte-index cursor the domain layer expects.
///
/// Single entry point for the shell-unit ↔ byte conversion table
/// (issue #6). Keeping the `match shell` here means every cmd-layer
/// short-circuit path (oversize line, paste-pending, config-load
/// failure, normal expansion) sees a consistent byte cursor without
/// re-implementing the conversion.
///
/// pwsh sends a UTF-16 code unit cursor (.NET / PSReadLine native
/// unit). Every other shell sends a Unicode-scalar-value (= Rust
/// `char`) cursor: bash's `READLINE_POINT`, zsh's `${#LBUFFER}`,
/// clink's `rl_buffer:getcursor() - 1`, and nu's `commandline
/// get-cursor` all count chars.
pub(crate) fn shell_cursor_to_byte(shell: Shell, line: &str, cursor: usize) -> usize {
    match shell {
        Shell::Pwsh => utf16_cursor_to_byte(line, cursor),
        Shell::Bash | Shell::Zsh | Shell::Clink | Shell::Nu => {
            char_cursor_to_byte(line, cursor)
        }
    }
}

/// Build a [`HookAction::InsertSpace`] that inserts a literal space
/// at `byte_cursor` and advances the cursor by 1 byte. Used by
/// `cmd::hook` short-circuit paths (oversize line, paste-pending,
/// config-load failure) so the three branches don't re-implement
/// the same five-line slice-and-concat.
///
/// `byte_cursor` is assumed to already be on a UTF-8 char boundary
/// — callers must convert from shell-native units via
/// [`shell_cursor_to_byte`] first.
pub(crate) fn insert_space_action(line: &str, byte_cursor: usize) -> HookAction {
    let cursor = byte_cursor.min(line.len());
    let mut s = String::with_capacity(line.len() + 1);
    s.push_str(&line[..cursor]);
    s.push(' ');
    s.push_str(&line[cursor..]);
    HookAction::InsertSpace {
        line: s,
        cursor: cursor + 1,
    }
}

/// Convert a shell-provided character-index cursor into the byte-index
/// cursor the domain layer expects.
///
/// bash / zsh / nu / clink all pass `--cursor` as a count of Unicode
/// scalar values (= Rust `char`) from the start of `--line`.
/// `domain::hook::hook` documents `cursor` as a byte offset; for
/// ASCII-only input the two coincide, which is why ASCII tests passed
/// before this conversion landed. For multi-byte input (Japanese,
/// emoji, combining marks) they diverge and the domain layer would
/// slice mid-character — that's issue #6.
///
/// The conversion is `O(char_cursor)` and runs once per `runex hook`
/// invocation (= once per trigger keypress). For a 16 KiB `line` cap
/// (`MAX_HOOK_LINE_BYTES`) the worst case is ~4 K char_indices steps,
/// well under any keystroke-latency budget.
///
/// Clamps to `line.len()` when `char_cursor` exceeds the character
/// count of `line`. Clamping (rather than panicking) is the defensive
/// response to an out-of-range cursor: the shell could in principle
/// send any usize and the hook must keep working — this matches the
/// existing `cursor.min(line.len())` clamp inside `domain::hook::hook`.
///
/// Uses `char_indices()` rather than the `unicode-segmentation` crate
/// because shells count Unicode scalar values, not grapheme clusters.
/// Adding a crate dependency just for `nth()` would only obscure that.
pub(crate) fn char_cursor_to_byte(line: &str, char_cursor: usize) -> usize {
    line.char_indices()
        .nth(char_cursor)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(line.len())
}

/// Inverse of [`char_cursor_to_byte`]: byte index → character index.
///
/// Used at the **output** side of `cmd::hook` to translate the byte
/// cursor the domain layer produces back into the character cursor
/// bash / zsh / nu / clink expect on the return path
/// (`READLINE_POINT=` etc.). pwsh uses [`byte_cursor_to_utf16`]
/// instead — its `SetCursorPosition` takes UTF-16 code units.
///
/// If `byte_cursor` lands inside a multi-byte character (which the
/// conversion at the entry side shouldn't produce, but we stay
/// defensive), the result is the character index of the most recent
/// boundary at or before `byte_cursor`. Clamps to the line's
/// character count when `byte_cursor >= line.len()`.
pub(crate) fn byte_cursor_to_char(line: &str, byte_cursor: usize) -> usize {
    if byte_cursor >= line.len() {
        return line.chars().count();
    }
    // Find the character whose byte range contains `byte_cursor`. If
    // the cursor sits exactly on a char boundary that char's index
    // is the answer. If it sits inside a multi-byte char (= not on
    // a boundary) we round down to that char's *start* index.
    line.char_indices()
        .enumerate()
        .find_map(|(char_idx, (byte_idx, ch))| {
            let next_byte = byte_idx + ch.len_utf8();
            if byte_cursor < next_byte {
                Some(char_idx)
            } else {
                None
            }
        })
        .unwrap_or_else(|| line.chars().count())
}

/// Convert a UTF-16 code-unit cursor (as PowerShell / PSReadLine
/// hands us) into the byte-index cursor the domain layer expects.
///
/// .NET strings — including the buffer that
/// `PSConsoleReadLine.GetBufferState` exposes — are UTF-16. The
/// `$cursor` ref it writes back is therefore a UTF-16 code-unit
/// offset, NOT a `char` (= Unicode scalar value) count and NOT a
/// byte offset. They differ for any character outside the BMP
/// (= 4-byte UTF-8 / surrogate-pair UTF-16): an emoji like 🎯
/// (U+1F3AF) is 1 char / 4 bytes UTF-8 / 2 UTF-16 code units.
///
/// If the cursor lands in the middle of a surrogate pair (= invalid
/// in well-formed UTF-16, but defensible against a misbehaving
/// caller), we round down to the previous valid boundary so the
/// downstream byte slice stays UTF-8 valid.
pub(crate) fn utf16_cursor_to_byte(line: &str, utf16_cursor: usize) -> usize {
    let mut consumed_utf16 = 0usize;
    for (byte_idx, ch) in line.char_indices() {
        if consumed_utf16 == utf16_cursor {
            return byte_idx;
        }
        let next_utf16 = consumed_utf16 + ch.len_utf16();
        if next_utf16 > utf16_cursor {
            // Cursor lands inside this character (= the only way this
            // can happen is if it's mid surrogate-pair). Round down
            // to the start byte of the character so the downstream
            // slice stays UTF-8 valid.
            return byte_idx;
        }
        consumed_utf16 = next_utf16;
    }
    line.len()
}

/// Inverse of [`utf16_cursor_to_byte`]: byte index → UTF-16 code-unit
/// count. Used at the **output** side of `cmd::hook` to format the
/// pwsh return value (`$__RUNEX_CURSOR`), which
/// `SetCursorPosition` interprets as a UTF-16 code-unit offset.
///
/// Sums `len_utf16()` of every char whose byte range ends at or
/// before `byte_cursor`. If `byte_cursor` lands mid-character (which
/// the entry-side conversion should never produce, but the helper
/// stays defensive), we count up to and including that partial char
/// so the cursor doesn't slip backwards.
pub(crate) fn byte_cursor_to_utf16(line: &str, byte_cursor: usize) -> usize {
    if byte_cursor >= line.len() {
        return line.chars().map(|c| c.len_utf16()).sum();
    }
    let mut utf16 = 0usize;
    for (byte_idx, ch) in line.char_indices() {
        if byte_idx >= byte_cursor {
            return utf16;
        }
        utf16 += ch.len_utf16();
    }
    utf16
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── char_cursor_to_byte ────────────────────────────────────────────

    #[test]
    fn char_cursor_to_byte_ascii_is_identity() {
        // ASCII では char == byte なので恒等。既存 ASCII テストとの
        // 互換が崩れないことを示す regression pin。
        assert_eq!(char_cursor_to_byte("lsd ./test1", 0), 0);
        assert_eq!(char_cursor_to_byte("lsd ./test1", 4), 4);
        assert_eq!(char_cursor_to_byte("lsd ./test1", 11), 11);
    }

    #[test]
    fn char_cursor_to_byte_japanese_three_byte_chars() {
        // 「お」「は」「よ」「う」 (= U+304A, U+306F, U+3088, U+3046) は
        // 各 3 byte UTF-8。char index 6 (= 「お」の直前) = byte 6 (ASCII 部),
        // char index 7 (= 「は」の直前) = byte 9, etc.
        let line = "lsd ./おはよう  ./test1";
        assert_eq!(char_cursor_to_byte(line, 6), 6, "before お");
        assert_eq!(char_cursor_to_byte(line, 7), 9, "before は");
        assert_eq!(char_cursor_to_byte(line, 8), 12, "before よ");
        assert_eq!(char_cursor_to_byte(line, 9), 15, "before う");
        assert_eq!(char_cursor_to_byte(line, 10), 18, "after う / before first space");
    }

    #[test]
    fn char_cursor_to_byte_at_line_end_returns_line_len() {
        // 6 ASCII + 4 chars (3 byte each) = 10 chars / 18 bytes.
        let line = "lsd ./おはよう";
        assert_eq!(char_cursor_to_byte(line, 10), line.len());
    }

    #[test]
    fn char_cursor_to_byte_past_line_end_clamps_to_line_len() {
        // Defensive clamp: a misbehaving shell can't make us panic.
        let line = "abc";
        assert_eq!(char_cursor_to_byte(line, 10), 3);
        assert_eq!(char_cursor_to_byte(line, usize::MAX), 3);
    }

    #[test]
    fn char_cursor_to_byte_empty_line() {
        assert_eq!(char_cursor_to_byte("", 0), 0);
        assert_eq!(char_cursor_to_byte("", 5), 0);
    }

    #[test]
    fn char_cursor_to_byte_emoji_four_byte_chars() {
        // 🎯 (U+1F3AF) is 4 bytes in UTF-8 / 1 Rust char / 2 UTF-16
        // code units (surrogate pair). The UTF-16 side is the
        // separate `utf16_cursor_to_byte` helper's concern — this
        // test only pins the char-count side.
        let line = "gst🎯end";
        assert_eq!(char_cursor_to_byte(line, 0), 0);
        assert_eq!(char_cursor_to_byte(line, 3), 3, "before 🎯");
        assert_eq!(char_cursor_to_byte(line, 4), 7, "after 🎯 (1 char, 4 byte)");
        assert_eq!(char_cursor_to_byte(line, 5), 8, "before n in 'end'");
    }

    #[test]
    fn char_cursor_to_byte_combining_marks_are_separate_chars() {
        // U+0061 (a) + U+0301 (combining acute) renders as "á" but is
        // 2 chars / 3 bytes. Rust char = code point, NOT grapheme
        // cluster — same convention bash and zsh use.
        let line = "a\u{0301}b";
        assert_eq!(char_cursor_to_byte(line, 0), 0);
        assert_eq!(char_cursor_to_byte(line, 1), 1, "between a and combining");
        assert_eq!(char_cursor_to_byte(line, 2), 3, "between combining and b");
        assert_eq!(char_cursor_to_byte(line, 3), 4);
    }

    #[test]
    fn char_cursor_to_byte_rtl_chars_are_normal_codepoints() {
        // U+202E (RLO) is 3 bytes / 1 char. RTL display reordering is
        // the terminal's concern; Rust handles it as a code point.
        let line = "a\u{202E}b";
        assert_eq!(char_cursor_to_byte(line, 0), 0);
        assert_eq!(char_cursor_to_byte(line, 1), 1, "before RLO");
        assert_eq!(char_cursor_to_byte(line, 2), 4, "after RLO (3 byte)");
        assert_eq!(char_cursor_to_byte(line, 3), 5);
    }

    #[test]
    fn char_cursor_to_byte_handles_nul_byte_without_panic() {
        // &str is valid UTF-8 by invariant so NUL (0x00) is just a
        // 1-byte / 1-char codepoint. Real config validation rejects
        // NULs elsewhere but the helper must not panic.
        let line = "a\0b";
        assert_eq!(char_cursor_to_byte(line, 0), 0);
        assert_eq!(char_cursor_to_byte(line, 1), 1);
        assert_eq!(char_cursor_to_byte(line, 2), 2);
        assert_eq!(char_cursor_to_byte(line, 3), 3);
    }

    // ── byte_cursor_to_char ────────────────────────────────────────────

    #[test]
    fn byte_cursor_to_char_ascii_is_identity() {
        assert_eq!(byte_cursor_to_char("lsd ./test1", 0), 0);
        assert_eq!(byte_cursor_to_char("lsd ./test1", 4), 4);
        assert_eq!(byte_cursor_to_char("lsd ./test1", 11), 11);
    }

    #[test]
    fn byte_cursor_to_char_japanese_three_byte_chars() {
        // Inverse of the char_cursor_to_byte test above.
        let line = "lsd ./おはよう  ./test1";
        assert_eq!(byte_cursor_to_char(line, 6), 6, "before お");
        assert_eq!(byte_cursor_to_char(line, 9), 7, "before は");
        assert_eq!(byte_cursor_to_char(line, 12), 8, "before よ");
        assert_eq!(byte_cursor_to_char(line, 15), 9, "before う");
        assert_eq!(byte_cursor_to_char(line, 18), 10, "after う / before first space");
    }

    #[test]
    fn byte_cursor_to_char_at_line_end_returns_char_count() {
        let line = "lsd ./おはよう";
        assert_eq!(byte_cursor_to_char(line, line.len()), 10);
    }

    #[test]
    fn byte_cursor_to_char_past_line_end_clamps_to_char_count() {
        // Defensive: must not panic for a too-large byte cursor.
        let line = "abc";
        assert_eq!(byte_cursor_to_char(line, 10), 3);
        assert_eq!(byte_cursor_to_char(line, usize::MAX), 3);
    }

    #[test]
    fn byte_cursor_to_char_empty_line() {
        assert_eq!(byte_cursor_to_char("", 0), 0);
        assert_eq!(byte_cursor_to_char("", 5), 0);
    }

    #[test]
    fn byte_cursor_to_char_emoji_four_byte_chars() {
        let line = "gst🎯end";
        assert_eq!(byte_cursor_to_char(line, 0), 0);
        assert_eq!(byte_cursor_to_char(line, 3), 3, "before 🎯");
        assert_eq!(byte_cursor_to_char(line, 7), 4, "after 🎯");
        assert_eq!(byte_cursor_to_char(line, 8), 5);
    }

    #[test]
    fn byte_cursor_to_char_round_trips_with_char_cursor_to_byte() {
        // Round-trip: char→byte→char must be identity on every valid
        // char index. This pins the two helpers as inverses, which is
        // the property the cmd::hook entry conversion and the render
        // exit conversion rely on for symmetry.
        let lines = [
            "lsd ./おはよう  ./test1",
            "gst🎯end",
            "a\u{0301}b",
            "abc",
            "",
        ];
        for line in lines {
            let char_count = line.chars().count();
            for ch in 0..=char_count {
                let byte = char_cursor_to_byte(line, ch);
                let back = byte_cursor_to_char(line, byte);
                assert_eq!(
                    back, ch,
                    "round-trip failed for line={line:?} char_cursor={ch}: byte={byte} back={back}"
                );
            }
        }
    }

    #[test]
    fn byte_cursor_to_char_combining_marks_are_separate_chars() {
        let line = "a\u{0301}b";
        assert_eq!(byte_cursor_to_char(line, 0), 0);
        assert_eq!(byte_cursor_to_char(line, 1), 1, "between a and combining");
        assert_eq!(byte_cursor_to_char(line, 3), 2, "between combining and b");
        assert_eq!(byte_cursor_to_char(line, 4), 3);
    }

    #[test]
    fn byte_cursor_to_char_non_char_boundary_rounds_down() {
        // If a byte cursor lands mid-character (which shouldn't
        // happen in practice — the conversion is the inverse of a
        // char-aligned slice — but the helper still has to be safe),
        // we round down to the previous char boundary.
        let line = "lsd ./おはよう";
        // Byte 7 sits inside「お」(bytes 6-8). The previous char
        // boundary is byte 6 = char 6 (the start of「お」).
        assert_eq!(byte_cursor_to_char(line, 7), 6);
        assert_eq!(byte_cursor_to_char(line, 8), 6);
    }

    // ── utf16_cursor_to_byte / byte_cursor_to_utf16 (pwsh) ────────────

    #[test]
    fn utf16_cursor_to_byte_ascii_is_identity() {
        // ASCII chars are 1 UTF-16 code unit and 1 byte each.
        assert_eq!(utf16_cursor_to_byte("hello", 0), 0);
        assert_eq!(utf16_cursor_to_byte("hello", 3), 3);
        assert_eq!(utf16_cursor_to_byte("hello", 5), 5);
    }

    #[test]
    fn utf16_cursor_to_byte_bmp_chars_one_code_unit_each() {
        // Japanese chars in the BMP (U+0000..U+FFFF) are 1 UTF-16
        // code unit each but 3 bytes in UTF-8.
        let line = "lsd ./おはよう";
        assert_eq!(utf16_cursor_to_byte(line, 6), 6, "before お");
        assert_eq!(utf16_cursor_to_byte(line, 7), 9, "before は (1 UTF-16 unit, 3 bytes)");
        assert_eq!(utf16_cursor_to_byte(line, 8), 12, "before よ");
        assert_eq!(utf16_cursor_to_byte(line, 10), 18, "after う");
    }

    #[test]
    fn utf16_cursor_to_byte_handles_surrogate_pair_for_emoji() {
        // 🎯 (U+1F3AF) is outside the BMP → 2 UTF-16 code units
        // (= surrogate pair D83C DFAF), 4 bytes UTF-8. PowerShell
        // counts the cursor by UTF-16 code unit, so cursor=1 lands
        // *inside* the surrogate pair (= invalid); cursor=2 is right
        // after the emoji.
        let line = "🎯end";
        assert_eq!(utf16_cursor_to_byte(line, 0), 0, "before 🎯");
        assert_eq!(utf16_cursor_to_byte(line, 2), 4, "after 🎯 (2 UTF-16 units, 4 bytes)");
        assert_eq!(utf16_cursor_to_byte(line, 3), 5, "after e");
        assert_eq!(utf16_cursor_to_byte(line, 5), 7);
    }

    #[test]
    fn utf16_cursor_to_byte_mid_surrogate_rounds_down() {
        // cursor=1 in "🎯..." sits inside the surrogate pair, which
        // is invalid in well-formed UTF-16. Round down to byte 0
        // (the start of the emoji) so the next slice doesn't split
        // a UTF-8 char.
        let line = "🎯end";
        assert_eq!(utf16_cursor_to_byte(line, 1), 0);
    }

    #[test]
    fn utf16_cursor_to_byte_past_line_end_clamps() {
        let line = "abc";
        assert_eq!(utf16_cursor_to_byte(line, 10), 3);
        assert_eq!(utf16_cursor_to_byte(line, usize::MAX), 3);
    }

    #[test]
    fn utf16_cursor_to_byte_empty_line() {
        assert_eq!(utf16_cursor_to_byte("", 0), 0);
        assert_eq!(utf16_cursor_to_byte("", 5), 0);
    }

    #[test]
    fn byte_cursor_to_utf16_ascii_is_identity() {
        assert_eq!(byte_cursor_to_utf16("hello", 0), 0);
        assert_eq!(byte_cursor_to_utf16("hello", 3), 3);
        assert_eq!(byte_cursor_to_utf16("hello", 5), 5);
    }

    #[test]
    fn byte_cursor_to_utf16_japanese_three_byte_chars() {
        // 3 bytes UTF-8 = 1 UTF-16 code unit.
        let line = "lsd ./おはよう";
        assert_eq!(byte_cursor_to_utf16(line, 6), 6, "before お");
        assert_eq!(byte_cursor_to_utf16(line, 9), 7, "before は");
        assert_eq!(byte_cursor_to_utf16(line, 12), 8, "before よ");
        assert_eq!(byte_cursor_to_utf16(line, 18), 10, "after う");
    }

    #[test]
    fn byte_cursor_to_utf16_emoji_surrogate_pair() {
        // 4 bytes UTF-8 = 2 UTF-16 code units. byte 4 = after 🎯 =
        // UTF-16 unit 2.
        let line = "🎯end";
        assert_eq!(byte_cursor_to_utf16(line, 0), 0, "before 🎯");
        assert_eq!(byte_cursor_to_utf16(line, 4), 2, "after 🎯");
        assert_eq!(byte_cursor_to_utf16(line, 5), 3, "after e");
        assert_eq!(byte_cursor_to_utf16(line, 7), 5);
    }

    #[test]
    fn byte_cursor_to_utf16_round_trips_with_utf16_cursor_to_byte() {
        // Round-trip on char-aligned UTF-16 cursor positions.
        let lines = [
            "🎯end",
            "lsd ./おはよう",
            "abc",
            "",
        ];
        for line in lines {
            let utf16_len: usize = line.chars().map(|c| c.len_utf16()).sum();
            // Only test cursor positions that sit on a UTF-16 code
            // point boundary (= NOT in the middle of a surrogate
            // pair). Iterate per char and accumulate the UTF-16
            // length so we stay on boundaries.
            let mut utf16_pos = 0usize;
            for ch in line.chars() {
                let byte = utf16_cursor_to_byte(line, utf16_pos);
                let back = byte_cursor_to_utf16(line, byte);
                assert_eq!(
                    back, utf16_pos,
                    "round-trip failed for line={line:?} utf16_pos={utf16_pos}"
                );
                utf16_pos += ch.len_utf16();
            }
            // End-of-line position too.
            assert_eq!(
                byte_cursor_to_utf16(line, line.len()),
                utf16_len,
                "end-of-line round-trip failed for {line:?}"
            );
        }
    }
}
