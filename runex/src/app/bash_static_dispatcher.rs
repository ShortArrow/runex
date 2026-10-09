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
//! Git Bash. The cache file embeds the abbreviation rules as bash
//! indexed arrays and re-implements the lookup/render in pure bash.
//! A runtime `case "${OSTYPE-}"` switch inside the same cache file
//! routes Git Bash to this bake-mode dispatcher and Linux/WSL bash to
//! the existing `runex hook` exec path; one cache file serves every
//! bash flavour the user might run with the same dotfiles.
//!
//! ## Command-position parity (issue #9, fixed in 0.1.19)
//!
//! The bake path reproduces `domain::hook::is_command_position` in pure
//! bash via `__runex_cyg_is_command_position`: a `case` over the four
//! pipeline operators (`&&`, `||`, `|`, `;`) plus a trailing-`sudo`
//! recursion that itself defers to the same operator check. The result
//! is byte-equivalent to what the Linux / WSL exec path returns for the
//! same READLINE_LINE / READLINE_POINT. The 0.1.17 trade-off where
//! Git Bash expanded any trailing token regardless of context (e.g.
//! `echo gst<Space>` would still expand `gst`) no longer applies.

use crate::domain::expand::{always_renders_blank, NUMBER_PLACEHOLDER};
use crate::domain::model::{Config, MatchKind, Shell};

/// Wrap `s` as a bash double-quoted string suitable for embedding as a
/// field of a baked table entry like `"key"$'\037'"value"`.
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

/// The rule's `when_command_exists` for bash, joined with `:` — the
/// separator the bake dispatcher splits on, which no command name
/// contains.
///
/// Returns `Some("")` for an unconditional rule (or an empty list, which
/// the exec path also treats as satisfied) and `None` when the rule has
/// a condition table without a bash entry: the exec path skips such a
/// rule on bash, so the bake tables omit it.
fn bash_conditions(rule: &crate::domain::model::Abbr) -> Option<String> {
    match &rule.when_command_exists {
        None => Some(String::new()),
        Some(per_shell) => per_shell.for_shell(Shell::Bash).map(|cmds| cmds.join(":")),
    }
}

/// Build the `__runex_abbr_exact` indexed-array body for exact rules
/// (no `{` in the key, no `match`), one line per rule in config order:
///
/// ```text
///     "key"$'\037'"template"$'\037'"cmd1:cmd2"
/// ```
///
/// Duplicate keys are all kept so the dispatcher can fall through from a
/// skipped rule to the next one with the same key, as the exec path
/// does. The third field is empty for an unconditional rule. Rules
/// without a bash expansion, or whose condition has no bash entry (see
/// [`bash_conditions`]), are omitted. `{number}` rules go to
/// [`pattern_table_lines`], glob rules to [`glob_table_lines`], and regex
/// rules (issue #20) to no table: Git Bash never expands them.
fn exact_table_lines(config: &Config) -> String {
    let mut lines = Vec::new();
    for rule in &config.abbr {
        if rule.key.contains('{') || rule.match_kind.is_some() {
            continue;
        }
        let Some(template) = rule.expand.for_shell(Shell::Bash) else {
            continue;
        };
        if always_renders_blank(rule, template) {
            continue;
        }
        let Some(conds) = bash_conditions(rule) else {
            continue;
        };
        let sep = "$'\\037'";
        lines.push(format!(
            "    {key}{sep}{template}{sep}{conds}",
            key = bash_double_quote_for_assoc(&rule.key),
            template = bash_double_quote_for_assoc(template),
            conds = bash_double_quote_for_assoc(&conds),
        ));
    }
    lines.join("\n")
}

/// Build the `__runex_abbr_patterns` indexed-array body for rules whose
/// key contains `{number}`, in config order.
///
/// Each emitted line is a single bash double-quoted string concatenated
/// with `$'\037'` (ANSI-C-quoted US, 0x1F) field separators:
///
/// ```text
///     "prefix"$'\037'"suffix"$'\037'"template"$'\037'"unit"$'\037'"cmd1:cmd2"
/// ```
///
/// The bake dispatcher splits this with
/// `IFS=$'\037' read -r prefix suffix template unit conds`. US is safe as
/// a separator because the config validator rejects every ASCII control
/// character in user-facing fields. The last field is the bash
/// condition from [`bash_conditions`]; a rule whose condition has no
/// bash entry is omitted.
fn pattern_table_lines(config: &Config) -> String {
    let mut lines = Vec::new();
    for rule in config.abbr.iter().filter(|rule| rule.match_kind.is_none()) {
        let Some(unit) = rule.number.as_deref() else {
            continue;
        };
        let Some(pos) = rule.key.find(NUMBER_PLACEHOLDER) else {
            continue;
        };
        let Some(template) = rule.expand.for_shell(Shell::Bash) else {
            continue;
        };
        if always_renders_blank(rule, template) {
            continue;
        }
        let Some(conds) = bash_conditions(rule) else {
            continue;
        };
        let prefix = &rule.key[..pos];
        let suffix = &rule.key[pos + NUMBER_PLACEHOLDER.len()..];
        let sep = "$'\\037'";
        lines.push(format!(
            "    {prefix}{sep}{suffix}{sep}{template}{sep}{unit}{sep}{conds}",
            prefix   = bash_double_quote_for_assoc(prefix),
            suffix   = bash_double_quote_for_assoc(suffix),
            template = bash_double_quote_for_assoc(template),
            unit     = bash_double_quote_for_assoc(unit),
            conds    = bash_double_quote_for_assoc(&conds),
        ));
    }
    lines.join("\n")
}

/// Build the `__runex_abbr_globs` indexed-array body for `match =
/// "glob"` rules (issue #19), in config order:
///
/// ```text
///     "pattern"$'\037'"head"$'\037'"tail"$'\037'"template"$'\037'"cmd1:cmd2"
/// ```
///
/// `pattern` is the key, handed unquoted to a bash `case` pattern; the
/// validator restricts glob keys to `*` and `?` as special characters,
/// so bash and `domain::expand::match_glob_key` agree on what matches.
/// `head` / `tail` are the key's parts around its single `*` (the whole
/// key and `""` when there is none) and strip the capture out of the
/// token. The last field is the bash condition from [`bash_conditions`].
fn glob_table_lines(config: &Config) -> String {
    let mut lines = Vec::new();
    for rule in config.abbr.iter().filter(|rule| rule.match_kind == Some(MatchKind::Glob)) {
        let Some(template) = rule.expand.for_shell(Shell::Bash) else {
            continue;
        };
        if always_renders_blank(rule, template) {
            continue;
        }
        let (head, tail) = rule.key.split_once('*').unwrap_or((rule.key.as_str(), ""));
        let Some(conds) = bash_conditions(rule) else {
            continue;
        };
        let sep = "$'\\037'";
        lines.push(format!(
            "    {pattern}{sep}{head}{sep}{tail}{sep}{template}{sep}{conds}",
            pattern = bash_double_quote_for_assoc(&rule.key),
            head = bash_double_quote_for_assoc(head),
            tail = bash_double_quote_for_assoc(tail),
            template = bash_double_quote_for_assoc(template),
            conds = bash_double_quote_for_assoc(&conds),
        ));
    }
    lines.join("\n")
}

/// Every character Rust's `char::is_whitespace` accepts, as a bash word
/// for a `[...]` bracket expression: the ASCII ones ANSI-C quoted, the
/// rest as literal UTF-8 in double quotes. The glob scan strips these to
/// test a rendering for blankness the way the exec path does (issue #52).
fn bash_blank_chars() -> String {
    let (ascii, other): (Vec<char>, Vec<char>) =
        (0..=0x10FFFF).filter_map(char::from_u32).filter(|c| c.is_whitespace()).partition(char::is_ascii);
    let ascii: String = ascii
        .iter()
        .map(|c| match c {
            ' ' => " ".to_string(),
            c => format!("\\x{:02x}", *c as u32),
        })
        .collect();
    let other: String = other.into_iter().collect();
    format!("$'{ascii}'\"{other}\"")
}

/// Generate the full cygwin/msys bake-mode dispatcher block:
///
/// 1. `__runex_cyg_expand` — public entry, called from `__runex_expand`
///    when sourced under Git Bash (selected by the `case "${OSTYPE-}"`
///    switch at the bottom of this block).
/// 2. `__runex_abbr_exact` / `__runex_abbr_patterns` / `__runex_abbr_globs`
///    — indexed arrays baked from `config`, one per phase, each in config
///    order with the rule's bash condition as the last field.
/// 3. `__runex_cyg_lookup` / `__runex_cyg_pattern_lookup` /
///    `__runex_cyg_glob_lookup` — ordered scans over those tables that
///    skip a rule the exec path would skip (self-loop, missing command,
///    over the 4096-byte cap) and fall through to the next one; plus
///    `__runex_cyg_render` / `__runex_cyg_conds_met` /
///    `__runex_cyg_byte_len`. All operate purely on bash variables (no
///    subprocesses).
/// 4. `case "${OSTYPE-}"` — re-defines `__runex_expand` to either the
///    bake path (cygwin / msys) or keep the exec path (Linux / WSL).
///
/// This block is inserted into `bash.sh` at `{BASH_CYG_DISPATCHER}` and
/// is empty when `runex export bash` is called without a config so the
/// legacy escape hatch (`eval "$(runex export bash)"`) stays unchanged.
pub(crate) fn generate_cygwin_dispatcher(config: &Config) -> String {
    let exact = exact_table_lines(config);
    let patterns = pattern_table_lines(config);
    let exact_block = if exact.is_empty() { String::new() } else { format!("\n{exact}\n") };
    let pattern_block = if patterns.is_empty() { String::new() } else { format!("\n{patterns}\n") };
    let globs = glob_table_lines(config);
    let glob_block = if globs.is_empty() { String::new() } else { format!("\n{globs}\n") };
    let blank_chars = bash_blank_chars();
    format!(
        r#"__runex_abbr_exact=({exact_block})
__runex_abbr_patterns=({pattern_block})
__runex_abbr_globs=({glob_block})
__runex_blank_chars={blank_chars}
__runex_cyg_render() {{
    local text="$1" pos
    __runex_hit=1
    pos="${{text%%\{{\}}*}}"
    if [ "$pos" = "$text" ]; then
        __runex_out="$text"
        __runex_cursor_off=""
    else
        __runex_cursor_off="${{#pos}}"
        __runex_out="${{pos}}${{text#*\{{\}}}}"
    fi
}}
declare -gA __runex_cmd_seen=()
__runex_cyg_conds_met() {{
    local conds="$1" c
    __runex_conds_met=1
    [ -z "$conds" ] && return
    local IFS=':'
    for c in $conds; do
        if [ -z "${{__runex_cmd_seen[$c]-}}" ]; then
            if type -P -- "$c" >/dev/null 2>&1; then __runex_cmd_seen[$c]=1; else __runex_cmd_seen[$c]=0; fi
        fi
        [ "${{__runex_cmd_seen[$c]}}" = 1 ] || __runex_conds_met=0
    done
    IFS=$' \t\n'
}}
__runex_cyg_lookup() {{
    local token="$1" entry key template conds
    __runex_out=""
    __runex_cursor_off=""
    for entry in "${{__runex_abbr_exact[@]}}"; do
        IFS=$'\037' read -r key template conds <<<"$entry"
        [ "$key" = "$token" ] || continue
        [ "$template" = "$key" ] && continue
        __runex_cyg_conds_met "$conds"
        [ "$__runex_conds_met" -eq 1 ] || continue
        __runex_cyg_render "$template"
        return
    done
}}
__runex_cyg_pattern_lookup() {{
    local token="$1" entry prefix suffix template unit conds rest digits n i repeated rendered
    __runex_out=""
    __runex_cursor_off=""
    for entry in "${{__runex_abbr_patterns[@]}}"; do
        IFS=$'\037' read -r prefix suffix template unit conds <<<"$entry"
        if [ -n "$prefix" ] && [ "${{token#"$prefix"}}" = "$token" ]; then continue; fi
        rest="${{token#"$prefix"}}"
        if [ -n "$suffix" ]; then
            [ "${{rest%"$suffix"}}" = "$rest" ] && continue
            rest="${{rest%"$suffix"}}"
        fi
        [ -z "$rest" ] && continue
        case "$rest" in (*[!0-9]*) continue ;; esac
        digits="${{rest#"${{rest%%[!0]*}}"}}"
        [ -z "$digits" ] && continue
        [ "${{#digits}}" -gt 3 ] && continue
        n=$((10#$digits))
        [ "$n" -gt 128 ] && continue
        __runex_cyg_conds_met "$conds"
        [ "$__runex_conds_met" -eq 1 ] || continue
        repeated=""
        for ((i=0; i<n; i++)); do repeated="${{repeated}}${{unit}}"; done
        __runex_cyg_byte_len "$repeated"
        [ "$__runex_len" -gt 4096 ] && continue
        rendered="${{template//"{{number}}"/"$repeated"}}"
        __runex_cyg_byte_len "$rendered"
        [ "$__runex_len" -gt 4096 ] && continue
        __runex_cyg_render "$rendered"
        return
    done
}}
__runex_cyg_byte_len() {{
    local LC_ALL=C
    __runex_len="${{#1}}"
}}
__runex_cyg_glob_lookup() {{
    local token="$1" entry pattern head tail template conds rest left right
    __runex_out=""
    __runex_cursor_off=""
    for entry in "${{__runex_abbr_globs[@]}}"; do
        IFS=$'\037' read -r pattern head tail template conds <<<"$entry"
        case "$token" in $pattern) ;; *) continue ;; esac
        __runex_cyg_conds_met "$conds"
        [ "$__runex_conds_met" -eq 1 ] || continue
        rest="${{token#$head}}"
        rest="${{rest%$tail}}"
        if [[ "$template" == *"{{}}"* ]]; then
            left="${{template%%"{{}}"*}}"
            right="${{template#*"{{}}"}}"
            left="${{left//"{{*}}"/"$rest"}}"
            right="${{right//"{{*}}"/"$rest"}}"
            __runex_out="${{left}}${{right}}"
            __runex_cursor_off="${{#left}}"
        else
            __runex_out="${{template//"{{*}}"/"$rest"}}"
            __runex_cursor_off=""
        fi
        __runex_cyg_byte_len "$__runex_out"
        if [ "$__runex_len" -gt 4096 ]; then
            __runex_out=""
            __runex_cursor_off=""
            continue
        fi
        if [ -z "${{__runex_out//[$__runex_blank_chars]/}}" ] || [ "$__runex_out" = "$token" ]; then
            __runex_out=""
            __runex_cursor_off=""
            continue
        fi
        __runex_hit=1
        return
    done
}}
__runex_cyg_is_command_position() {{
    # Sets __runex_cmd_pos = 1 if $1 is a command-position prefix,
    # 0 otherwise. Mirrors `domain::hook::is_command_position`:
    #   - empty / whitespace-only prefix → command position
    #   - ends with `&&` / `||` / `|` / `;` → command position
    #   - ends with the word `sudo` whose prefix is itself command
    #     position → command position
    local prefix="$1"
    while [ "${{prefix: -1}}" = " " ]; do
        prefix="${{prefix:0:${{#prefix}}-1}}"
    done
    if [ -z "$prefix" ]; then __runex_cmd_pos=1; return; fi
    case "$prefix" in
        *"&&"|*"||"|*"|"|*";") __runex_cmd_pos=1; return ;;
    esac
    local last_word="${{prefix##* }}"
    local before_last
    if [ "$last_word" = "$prefix" ]; then
        before_last=""
    else
        before_last="${{prefix:0:$((${{#prefix}} - ${{#last_word}}))}}"
    fi
    if [ "$last_word" = "sudo" ]; then
        while [ "${{before_last: -1}}" = " " ]; do
            before_last="${{before_last:0:${{#before_last}}-1}}"
        done
        if [ -z "$before_last" ]; then __runex_cmd_pos=1; return; fi
        case "$before_last" in
            *"&&"|*"||"|*"|"|*";") __runex_cmd_pos=1; return ;;
        esac
    fi
    __runex_cmd_pos=0
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
    # Substring slice for prefix (= avoid `${{left%$token}}` glob
    # interpretation when the token contains `?`, `*`, or `[`).
    prefix="${{left:0:$((${{#left}} - ${{#token}}))}}"
    # Command-position check (issue #9): if the prefix is not a
    # command position, fall back to a literal space insertion
    # without consulting the abbreviation tables.
    __runex_cyg_is_command_position "$prefix"
    if [ "$__runex_cmd_pos" -eq 0 ]; then
        READLINE_LINE="${{left}} ${{right}}"
        READLINE_POINT=$((READLINE_POINT + 1))
        return
    fi
    __runex_cmd_seen=()
    local restore_nocasematch=0
    if shopt -q nocasematch; then
        restore_nocasematch=1
        shopt -u nocasematch
    fi
    __runex_hit=0
    __runex_cyg_lookup "$token"
    if [ "$__runex_hit" -eq 0 ]; then __runex_cyg_pattern_lookup "$token"; fi
    if [ "$__runex_hit" -eq 0 ]; then __runex_cyg_glob_lookup "$token"; fi
    if [ "$restore_nocasematch" -eq 1 ]; then shopt -s nocasematch; fi
    if [ "$__runex_hit" -eq 0 ]; then
        READLINE_LINE="${{left}} ${{right}}"
        READLINE_POINT=$((READLINE_POINT + 1))
        return
    fi
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
        pattern_block = pattern_block,
        glob_block = glob_block,
    )
}


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

    use crate::domain::model::{Abbr, KeybindConfig, PerShellCmds, PerShellString, PrecacheConfig};

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
            match_kind: None,
        }
    }

    fn abbr_with_when_cmds(key: &str, expand: &str, cmds: Vec<&str>) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(expand.into()),
            when_command_exists: Some(PerShellCmds::All(
                cmds.into_iter().map(String::from).collect(),
            )),
            number: None,
            match_kind: None,
        }
    }

    fn pwsh_only_condition() -> Option<PerShellCmds> {
        Some(PerShellCmds::ByShell {
            default: None,
            bash: None,
            zsh: None,
            pwsh: Some(vec!["git".into()]),
            nu: None,
        })
    }

    #[test]
    fn exact_table_lines_emits_one_entry_per_exact_rule_in_config_order() {
        let c = cfg(vec![
            plain_abbr("gst", "git status"),
            plain_abbr("gcm", "git commit -m"),
        ]);
        assert_eq!(
            exact_table_lines(&c),
            "    \"gst\"$'\\037'\"git status\"$'\\037'\"\"\n    \"gcm\"$'\\037'\"git commit -m\"$'\\037'\"\""
        );
    }

    #[test]
    fn exact_table_lines_keeps_duplicate_keys_in_config_order() {
        let c = cfg(vec![
            abbr_with_when_cmds("ls", "lsd", vec!["lsd"]),
            plain_abbr("ls", "ls --color"),
        ]);
        assert_eq!(
            exact_table_lines(&c),
            "    \"ls\"$'\\037'\"lsd\"$'\\037'\"lsd\"\n    \"ls\"$'\\037'\"ls --color\"$'\\037'\"\""
        );
    }

    #[test]
    fn exact_table_lines_joins_multi_command_condition_with_colon() {
        let c = cfg(vec![abbr_with_when_cmds(
            "ks",
            "kubectl get pods",
            vec!["kubectl", "stern"],
        )]);
        assert_eq!(
            exact_table_lines(&c),
            "    \"ks\"$'\\037'\"kubectl get pods\"$'\\037'\"kubectl:stern\""
        );
    }

    #[test]
    fn exact_table_lines_uses_bash_specific_when_command_exists_value() {
        let a = Abbr {
            when_command_exists: Some(PerShellCmds::ByShell {
                default: Some(vec!["open".into()]),
                bash:    Some(vec!["xdg-open".into()]),
                zsh: None, pwsh: None, nu: None,
            }),
            ..plain_abbr("open", "xdg-open")
        };
        assert_eq!(
            exact_table_lines(&cfg(vec![a])),
            "    \"open\"$'\\037'\"xdg-open\"$'\\037'\"xdg-open\""
        );
    }

    #[test]
    fn exact_table_lines_omits_rule_whose_condition_has_no_bash_entry() {
        let pwsh_only = Abbr {
            when_command_exists: pwsh_only_condition(),
            ..plain_abbr("pwx", "pwshexact")
        };
        let c = cfg(vec![pwsh_only, plain_abbr("gst", "git status")]);
        assert_eq!(
            exact_table_lines(&c),
            "    \"gst\"$'\\037'\"git status\"$'\\037'\"\""
        );
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
            match_kind: None,
        };
        assert_eq!(
            exact_table_lines(&cfg(vec![a])),
            "    \"open\"$'\\037'\"xdg-open --wait\"$'\\037'\"\""
        );
    }

    #[test]
    fn exact_table_lines_skips_rules_without_bash_expand_value() {
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
            match_kind: None,
        };
        assert_eq!(
            exact_table_lines(&cfg(vec![a, plain_abbr("gst", "git status")])),
            "    \"gst\"$'\\037'\"git status\"$'\\037'\"\""
        );
    }

    #[test]
    fn exact_table_lines_excludes_number_and_glob_rules() {
        let c = cfg(vec![
            plain_abbr("gst", "git status"),
            pattern_abbr("up{number}", "cd {number}", "../"),
            glob_abbr("k*", "kubectl {*}"),
        ]);
        assert_eq!(
            exact_table_lines(&c),
            "    \"gst\"$'\\037'\"git status\"$'\\037'\"\""
        );
    }

    #[test]
    fn exact_table_lines_excludes_brace_keys_defensively() {
        let mut bad = plain_abbr("ok", "ok");
        bad.key = "bad{}key".into();
        let c = cfg(vec![plain_abbr("gst", "git status"), bad]);
        assert_eq!(
            exact_table_lines(&c),
            "    \"gst\"$'\\037'\"git status\"$'\\037'\"\""
        );
    }

    #[test]
    fn exact_table_lines_empty_for_empty_config() {
        assert_eq!(exact_table_lines(&cfg(vec![])), "");
    }

    // ── glob rules (issue #19) ─────────────────────────────────────────

    fn glob_abbr(key: &str, expand: &str) -> Abbr {
        Abbr {
            match_kind: Some(crate::domain::model::MatchKind::Glob),
            ..plain_abbr(key, expand)
        }
    }

    #[test]
    fn glob_table_lines_emits_pattern_head_tail_template_and_conditions() {
        let rule = Abbr {
            match_kind: Some(crate::domain::model::MatchKind::Glob),
            ..abbr_with_when_cmds("d*x", "docker {*}", vec!["docker", "grep"])
        };
        let s = glob_table_lines(&cfg(vec![rule]));
        assert_eq!(
            s,
            "    \"d*x\"$'\\037'\"d\"$'\\037'\"x\"$'\\037'\"docker {*}\"$'\\037'\"docker:grep\""
        );
    }

    #[test]
    fn glob_table_lines_uses_the_whole_key_as_head_without_a_star() {
        let s = glob_table_lines(&cfg(vec![glob_abbr("k?", "kubectl")]));
        assert_eq!(s, "    \"k?\"$'\\037'\"k?\"$'\\037'\"\"$'\\037'\"kubectl\"$'\\037'\"\"");
    }

    #[test]
    fn glob_table_lines_skips_exact_and_number_rules() {
        let c = cfg(vec![plain_abbr("gst", "git status"), pattern_abbr("up{number}", "cd {number}", "../")]);
        assert_eq!(glob_table_lines(&c), "");
    }

    // ── regex rules (issue #20): not baked ─────────────────────────────

    fn regex_abbr(key: &str, expand: &str) -> Abbr {
        Abbr {
            match_kind: Some(crate::domain::model::MatchKind::Regex),
            ..plain_abbr(key, expand)
        }
    }

    #[test]
    fn regex_rules_go_into_no_bake_table() {
        let regex_with_cond = Abbr {
            match_kind: Some(crate::domain::model::MatchKind::Regex),
            ..abbr_with_when_cmds(r"r(\w+)", "REGEX {1}", vec!["git"])
        };
        let regex_with_number_text = Abbr {
            match_kind: Some(crate::domain::model::MatchKind::Regex),
            ..pattern_abbr("up{number}", "cd {number}", "../")
        };
        let c = cfg(vec![
            regex_with_cond,
            regex_abbr("a{2}", "AA"),
            regex_with_number_text,
            glob_abbr("k*", "kubectl {*}"),
        ]);
        let glob_only = glob_table_lines(&cfg(vec![glob_abbr("k*", "kubectl {*}")]));
        assert_eq!(glob_table_lines(&c), glob_only);
        assert_eq!(exact_table_lines(&c), "");
        assert_eq!(pattern_table_lines(&c), "");
    }

    #[test]
    fn bake_expand_tries_the_glob_table_after_the_number_table() {
        let s = generate_cygwin_dispatcher(&cfg(vec![glob_abbr("k*", "kubectl {*}")]));
        let number = s.find("__runex_cyg_pattern_lookup \"$token\"; fi").expect("number lookup call");
        let glob = s.find("__runex_cyg_glob_lookup \"$token\"; fi").expect("glob lookup call");
        assert!(number < glob, "glob rules come after {{number}} rules (ADR 0005)");
    }

    // ── pattern_table_lines ────────────────────────────────────────────

    fn pattern_abbr(key: &str, expand: &str, unit: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(expand.into()),
            when_command_exists: None,
            number: Some(unit.into()),
            match_kind: None,
        }
    }

    #[test]
    fn pattern_table_lines_emits_entry_with_prefix_suffix_template_unit() {
        // `up{number}` → prefix="up", suffix="", template="cd {number}", unit="../"
        let c = cfg(vec![pattern_abbr("up{number}", "cd {number}", "../")]);
        let s = pattern_table_lines(&c);
        // Field separator is bash ANSI-C-quoted US (\037). The four-space
        // indent matches the array-entry convention used elsewhere.
        assert_eq!(
            s,
            "    \"up\"$'\\037'\"\"$'\\037'\"cd {number}\"$'\\037'\"../\"$'\\037'\"\""
        );
    }

    #[test]
    fn bash_blank_chars_lists_ascii_and_unicode_whitespace() {
        let s = bash_blank_chars();
        assert!(s.starts_with(r"$'\x09\x0a\x0b\x0c\x0d '"), "{s:?}");
        for c in ['\u{85}', '\u{a0}', '\u{2028}', '\u{3000}'] {
            assert!(s.contains(c), "missing U+{:04X}: {s:?}", c as u32);
        }
        assert!(!s.contains('\u{200b}'), "zero-width space is not whitespace: {s:?}");
    }

    /// Issue #52: a rule whose bash expansion is blank once `{}` is
    /// removed never fires, so the bake tables leave it out.
    #[test]
    fn exact_and_pattern_tables_omit_rules_that_always_render_blank() {
        let c = cfg(vec![
            plain_abbr("zz", "{}"),
            plain_abbr("zz", " {} "),
            plain_abbr("zz", "zz2"),
            pattern_abbr("ez{number}", "{}", "x"),
            pattern_abbr("ez{number}", "{number}", " "),
            pattern_abbr("ez{number}", "E{number}", "x"),
        ]);
        let kept = cfg(vec![plain_abbr("zz", "zz2"), pattern_abbr("ez{number}", "E{number}", "x")]);
        assert_eq!(exact_table_lines(&c), exact_table_lines(&kept));
        assert_eq!(pattern_table_lines(&c), pattern_table_lines(&kept));
    }

    #[test]
    fn pattern_table_lines_carries_the_bash_condition_as_fifth_field() {
        let up = Abbr {
            when_command_exists: Some(PerShellCmds::All(vec!["pushd".into(), "popd".into()])),
            ..pattern_abbr("up{number}", "cd {number}", "../")
        };
        assert_eq!(
            pattern_table_lines(&cfg(vec![up])),
            "    \"up\"$'\\037'\"\"$'\\037'\"cd {number}\"$'\\037'\"../\"$'\\037'\"pushd:popd\""
        );
    }

    #[test]
    fn pattern_table_lines_omits_rule_whose_condition_has_no_bash_entry() {
        let up = Abbr {
            when_command_exists: pwsh_only_condition(),
            ..pattern_abbr("up{number}", "cd {number}", "../")
        };
        assert_eq!(pattern_table_lines(&cfg(vec![up])), "");
    }

    #[test]
    fn pattern_table_lines_handles_prefix_and_suffix() {
        // `g{number}p` → prefix="g", suffix="p"
        let c = cfg(vec![pattern_abbr("g{number}p", "git push -n {number}", "x")]);
        let s = pattern_table_lines(&c);
        assert_eq!(
            s,
            "    \"g\"$'\\037'\"p\"$'\\037'\"git push -n {number}\"$'\\037'\"x\"$'\\037'\"\""
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
            match_kind: None,
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
            match_kind: None,
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
            match_kind: None,
        };
        let s = pattern_table_lines(&cfg(vec![a]));
        assert_eq!(s, "");
    }

    #[test]
    fn pattern_table_lines_empty_for_empty_config() {
        assert_eq!(pattern_table_lines(&cfg(vec![])), "");
    }

    // ── command-position helper (issue #9) ───────────────────────────────

    /// The bake dispatcher must include a pure-bash command-position
    /// helper so the runtime check (issue #9 closeout of the 0.1.17
    /// trade-off) can run without spawning the runex binary from the
    /// `bind -x` handler.
    #[test]
    fn bake_includes_command_position_helper() {
        let c = cfg(vec![plain_abbr("gst", "git status")]);
        let s = generate_cygwin_dispatcher(&c);
        assert!(
            s.contains("__runex_cyg_is_command_position"),
            "bake dispatcher must define a pure-bash command-position helper: {s}"
        );
    }

    /// The command-position helper recognises every pipeline / list
    /// operator that the Rust `domain::hook::is_command_position`
    /// recognises. The patterns must appear in the helper body so a
    /// future regression that drops one of them surfaces here.
    #[test]
    fn bake_command_position_helper_recognizes_pipeline_ops() {
        let c = cfg(vec![plain_abbr("gst", "git status")]);
        let s = generate_cygwin_dispatcher(&c);
        for pat in [r#"*"&&""#, r#"*"||""#, r#"*"|""#, r#"*";""#] {
            assert!(
                s.contains(pat),
                "bake helper must match pipeline operator pattern {pat}: {s}"
            );
        }
    }

    /// `sudo` at the end of the prefix should mark command position
    /// (provided the part before `sudo` is itself command position).
    /// The bake helper must look at the last whitespace-separated
    /// word.
    #[test]
    fn bake_command_position_helper_recognizes_sudo() {
        let c = cfg(vec![plain_abbr("gst", "git status")]);
        let s = generate_cygwin_dispatcher(&c);
        assert!(
            s.contains(r#"if [ "$last_word" = "sudo" ]"#),
            "bake helper must check for trailing `sudo` word: {s}"
        );
    }

    /// The bake expand function must invoke the command-position
    /// helper before doing the abbreviation lookup. Without this the
    /// helper would be dead code and the 0.1.17 trade-off would
    /// silently survive.
    #[test]
    fn bake_expand_calls_command_position_before_lookup() {
        let c = cfg(vec![plain_abbr("gst", "git status")]);
        let s = generate_cygwin_dispatcher(&c);
        let expand_body = &s[s.find("__runex_cyg_expand()").expect("expand fn must exist")..];
        let cmd_pos_idx = expand_body
            .find("__runex_cyg_is_command_position")
            .expect("expand body must call the command-position helper");
        let lookup_idx = expand_body
            .find("__runex_cyg_lookup ")
            .expect("expand body must call lookup");
        assert!(
            cmd_pos_idx < lookup_idx,
            "command-position check must precede the abbreviation lookup; \
             cmd_pos at {cmd_pos_idx}, lookup at {lookup_idx}: {expand_body}"
        );
    }

    /// The 0.1.17 dispatcher computed `prefix="${left%$token}"`, which
    /// makes bash treat the token as a `%` glob — a `*` / `?` / `[` in
    /// the token would match unexpected portions of the left side and
    /// corrupt the prefix. Issue #9 switches to a substring slice
    /// (`${left:0:<len(left) - len(token)>}`) so the prefix is
    /// computed byte-for-byte, regardless of what characters the token
    /// happens to contain.
    #[test]
    fn bake_uses_substring_prefix_calc_not_glob_pattern() {
        let c = cfg(vec![plain_abbr("gst", "git status")]);
        let s = generate_cygwin_dispatcher(&c);
        assert!(
            !s.contains(r#"prefix="${left%$token}""#),
            "bake expand must not use `${{left%$token}}` glob pattern (issue #9): {s}"
        );
        assert!(
            s.contains("prefix=\"${left:0:$((${#left} - ${#token}))}\""),
            "bake expand must compute prefix via substring slice (issue #9): {s}"
        );
    }
}
