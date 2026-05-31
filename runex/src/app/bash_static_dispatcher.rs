//! Bake-mode bash dispatcher generator for the cygwin/msys (Git Bash)
//! workaround of issue #7.
//!
//! ## Background
//!
//! In Git Bash the `bind -x` handler is invoked under the cygwin readline
//! backend. PoC reproductions on Windows 11 + Git Bash 2.50 show that
//! spawning a Win32 .exe (regardless of cursor placement or whether the
//! subprocess output is consumed via `$()` or a temp file) from inside
//! the handler causes the *next* SIGINT to be lost — the user's Ctrl+C
//! after a fresh expansion no longer clears the line buffer, and the
//! next Enter therefore runs the stale expanded command.
//!
//! ## Strategy
//!
//! Avoid spawning `runex.exe` from the trigger handler altogether on
//! Git Bash. The cache file embeds the abbreviation table as a bash
//! associative array and re-implements the lookup/render in pure bash.
//! A runtime `case "${OSTYPE-}"` switch inside the same cache file
//! routes Git Bash to this bake-mode dispatcher and Linux/WSL bash to
//! the existing `runex hook` exec path; one cache file serves every
//! bash flavour the user might run with the same dotfiles.
//!
//! ## Trade-off (documented in `docs/setup.{md,ja.md}`)
//!
//! Command-position detection (e.g. `echo gst` *not* expanding `gst`) is
//! handled in Rust by `domain::hook::is_command_position`. Re-implementing
//! that state machine in pure bash would more than double the size of
//! this module and add maintenance burden, so the bake path treats every
//! trailing token as if it were in command position. This is a known and
//! intentional regression vs. the exec path.

use crate::domain::expand::NUMBER_PLACEHOLDER;
use crate::domain::model::{Config, Shell};

#[cfg(test)]
mod tests {
    use super::*;

    // ── bash_double_quote_for_assoc ────────────────────────────────────

    #[test]
    fn bash_double_quote_for_assoc_wraps_plain_ascii() {
        assert_eq!(bash_double_quote_for_assoc("gcm"), "\"gcm\"");
    }

    #[test]
    fn bash_double_quote_for_assoc_escapes_double_quote() {
        assert_eq!(bash_double_quote_for_assoc("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn bash_double_quote_for_assoc_escapes_backslash() {
        assert_eq!(bash_double_quote_for_assoc("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn bash_double_quote_for_assoc_escapes_dollar() {
        // Inside a bash double-quoted string `$HOME` would normally expand.
        // Escape so the literal bytes survive into READLINE_LINE.
        assert_eq!(bash_double_quote_for_assoc("$HOME"), "\"\\$HOME\"");
    }

    #[test]
    fn bash_double_quote_for_assoc_escapes_backtick() {
        // Backtick command substitution would otherwise execute a
        // subprocess at every `source` — defeats the whole point of
        // the bake path.
        assert_eq!(bash_double_quote_for_assoc("`whoami`"), "\"\\`whoami\\`\"");
    }

    #[test]
    fn bash_double_quote_for_assoc_drops_ascii_control_chars() {
        // Config validator already rejects control chars in user-facing
        // fields, but the helper still drops them defensively so a
        // future caller that bypasses validation can't inject newlines
        // into the cache file.
        let s = bash_double_quote_for_assoc("a\nb\tc\x01d");
        assert_eq!(s, "\"abcd\"");
    }

    #[test]
    fn bash_double_quote_for_assoc_drops_deceptive_unicode() {
        // RLO (U+202E) and BOM (U+FEFF) — same policy as bash_quote_string.
        let s = bash_double_quote_for_assoc("a\u{202E}b\u{FEFF}c");
        assert_eq!(s, "\"abc\"");
    }

    #[test]
    fn bash_double_quote_for_assoc_preserves_single_quote() {
        // Single quotes inside double-quoted strings are literal in bash —
        // no escape needed. This is important: the `{}` placeholder is
        // commonly used inside `'...'` (e.g. `git commit -am '{}'`),
        // and the value must round-trip byte-for-byte.
        assert_eq!(bash_double_quote_for_assoc("a'b"), "\"a'b\"");
    }

    // ── exact_table_lines ──────────────────────────────────────────────

    use crate::domain::model::{Abbr, KeybindConfig, PerShellString, PrecacheConfig};

    fn cfg(abbr: Vec<Abbr>) -> Config {
        Config {
            version: 1,
            keybind: KeybindConfig::default(),
            precache: PrecacheConfig::default(),
            abbr,
        }
    }

    fn plain_abbr(key: &str, expand: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(expand.into()),
            when_command_exists: None,
            number: None,
        }
    }

    #[test]
    fn exact_table_lines_emits_one_entry_per_plain_abbr() {
        let c = cfg(vec![
            plain_abbr("gst", "git status"),
            plain_abbr("gcm", "git commit -m"),
        ]);
        let s = exact_table_lines(&c);
        assert!(s.contains("[\"gst\"]=\"git status\""), "got: {s}");
        assert!(s.contains("[\"gcm\"]=\"git commit -m\""), "got: {s}");
    }

    #[test]
    fn exact_table_lines_excludes_pattern_keys() {
        // `{number}` keys go into the pattern table instead.
        let mut up = plain_abbr("up{number}", "cd {number}");
        up.number = Some("../".into());
        let c = cfg(vec![plain_abbr("gst", "git status"), up]);
        let s = exact_table_lines(&c);
        assert!(s.contains("[\"gst\"]"), "exact table should keep gst: {s}");
        assert!(!s.contains("up{number}"), "exact table should drop pattern keys: {s}");
    }

    #[test]
    fn exact_table_lines_excludes_cursor_placeholder_in_key_position_safely() {
        // `{}` cursor placeholder belongs to expand text, not keys.
        // The key filter rejects any `{`, which includes the unlikely
        // case of a `{}` literal in the key. Validator already rejects
        // that, but the filter is the line of defence.
        let mut bad = plain_abbr("ok", "ok");
        bad.key = "bad{}key".into();
        let c = cfg(vec![plain_abbr("gst", "git status"), bad]);
        let s = exact_table_lines(&c);
        assert!(s.contains("[\"gst\"]"), "got: {s}");
        assert!(!s.contains("bad{}key"), "got: {s}");
    }

    #[test]
    fn exact_table_lines_uses_bash_specific_expand_value_when_bound() {
        let a = Abbr {
            key: "open".into(),
            expand: PerShellString::ByShell {
                default: Some("xdg-open".into()),
                bash:    Some("xdg-open --wait".into()),
                zsh: None, pwsh: None, nu: None,
            },
            when_command_exists: None,
            number: None,
        };
        let s = exact_table_lines(&cfg(vec![a]));
        assert!(s.contains("[\"open\"]=\"xdg-open --wait\""), "got: {s}");
    }

    #[test]
    fn exact_table_lines_skips_rules_without_bash_expand_value() {
        // `default = None` + bash = None → for_shell(Bash) returns None
        // and the rule contributes nothing to the bake table.
        let a = Abbr {
            key: "winonly".into(),
            expand: PerShellString::ByShell {
                default: None,
                bash: None,
                zsh: None,
                pwsh: Some("Get-Process".into()),
                nu: None,
            },
            when_command_exists: None,
            number: None,
        };
        let s = exact_table_lines(&cfg(vec![a, plain_abbr("gst", "git status")]));
        assert!(!s.contains("winonly"), "got: {s}");
        assert!(s.contains("[\"gst\"]"), "got: {s}");
    }

    #[test]
    fn exact_table_lines_indents_with_four_spaces() {
        // Cache file readability: every entry indented for inclusion
        // inside the `declare -gA __runex_abbr_expand=(...)` block.
        let s = exact_table_lines(&cfg(vec![plain_abbr("gst", "git status")]));
        assert!(s.starts_with("    "), "expected four-space indent, got: {s:?}");
    }

    #[test]
    fn exact_table_lines_empty_for_empty_config() {
        let s = exact_table_lines(&cfg(vec![]));
        assert_eq!(s, "");
    }

    // ── cond_table_lines ───────────────────────────────────────────────

    use crate::domain::model::PerShellCmds;

    fn abbr_with_when_cmds(key: &str, expand: &str, cmds: Vec<&str>) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(expand.into()),
            when_command_exists: Some(PerShellCmds::All(
                cmds.into_iter().map(String::from).collect(),
            )),
            number: None,
        }
    }

    #[test]
    fn cond_table_lines_emits_entry_for_single_command_guard() {
        let c = cfg(vec![abbr_with_when_cmds("ls", "lsd", vec!["lsd"])]);
        let s = cond_table_lines(&c);
        assert!(s.contains("[\"ls\"]=\"lsd\""), "got: {s}");
    }

    #[test]
    fn cond_table_lines_joins_multi_command_guard_with_colon() {
        // `:` is the conventional bash IFS for PATH-style lists and never
        // appears in a command name, so it's the safest delim for splitting
        // back in the bake dispatcher.
        let c = cfg(vec![abbr_with_when_cmds(
            "ks",
            "kubectl get pods",
            vec!["kubectl", "stern"],
        )]);
        let s = cond_table_lines(&c);
        assert!(s.contains("[\"ks\"]=\"kubectl:stern\""), "got: {s}");
    }

    #[test]
    fn cond_table_lines_skips_rules_without_when_command_exists() {
        let c = cfg(vec![
            plain_abbr("gst", "git status"),
            abbr_with_when_cmds("ls", "lsd", vec!["lsd"]),
        ]);
        let s = cond_table_lines(&c);
        assert!(s.contains("[\"ls\"]"), "got: {s}");
        assert!(!s.contains("[\"gst\"]"), "cond table must not list unguarded rules: {s}");
    }

    #[test]
    fn cond_table_lines_uses_bash_specific_when_command_exists_value() {
        let a = Abbr {
            key: "open".into(),
            expand: PerShellString::All("xdg-open".into()),
            when_command_exists: Some(PerShellCmds::ByShell {
                default: Some(vec!["open".into()]),
                bash:    Some(vec!["xdg-open".into()]),
                zsh: None, pwsh: None, nu: None,
            }),
            number: None,
        };
        let s = cond_table_lines(&cfg(vec![a]));
        assert!(s.contains("[\"open\"]=\"xdg-open\""), "got: {s}");
    }

    #[test]
    fn cond_table_lines_skips_empty_command_list() {
        // Defensive: an empty list would map to an empty string in the
        // bake table and bash's `for c in $conds` would do nothing,
        // which is correct but wastes a line in the cache file.
        let c = cfg(vec![abbr_with_when_cmds("nope", "noop", vec![])]);
        let s = cond_table_lines(&c);
        assert_eq!(s, "");
    }

    #[test]
    fn cond_table_lines_excludes_pattern_keys() {
        // Pattern keys (`{number}`) live in the pattern table, which has
        // its own condition handling. Don't double-list them here.
        let mut up = abbr_with_when_cmds("up{number}", "cd {number}", vec!["pushd"]);
        up.number = Some("../".into());
        let s = cond_table_lines(&cfg(vec![up]));
        assert_eq!(s, "");
    }

    // ── pattern_table_lines ────────────────────────────────────────────

    fn pattern_abbr(key: &str, expand: &str, unit: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(expand.into()),
            when_command_exists: None,
            number: Some(unit.into()),
        }
    }

    #[test]
    fn pattern_table_lines_emits_entry_with_prefix_suffix_template_unit() {
        // `up{number}` → prefix="up", suffix="", template="cd {number}", unit="../"
        let c = cfg(vec![pattern_abbr("up{number}", "cd {number}", "../")]);
        let s = pattern_table_lines(&c);
        // Field separator is bash ANSI-C-quoted US (\037). The four-space
        // indent matches the array-entry convention used elsewhere.
        assert!(
            s.contains("\"up\"$'\\037'\"\"$'\\037'\"cd {number}\"$'\\037'\"../\""),
            "got: {s}"
        );
        assert!(s.starts_with("    "), "expected four-space indent, got: {s:?}");
    }

    #[test]
    fn pattern_table_lines_handles_prefix_and_suffix() {
        // `g{number}p` → prefix="g", suffix="p"
        let c = cfg(vec![pattern_abbr("g{number}p", "git push -n {number}", "x")]);
        let s = pattern_table_lines(&c);
        assert!(
            s.contains("\"g\"$'\\037'\"p\"$'\\037'\"git push -n {number}\"$'\\037'\"x\""),
            "got: {s}"
        );
    }

    #[test]
    fn pattern_table_lines_skips_rules_without_number_unit() {
        // Without a number unit the pattern can't be repeated, so the
        // rule is invalid at validation time; we skip defensively even
        // if the validator missed it.
        let no_unit = Abbr {
            key: "up{number}".into(),
            expand: PerShellString::All("cd {number}".into()),
            when_command_exists: None,
            number: None,
        };
        let s = pattern_table_lines(&cfg(vec![no_unit]));
        assert_eq!(s, "");
    }

    #[test]
    fn pattern_table_lines_skips_rules_without_number_placeholder_in_key() {
        // `number` set but no `{number}` in key — also invalid, skip.
        let weird = Abbr {
            key: "up".into(),
            expand: PerShellString::All("cd".into()),
            when_command_exists: None,
            number: Some("../".into()),
        };
        let s = pattern_table_lines(&cfg(vec![weird]));
        assert_eq!(s, "");
    }

    #[test]
    fn pattern_table_lines_skips_rules_without_bash_expand_value() {
        let a = Abbr {
            key: "up{number}".into(),
            expand: PerShellString::ByShell {
                default: None,
                bash: None,
                zsh: None,
                pwsh: Some("Set-Location ..".into()),
                nu: None,
            },
            when_command_exists: None,
            number: Some("../".into()),
        };
        let s = pattern_table_lines(&cfg(vec![a]));
        assert_eq!(s, "");
    }

    #[test]
    fn pattern_table_lines_empty_for_empty_config() {
        assert_eq!(pattern_table_lines(&cfg(vec![])), "");
    }
}

/// Wrap `s` as a bash double-quoted string suitable for embedding inside
/// an associative-array initializer like `["key"]="value"`.
///
/// Escapes the four characters that bash interprets inside a
/// double-quoted string (`"`, `\`, `$`, `` ` ``) so the value survives
/// as literal bytes. Single quotes are left alone — they are literal
/// inside double quotes and the `{}` placeholder is frequently embedded
/// inside `'...'` argument quoting.
///
/// ASCII control characters and deceptive Unicode are silently dropped,
/// matching the policy of [`crate::domain::shell::bash_quote_string`].
fn bash_double_quote_for_assoc(s: &str) -> String {
    use crate::domain::sanitize::{is_deceptive_unicode, is_unicode_line_separator};
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            '`' => out.push_str("\\`"),
            c if c.is_ascii_control() => {}
            c if is_unicode_line_separator(c) => {}
            c if is_deceptive_unicode(c) => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Build the `__runex_abbr_expand` associative-array body for the bake
/// dispatcher: one `    ["key"]="expand"` line per non-pattern rule.
///
/// Rules whose key contains `{` are skipped — they are pattern rules and
/// are handled by [`pattern_table_lines`] further down. Rules without a
/// bash-applicable expansion (e.g. `pwsh`-only `ByShell` with no
/// `default`) are dropped silently; the user already validated that the
/// config makes sense for the shells they care about, and dropping is the
/// only response that keeps the bake path bytewise equivalent to the
/// exec path for bash.
fn exact_table_lines(config: &Config) -> String {
    let mut lines = Vec::new();
    for rule in &config.abbr {
        if rule.key.contains('{') {
            continue;
        }
        let Some(expand) = rule.expand.for_shell(Shell::Bash) else {
            continue;
        };
        lines.push(format!(
            "    [{}]={}",
            bash_double_quote_for_assoc(&rule.key),
            bash_double_quote_for_assoc(expand),
        ));
    }
    lines.join("\n")
}

/// Build the `__runex_abbr_cond` associative-array body: one
/// `    ["key"]="cmd1:cmd2"` line per rule that has a non-empty
/// `when_command_exists` list for bash.
///
/// `:` is used as the join character because the dispatcher in
/// `bash.sh` splits the list with `IFS=':'` — neither a command name
/// nor a `bash_double_quote_for_assoc`'d byte sequence can contain a
/// raw `:` that would confuse the split. Empty lists are skipped (a
/// guard of "no commands required" is equivalent to no guard at all).
fn cond_table_lines(config: &Config) -> String {
    let mut lines = Vec::new();
    for rule in &config.abbr {
        if rule.key.contains('{') {
            continue;
        }
        let Some(cmds) = rule
            .when_command_exists
            .as_ref()
            .and_then(|w| w.for_shell(Shell::Bash))
        else {
            continue;
        };
        if cmds.is_empty() {
            continue;
        }
        let joined = cmds.join(":");
        lines.push(format!(
            "    [{}]={}",
            bash_double_quote_for_assoc(&rule.key),
            bash_double_quote_for_assoc(&joined),
        ));
    }
    lines.join("\n")
}

/// Build the `__runex_abbr_patterns` indexed-array body for rules whose
/// key contains `{number}`.
///
/// Each emitted line is a single bash double-quoted string concatenated
/// with `$'\037'` (ANSI-C-quoted US, 0x1F) field separators:
///
/// ```text
///     "prefix"$'\037'"suffix"$'\037'"template"$'\037'"unit"
/// ```
///
/// The bake dispatcher in `bash.sh` splits this with
/// `IFS=$'\037' read -r prefix suffix template unit`. US is safe as a
/// separator because the config validator rejects every ASCII control
/// character in user-facing fields, so it can never appear inside
/// `prefix` / `suffix` / `template` / `unit`.
fn pattern_table_lines(config: &Config) -> String {
    let mut lines = Vec::new();
    for rule in &config.abbr {
        let Some(unit) = rule.number.as_deref() else {
            continue;
        };
        let Some(pos) = rule.key.find(NUMBER_PLACEHOLDER) else {
            continue;
        };
        let Some(template) = rule.expand.for_shell(Shell::Bash) else {
            continue;
        };
        let prefix = &rule.key[..pos];
        let suffix = &rule.key[pos + NUMBER_PLACEHOLDER.len()..];
        let sep = "$'\\037'";
        lines.push(format!(
            "    {prefix}{sep}{suffix}{sep}{template}{sep}{unit}",
            prefix   = bash_double_quote_for_assoc(prefix),
            suffix   = bash_double_quote_for_assoc(suffix),
            template = bash_double_quote_for_assoc(template),
            unit     = bash_double_quote_for_assoc(unit),
            sep      = sep,
        ));
    }
    lines.join("\n")
}

/// Generate the full cygwin/msys bake-mode dispatcher block:
///
/// 1. `__runex_cyg_expand` — public entry, called from `__runex_expand`
///    when sourced under Git Bash (selected by the `case "${OSTYPE-}"`
///    switch at the bottom of this block).
/// 2. `__runex_abbr_expand` / `__runex_abbr_cond` / `__runex_abbr_patterns`
///    — static tables baked from `config`.
/// 3. `__runex_cyg_lookup` / `__runex_cyg_pattern_lookup` / `__runex_cyg_render`
///    — helpers that operate purely on bash variables (no subprocesses).
/// 4. `case "${OSTYPE-}"` — re-defines `__runex_expand` to either the
///    bake path (cygwin / msys) or keep the exec path (Linux / WSL).
///
/// This block is inserted into `bash.sh` at `{BASH_CYG_DISPATCHER}` and
/// is empty when `runex export bash` is called without a config so the
/// legacy escape hatch (`eval "$(runex export bash)"`) stays unchanged.
pub(crate) fn generate_cygwin_dispatcher(config: &Config) -> String {
    let exact = exact_table_lines(config);
    let cond = cond_table_lines(config);
    let patterns = pattern_table_lines(config);
    let exact_block = if exact.is_empty() { String::new() } else { format!("\n{exact}\n") };
    let cond_block  = if cond.is_empty()  { String::new() } else { format!("\n{cond}\n") };
    let pattern_block = if patterns.is_empty() { String::new() } else { format!("\n{patterns}\n") };
    format!(
        r#"declare -gA __runex_abbr_expand=({exact_block})
declare -gA __runex_abbr_cond=({cond_block})
__runex_abbr_patterns=({pattern_block})
__runex_cyg_render() {{
    local text="$1" pos
    pos="${{text%%\{{\}}*}}"
    if [ "$pos" = "$text" ]; then
        __runex_out="$text"
        __runex_cursor_off=""
    else
        __runex_cursor_off="${{#pos}}"
        __runex_out="${{pos}}${{text#*\{{\}}}}"
    fi
}}
__runex_cyg_lookup() {{
    local key="$1" raw conds c
    __runex_out=""
    __runex_cursor_off=""
    raw="${{__runex_abbr_expand[$key]-}}"
    [ -z "$raw" ] && return
    conds="${{__runex_abbr_cond[$key]-}}"
    if [ -n "$conds" ]; then
        local IFS=':'
        for c in $conds; do command -v "$c" >/dev/null 2>&1 || return; done
    fi
    [ "$raw" = "$key" ] && return
    __runex_cyg_render "$raw"
}}
__runex_cyg_pattern_lookup() {{
    local token="$1" entry prefix suffix template unit rest n i repeated rendered
    __runex_out=""
    __runex_cursor_off=""
    for entry in "${{__runex_abbr_patterns[@]}}"; do
        IFS=$'\037' read -r prefix suffix template unit <<<"$entry"
        [ "${{token#"$prefix"}}" = "$token" ] && continue
        rest="${{token#"$prefix"}}"
        if [ -n "$suffix" ]; then
            [ "${{rest%"$suffix"}}" = "$rest" ] && continue
            rest="${{rest%"$suffix"}}"
        fi
        [ -z "$rest" ] && continue
        case "$rest" in (*[!0-9]*) continue ;; esac
        n="$rest"
        [ "$n" -le 0 ] 2>/dev/null && continue
        [ "$n" -gt 128 ] 2>/dev/null && continue
        repeated=""
        for ((i=0; i<n; i++)); do repeated="${{repeated}}${{unit}}"; done
        [ "${{#repeated}}" -gt 4096 ] && continue
        rendered="${{template//\{{number\}}/$repeated}}"
        [ "${{#rendered}}" -gt 4096 ] && continue
        __runex_cyg_render "$rendered"
        return
    done
}}
__runex_cyg_expand() {{
    local left right token prefix
    left="${{READLINE_LINE:0:READLINE_POINT}}"
    right="${{READLINE_LINE:READLINE_POINT}}"
    if [ -n "$right" ] && [ "${{right:0:1}}" != " " ]; then
        READLINE_LINE="${{left}} ${{right}}"
        READLINE_POINT=$((READLINE_POINT + 1))
        return
    fi
    token="${{left##* }}"
    if [ -z "$token" ]; then
        READLINE_LINE="${{left}} ${{right}}"
        READLINE_POINT=$((READLINE_POINT + 1))
        return
    fi
    __runex_cyg_lookup "$token"
    if [ -z "$__runex_out" ]; then __runex_cyg_pattern_lookup "$token"; fi
    if [ -z "$__runex_out" ]; then
        READLINE_LINE="${{left}} ${{right}}"
        READLINE_POINT=$((READLINE_POINT + 1))
        return
    fi
    prefix="${{left%$token}}"
    if [ -n "$__runex_cursor_off" ]; then
        READLINE_LINE="${{prefix}}${{__runex_out}}${{right}}"
        READLINE_POINT=$(( ${{#prefix}} + __runex_cursor_off ))
    else
        READLINE_LINE="${{prefix}}${{__runex_out}} ${{right}}"
        READLINE_POINT=$(( ${{#prefix}} + ${{#__runex_out}} + 1 ))
    fi
}}
"#,
        exact_block = exact_block,
        cond_block = cond_block,
        pattern_block = pattern_block,
    )
}
