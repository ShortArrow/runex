//! Per-keystroke hook logic, centralising what each shell template used to
//! re-implement in bash/zsh/pwsh/clink/nu. The goal is that shells provide a
//! thin "buffer + cursor in, eval-able string out" adapter and all the real
//! expansion logic — command-position detection, token extraction, cursor
//! placeholder handling, shell escaping — lives here in Rust.

use crate::expand::expand;
use crate::model::{Config, ExpandResult, Shell};
use crate::shell::{bash_quote_string, lua_quote_string, pwsh_quote_string};

/// Outcome of a hook call — what the shell adapter should do to its buffer.
///
/// `line` is the full new buffer text and `cursor` is the new cursor position
/// in **bytes** from the start of `line`. Shells are expected to replace their
/// buffer and cursor with these values atomically.
#[derive(Debug, Clone, PartialEq)]
pub enum HookAction {
    /// The token at the cursor expanded; shell should replace the buffer.
    Replace { line: String, cursor: usize },
    /// No expansion happened; shell should just insert a literal space at the
    /// cursor (the conventional trigger fallback).
    InsertSpace { line: String, cursor: usize },
}

/// Core hook entry point, shell-agnostic.
///
/// `line` is the shell's current buffer, `cursor` is the byte offset of the
/// caret. If a known abbreviation ends at the cursor and the prefix is a
/// command position, expand it; otherwise fall back to inserting a space.
pub fn hook<F>(
    config: &Config,
    shell: Shell,
    line: &str,
    cursor: usize,
    command_exists: F,
) -> HookAction
where
    F: Fn(&str) -> bool,
{
    let cursor = cursor.min(line.len());

    // If the cursor is not at a space boundary (mid-word), treat this like any
    // other printable key: insert a space at the cursor and don't try to
    // expand. This matches what each shell's template did historically.
    if cursor < line.len() && !line[cursor..].starts_with(' ') {
        return insert_space(line, cursor);
    }

    let left = &line[..cursor];
    let Some(token_start) = token_start_of(left) else {
        return insert_space(line, cursor);
    };
    let token = &left[token_start..];
    if token.is_empty() {
        return insert_space(line, cursor);
    }

    let prefix = &line[..token_start];
    if !is_command_position(prefix) {
        return insert_space(line, cursor);
    }

    if !is_known_token(config, token) {
        return insert_space(line, cursor);
    }

    match expand(config, token, shell, command_exists) {
        ExpandResult::Expanded { text, cursor_offset } => {
            let right = &line[cursor..];
            let mut new_line = String::with_capacity(prefix.len() + text.len() + right.len() + 1);
            new_line.push_str(prefix);
            new_line.push_str(&text);
            let cursor_after_expand = match cursor_offset {
                Some(off) => token_start + off,
                None => token_start + text.len(),
            };
            // Append the trailing space that the trigger key would have
            // produced, then everything that was to the right of the old cursor.
            new_line.insert(cursor_after_expand, ' ');
            new_line.push_str(right);
            HookAction::Replace {
                line: new_line,
                cursor: cursor_after_expand + 1,
            }
        }
        ExpandResult::PassThrough(_) => insert_space(line, cursor),
    }
}

fn insert_space(line: &str, cursor: usize) -> HookAction {
    let mut new_line = String::with_capacity(line.len() + 1);
    new_line.push_str(&line[..cursor]);
    new_line.push(' ');
    new_line.push_str(&line[cursor..]);
    HookAction::InsertSpace {
        line: new_line,
        cursor: cursor + 1,
    }
}

/// Returns the byte index where the last whitespace-delimited token starts
/// in `left` (i.e. the candidate abbreviation to expand). Returns `None` when
/// `left` is empty.
fn token_start_of(left: &str) -> Option<usize> {
    if left.is_empty() {
        return None;
    }
    Some(left.rfind(' ').map_or(0, |i| i + 1))
}

fn is_known_token(config: &Config, token: &str) -> bool {
    config.abbr.iter().any(|abbr| abbr.key == token)
}

/// Render a `HookAction` into a shell-specific eval-able string. The shell
/// integration script consumes this verbatim via `eval` (or equivalent).
pub fn render_action(shell: Shell, action: &HookAction) -> String {
    let (line, cursor) = match action {
        HookAction::Replace { line, cursor } | HookAction::InsertSpace { line, cursor } => {
            (line, cursor)
        }
    };
    match shell {
        Shell::Bash => format!(
            "READLINE_LINE={}; READLINE_POINT={}",
            bash_quote_string(line),
            cursor,
        ),
        Shell::Zsh => {
            let (lb, rb) = line.split_at(*cursor);
            format!("LBUFFER={}; RBUFFER={}", bash_quote_string(lb), bash_quote_string(rb))
        }
        Shell::Pwsh => format!(
            "$__RUNEX_LINE = {}\n$__RUNEX_CURSOR = {}",
            pwsh_quote_string(line),
            cursor,
        ),
        Shell::Clink => format!(
            "return {{ line = {}, cursor = {} }}",
            lua_quote_string(line),
            cursor,
        ),
        // nu has no safe `eval`; the bootstrap reads the emitted JSON object
        // with `from json` and applies the replace/set-cursor itself. Two
        // fields, machine-parsable, no string escaping inside nu.
        Shell::Nu => format!(
            "{{\"line\": {}, \"cursor\": {}}}",
            serde_json::to_string(line).unwrap_or_else(|_| "\"\"".into()),
            cursor,
        ),
    }
}

/// Returns `true` when the characters to the left of the token we're about to
/// consider represent a position where a fresh command name is expected.
///
/// Command position is:
/// - the very start of the line (after trimming trailing spaces), or
/// - immediately after a pipeline / list operator (`|`, `||`, `&&`, `;`), or
/// - immediately after `sudo ` that is itself in command position (e.g.
///   `sudo gcm` should still expand `gcm`).
///
/// Anything else (mid-arguments, after `=`, inside a command's arguments) is
/// treated as non-command-position and abbreviations should not expand.
pub fn is_command_position(prefix: &str) -> bool {
    let trimmed = trim_trailing_spaces(prefix);

    if trimmed.is_empty() {
        return true;
    }

    if ends_with_pipeline_operator(trimmed) {
        return true;
    }

    if let Some(before_sudo) = strip_trailing_sudo(trimmed) {
        let before_sudo = trim_trailing_spaces(before_sudo);
        if before_sudo.is_empty() {
            return true;
        }
        return ends_with_pipeline_operator(before_sudo);
    }

    false
}

fn trim_trailing_spaces(s: &str) -> &str {
    s.trim_end_matches(' ')
}

fn ends_with_pipeline_operator(s: &str) -> bool {
    // Order matters: check two-char operators before one-char `|`.
    s.ends_with("&&")
        || s.ends_with("||")
        || s.ends_with('|')
        || s.ends_with(';')
}

/// If `prefix` ends with the whitespace-separated word `sudo`, returns the
/// slice before it (including any trailing whitespace that preceded `sudo`).
/// Returns `None` otherwise.
fn strip_trailing_sudo(prefix: &str) -> Option<&str> {
    // Previous word is whatever follows the last space (or the whole prefix
    // when there is no space).
    let prev_word_start = prefix.rfind(' ').map_or(0, |i| i + 1);
    let prev_word = &prefix[prev_word_start..];
    if prev_word == "sudo" {
        Some(&prefix[..prev_word_start])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_position_empty_prefix_is_true() {
        assert!(is_command_position(""));
    }

    #[test]
    fn command_position_only_spaces_is_true() {
        assert!(is_command_position("    "));
    }

    #[test]
    fn command_position_after_pipe_is_true() {
        assert!(is_command_position("ls | "));
        assert!(is_command_position("ls |"));
    }

    #[test]
    fn command_position_after_logical_or_is_true() {
        assert!(is_command_position("foo || "));
    }

    #[test]
    fn command_position_after_logical_and_is_true() {
        assert!(is_command_position("foo && "));
    }

    #[test]
    fn command_position_after_semicolon_is_true() {
        assert!(is_command_position("foo; "));
    }

    #[test]
    fn command_position_after_sudo_at_start_is_true() {
        assert!(is_command_position("sudo "));
    }

    #[test]
    fn command_position_after_sudo_following_pipe_is_true() {
        assert!(is_command_position("ls | sudo "));
    }

    #[test]
    fn command_position_after_sudo_mid_args_is_false() {
        // `sudo` as an argument to another command is NOT command position.
        assert!(!is_command_position("echo sudo "));
    }

    #[test]
    fn command_position_middle_of_args_is_false() {
        assert!(!is_command_position("ls -la "));
        assert!(!is_command_position("git commit -m "));
    }

    #[test]
    fn command_position_not_fooled_by_substring_sudo() {
        // "pseudo" ends in "sudo" but isn't the word sudo.
        assert!(!is_command_position("pseudo "));
    }

    #[test]
    fn command_position_does_not_expand_after_assignment() {
        // `VAR=value cmd` – after `VAR=` we're not in command position here
        // (the whole `VAR=value` is a preceding assignment, but our simple
        // model intentionally treats this as non-command-position; abbrevs
        // shouldn't fire for the RHS of an assignment).
        assert!(!is_command_position("VAR="));
    }

    // ---- hook() behaviour ----

    use crate::config::parse_config;

    fn sample_config() -> Config {
        parse_config(
            r#"
            version = 1
            [[abbr]]
            key = "gcm"
            expand = "git commit -m"

            [[abbr]]
            key = "gca"
            expand = "git commit -am '{}'"

            [[abbr]]
            key = "ls"
            expand = "lsd"
            when_command_exists = ["lsd"]
            "#,
        )
        .unwrap()
    }

    fn always_exists(_: &str) -> bool { true }
    fn never_exists(_: &str) -> bool { false }

    #[test]
    fn hook_inserts_space_on_empty_line() {
        let config = sample_config();
        let action = hook(&config, Shell::Bash, "", 0, always_exists);
        assert_eq!(action, HookAction::InsertSpace { line: " ".into(), cursor: 1 });
    }

    #[test]
    fn hook_inserts_space_for_unknown_token() {
        let config = sample_config();
        let action = hook(&config, Shell::Bash, "nope", 4, always_exists);
        assert_eq!(action, HookAction::InsertSpace { line: "nope ".into(), cursor: 5 });
    }

    #[test]
    fn hook_inserts_space_when_not_in_command_position() {
        let config = sample_config();
        // `echo gcm` - gcm is an argument to echo, not a command name.
        let action = hook(&config, Shell::Bash, "echo gcm", 8, always_exists);
        assert_eq!(action, HookAction::InsertSpace { line: "echo gcm ".into(), cursor: 9 });
    }

    #[test]
    fn hook_expands_known_token_at_command_position() {
        let config = sample_config();
        let action = hook(&config, Shell::Bash, "gcm", 3, always_exists);
        // Expanded "git commit -m" (13 chars) then the triggering space.
        assert_eq!(
            action,
            HookAction::Replace { line: "git commit -m ".into(), cursor: 14 }
        );
    }

    #[test]
    fn hook_expands_token_after_sudo() {
        let config = sample_config();
        let action = hook(&config, Shell::Bash, "sudo gcm", 8, always_exists);
        assert_eq!(
            action,
            HookAction::Replace { line: "sudo git commit -m ".into(), cursor: 19 }
        );
    }

    #[test]
    fn hook_handles_cursor_placeholder() {
        let config = sample_config();
        // Rule: `expand = "git commit -am '{}'"`. After expansion the `{}` is
        // removed, cursor lands at that position, and the trigger space is
        // inserted **at the cursor** (so the user can keep typing inside the
        // quotes). That yields `git commit -am ' '` with cursor just after the
        // inserted space.
        let action = hook(&config, Shell::Bash, "gca", 3, always_exists);
        if let HookAction::Replace { line, cursor } = action {
            assert_eq!(line, "git commit -am ' '");
            assert_eq!(cursor, 17);
        } else {
            panic!("expected Replace, got {:?}", action);
        }
    }

    #[test]
    fn hook_respects_when_command_exists_failure() {
        let config = sample_config();
        // `ls` rule requires `lsd` to exist; with never_exists() it should skip.
        let action = hook(&config, Shell::Bash, "ls", 2, never_exists);
        assert_eq!(action, HookAction::InsertSpace { line: "ls ".into(), cursor: 3 });
    }

    #[test]
    fn hook_preserves_text_right_of_cursor() {
        let config = sample_config();
        // Buffer: "gcm xyz", cursor at 3 (right after "gcm").
        let action = hook(&config, Shell::Bash, "gcm xyz", 3, always_exists);
        if let HookAction::Replace { line, cursor } = action {
            assert_eq!(line, "git commit -m  xyz");
            assert_eq!(cursor, 14);
        } else {
            panic!("expected Replace, got {:?}", action);
        }
    }

    // ---- render_action() ----

    #[test]
    fn render_bash_quotes_single_quotes_in_expansion() {
        let action = HookAction::Replace {
            line: "git commit -am '' ".into(),
            cursor: 17,
        };
        let out = render_action(Shell::Bash, &action);
        // bash_quote_string wraps in single quotes and escapes embedded ones
        // with the `'\''` pattern.
        assert!(out.starts_with("READLINE_LINE="));
        assert!(out.contains("'\\''"), "render output should escape quotes: {}", out);
        assert!(out.ends_with("; READLINE_POINT=17"));
    }

    #[test]
    fn render_zsh_splits_lbuffer_rbuffer() {
        let action = HookAction::Replace {
            line: "git commit -m  xyz".into(),
            cursor: 14,
        };
        let out = render_action(Shell::Zsh, &action);
        assert!(out.contains("LBUFFER="));
        assert!(out.contains("RBUFFER="));
    }
}
