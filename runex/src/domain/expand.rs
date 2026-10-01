use serde::Serialize;

use std::cell::RefCell;
use std::time::Instant;

use crate::domain::model::{Abbr, Config, ExpandResult, MatchKind};
use crate::domain::shell::Shell;
use crate::domain::timings::{CommandExistsCall, Timings};

/// `{number}` placeholder marker (issue #1).
pub(crate) const NUMBER_PLACEHOLDER: &str = "{number}";

/// Placeholder in `expand` for the text a glob key's `*` matched
/// (ADR 0005).
pub(crate) const GLOB_CAPTURE_PLACEHOLDER: &str = "{*}";

/// [`GLOB_CAPTURE_PLACEHOLDER`] without its braces.
const GLOB_CAPTURE_NAME: &str = "*";

/// Upper bound on the value captured by `{number}`. Above this the
/// pattern simply fails to match — the user-visible effect is the
/// same as typing an unknown token. Picked to bound the rendered
/// length given a `MAX_NUMBER_UNIT_BYTES = 32` per-unit cap
/// (32 * 128 = 4096 = MAX_RENDERED_EXPAND_BYTES).
pub(crate) const MAX_NUMERIC_REPEAT: u32 = 128;

/// Hard ceiling on a rendered expansion. Matches the static
/// `MAX_EXPAND_BYTES = 4096` from config validation so a dynamic
/// repetition cannot exceed what a hand-written expansion could.
pub(crate) const MAX_RENDERED_EXPAND_BYTES: usize = 4_096;

/// Captures extracted from a token by [`match_rule`]: the digits of a
/// `{number}` key, the text a glob key's `*` matched, or the groups of
/// a regex key.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Bindings {
    pub number: Option<u32>,
    pub glob: Option<String>,
    /// Groups of a regex key in order, group 1 first.
    pub regex: Option<Vec<RegexGroup>>,
}

/// One capture group of a regex key. `text` is `""` when the group did
/// not take part in the match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegexGroup {
    pub name: Option<String>,
    pub text: String,
}

impl Bindings {
    pub(crate) fn empty() -> Self {
        Self::default()
    }
}

/// Matching phase of a rule. Phases run in declaration order and a rule
/// in an earlier phase always beats one in a later phase, whatever the
/// config order (ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Exact,
    Number,
    Glob,
    Regex,
}

const PHASES: [Phase; 4] = [Phase::Exact, Phase::Number, Phase::Glob, Phase::Regex];

fn phase_of(abbr: &Abbr) -> Phase {
    match abbr.match_kind {
        Some(MatchKind::Glob) => Phase::Glob,
        Some(MatchKind::Regex) => Phase::Regex,
        None if abbr.key.contains('{') => Phase::Number,
        None => Phase::Exact,
    }
}

/// Match one rule against a typed `token`, honouring its `match` field.
pub(crate) fn match_rule(abbr: &Abbr, token: &str) -> Option<Bindings> {
    match abbr.match_kind {
        Some(MatchKind::Glob) => {
            let captured = match_glob_key(&abbr.key, token)?;
            Some(Bindings { glob: Some(captured), ..Bindings::empty() })
        }
        Some(MatchKind::Regex) => {
            let groups = match_regex_key(&abbr.key, token)?;
            Some(Bindings { regex: Some(groups), ..Bindings::empty() })
        }
        None => match_abbr_key(&abbr.key, token),
    }
}

/// Upper bound on a compiled regex key. The config is re-read on every
/// key press, and regex-lite's default limit (10 MiB) bounds only the
/// NFA: a key under 1 KiB could still make each match allocate
/// gigabytes for its capture slots. Abbreviation patterns compile to a
/// few KiB.
const REGEX_SIZE_LIMIT: usize = 64 * 1024;

/// Upper bound on capture groups in a regex key; the capture slot table
/// grows with this number times the compiled size.
pub(crate) const MAX_REGEX_GROUPS: usize = 16;

fn build_regex(pattern: &str) -> Result<regex_lite::Regex, regex_lite::Error> {
    regex_lite::RegexBuilder::new(pattern).size_limit(REGEX_SIZE_LIMIT).build()
}

/// Compile a regex `key` so that it must match the whole token
/// (`^(?:key)$`). The key must already have passed
/// [`regex_key_group_count`].
fn compile_regex_key(key: &str) -> Result<regex_lite::Regex, regex_lite::Error> {
    build_regex(&format!("^(?:{key})$"))
}

/// Validate a regex `key` and return how many capture groups it has.
/// The key must compile on its own (a key such as `a)|(b` compiles only
/// once wrapped, and wrapping it would turn the anchors into
/// alternatives) and wrapped as [`compile_regex_key`] does (a `(?x)`
/// comment or the nesting limit can break only the wrapped form), within
/// [`REGEX_SIZE_LIMIT`] and [`MAX_REGEX_GROUPS`]. Config validation runs
/// this on every regex key, so the runtime compile cannot fail.
pub(crate) fn regex_key_group_count(key: &str) -> Result<usize, String> {
    let alone = build_regex(key).map_err(|error| error.to_string())?;
    compile_regex_key(key).map_err(|error| error.to_string())?;
    let groups = alone.captures_len() - 1;
    if groups > MAX_REGEX_GROUPS {
        return Err(format!("{groups} capture groups (at most {MAX_REGEX_GROUPS} are allowed)"));
    }
    Ok(groups)
}

/// Match a regex `key` against `token`. Returns every capture group,
/// or `None` when the token does not match or the key does not compile
/// (validation rejects the latter at load time).
fn match_regex_key(key: &str, token: &str) -> Option<Vec<RegexGroup>> {
    let regex = compile_regex_key(key).ok()?;
    let captures = regex.captures(token)?;
    let groups = regex
        .capture_names()
        .enumerate()
        .skip(1)
        .map(|(i, name)| RegexGroup {
            name: name.map(str::to_string),
            text: captures.get(i).map_or_else(String::new, |m| m.as_str().to_string()),
        })
        .collect();
    Some(groups)
}

/// The group number written as `{1}`..`{9}` in a regex rule's `expand`.
pub(crate) fn regex_group_number(placeholder_name: &str) -> Option<usize> {
    match placeholder_name.as_bytes() {
        [digit @ b'1'..=b'9'] => Some(usize::from(digit - b'0')),
        _ => None,
    }
}

/// Match a glob `key` against `token`, comparing characters rather
/// than bytes: `*` matches zero or more characters, `?` exactly one,
/// and validation guarantees at most one `*`. Returns the text the `*`
/// matched, or `""` when the key has no `*`.
fn match_glob_key(key: &str, token: &str) -> Option<String> {
    let token: Vec<char> = token.chars().collect();
    let (head, tail) = match key.split_once('*') {
        Some((head, tail)) => (head, Some(tail)),
        None => (key, None),
    };
    let head: Vec<char> = head.chars().collect();
    let Some(tail) = tail else {
        return glob_fixed_part_matches(&head, &token).then(String::new);
    };
    let tail: Vec<char> = tail.chars().collect();
    if token.len() < head.len() + tail.len() {
        return None;
    }
    let middle_end = token.len() - tail.len();
    let matched = glob_fixed_part_matches(&head, &token[..head.len()])
        && glob_fixed_part_matches(&tail, &token[middle_end..]);
    matched.then(|| token[head.len()..middle_end].iter().collect())
}

/// Equal-length comparison where `?` in `pattern` accepts any character.
fn glob_fixed_part_matches(pattern: &[char], text: &[char]) -> bool {
    pattern.len() == text.len() && pattern.iter().zip(text).all(|(p, t)| *p == '?' || p == t)
}

/// Try to match `key` (which may contain `{number}`) against a typed
/// `token`. Returns `Some(Bindings)` on a successful match, `None`
/// otherwise. Pure function; no I/O.
pub(crate) fn match_abbr_key(key: &str, token: &str) -> Option<Bindings> {
    // Fast path: no placeholder syntax → exact compare.
    if !key.contains('{') {
        return (key == token).then(Bindings::empty);
    }
    // Pattern path: split on the first (and validated-unique) `{number}`.
    let Some((prefix, suffix)) = split_once_number_placeholder(key) else {
        // Unrecognised placeholder in the key. Validation rejects this at
        // parse time; defensively fall back to literal compare here so a
        // hypothetical bypass cannot accidentally match arbitrary tokens.
        return (key == token).then(Bindings::empty);
    };
    let rest = token.strip_prefix(prefix)?.strip_suffix(suffix)?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = rest.parse().ok()?;
    if n == 0 || n > MAX_NUMERIC_REPEAT {
        return None;
    }
    Some(Bindings { number: Some(n), ..Bindings::empty() })
}

/// Split `key` at the single `{number}` placeholder. Returns `None`
/// when the key contains no `{number}` or when it contains some
/// other `{...}` token (validator must catch the latter).
fn split_once_number_placeholder(key: &str) -> Option<(&str, &str)> {
    let pos = key.find(NUMBER_PLACEHOLDER)?;
    let prefix = &key[..pos];
    let suffix = &key[pos + NUMBER_PLACEHOLDER.len()..];
    // Reject other placeholder syntax — only `{number}` is supported.
    if prefix.contains('{') || suffix.contains('{') {
        return None;
    }
    Some((prefix, suffix))
}

/// Render the text to insert and the cursor offset. The cursor
/// placeholder is located in the template before the glob or regex
/// captures are substituted, so a `{}` the user typed inside a capture
/// stays literal text instead of being taken for the placeholder.
fn render_with_cursor(abbr: &Abbr, shell: Shell, bindings: &Bindings) -> Option<(String, Option<usize>)> {
    let base = render_number(abbr, shell, bindings)?;
    let (text, cursor) = extract_cursor_placeholder(&base);
    let Some(pos) = cursor else {
        return Some((substitute_captures(&text, bindings)?, None));
    };
    let left = substitute_captures(&text[..pos], bindings)?;
    let right = substitute_captures(&text[pos..], bindings)?;
    let cursor = left.len();
    let joined = left + &right;
    (joined.len() <= MAX_RENDERED_EXPAND_BYTES).then_some((joined, Some(cursor)))
}

/// Replace the capture placeholders in `text`: `{*}` with the glob
/// capture, `{1}`..`{9}` and `{name}` with regex groups. A placeholder
/// the bindings cannot resolve stays literal. The final length is
/// computed first, so a template with many placeholders and a long
/// token is refused without building the oversized string.
fn substitute_captures(text: &str, bindings: &Bindings) -> Option<String> {
    let mut final_len: usize = 0;
    for_each_rendered_piece(text, bindings, |piece| final_len = final_len.saturating_add(piece.len()));
    if final_len > MAX_RENDERED_EXPAND_BYTES {
        return None;
    }
    let mut rendered = String::with_capacity(final_len);
    for_each_rendered_piece(text, bindings, |piece| rendered.push_str(piece));
    Some(rendered)
}

/// Feed `text` to `emit` piece by piece, scanning left to right, with
/// each `{...}` that [`resolve_capture`] knows replaced by its capture.
fn for_each_rendered_piece<'a>(text: &'a str, bindings: &'a Bindings, mut emit: impl FnMut(&'a str)) {
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after_open = &rest[open + 1..];
        let resolved = after_open
            .find('}')
            .and_then(|close| Some((close, resolve_capture(&after_open[..close], bindings)?)));
        match resolved {
            Some((close, capture)) => {
                emit(&rest[..open]);
                emit(capture);
                rest = &after_open[close + 1..];
            }
            None => {
                emit(&rest[..=open]);
                rest = after_open;
            }
        }
    }
    emit(rest);
}

/// The capture that the placeholder `{name}` stands for, or `None`
/// when the bindings provide no such capture.
fn resolve_capture<'a>(name: &str, bindings: &'a Bindings) -> Option<&'a str> {
    if name == GLOB_CAPTURE_NAME {
        return bindings.glob.as_deref();
    }
    let groups = bindings.regex.as_ref()?;
    let group = match regex_group_number(name) {
        Some(number) => groups.get(number - 1),
        None => groups.iter().find(|group| group.name.as_deref() == Some(name)),
    };
    group.map(|group| group.text.as_str())
}

/// Apply the `{number}` repetition to the template for `shell`.
fn render_number(abbr: &Abbr, shell: Shell, bindings: &Bindings) -> Option<String> {
    let template = abbr.expand.for_shell(shell)?;
    let rendered = match bindings.number {
        None => template.to_string(),
        Some(n) => {
            let unit = abbr.number.as_deref()?;
            let total_repeat = unit.len().checked_mul(n as usize)?;
            // Reject if the repeated unit alone already exceeds the cap;
            // the full template can only be larger.
            if total_repeat > MAX_RENDERED_EXPAND_BYTES {
                return None;
            }
            let repeated = unit.repeat(n as usize);
            let rendered = template.replace(NUMBER_PLACEHOLDER, &repeated);
            if rendered.len() > MAX_RENDERED_EXPAND_BYTES {
                return None;
            }
            rendered
        }
    };
    Some(rendered)
}

/// A single skipped rule — part of the `which_abbr` trace.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub(crate) enum SkipReason {
    /// key == expand (self-loop guard).
    SelfLoop,
    /// One or more `when_command_exists` commands were absent.
    ConditionFailed {
        found_commands: Vec<String>,
        missing_commands: Vec<String>,
    },
    /// No expand entry for this shell (and no default).
    NoShellEntry,
}

/// Result of a `which` lookup — mirrors `expand()` scan order exactly.
///
/// `skipped` contains every rule that matched the key but was bypassed,
/// in the same order `expand()` would skip them. This ensures `which_abbr`
/// and `expand` agree on the final outcome even with duplicate-key rules.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub(crate) enum WhichResult {
    /// Token matched a rule and all conditions passed.
    Expanded {
        key: String,
        expansion: String,
        rule_index: usize,
        /// Commands that were checked via `when_command_exists` and passed.
        satisfied_conditions: Vec<String>,
        /// Earlier rules with the same key that were skipped before this one.
        skipped: Vec<(usize, SkipReason)>,
    },
    /// Every matching rule was skipped; here is why each one was bypassed.
    AllSkipped {
        token: String,
        skipped: Vec<(usize, SkipReason)>,
    },
    /// No rule had this key at all.
    NoMatch { token: String },
}

/// Expand a token using the config.
///
/// `shell` selects the per-shell expand/when_command_exists entry.
/// `command_exists` is injected for testability (DI).
///
/// Matching runs in phases (issue #1, ADR 0005): exact rules, then
/// `{number}` rules, then glob rules, then regex rules (issue #20). Within each phase the config's
/// rule order is preserved (first match wins), so an exact rule always
/// beats a pattern that would also accept the token, even when the
/// pattern rule appears earlier in the config.
pub(crate) fn expand<F>(config: &Config, token: &str, shell: Shell, command_exists: F) -> ExpandResult
where
    F: Fn(&str) -> bool,
{
    for phase in PHASES {
        for abbr in config.abbr.iter().filter(|abbr| phase_of(abbr) == phase) {
            let Some(bindings) = match_rule(abbr, token) else {
                continue;
            };
            if let Some(result) = try_expand_rule(abbr, token, shell, &command_exists, &bindings) {
                return result;
            }
        }
    }
    ExpandResult::PassThrough(token.to_string())
}

/// Apply one rule with prepared bindings. Returns `Some(Expanded)` when
/// the rule fires, `None` to skip and continue scanning. Encapsulates
/// the shell-entry / self-loop / `when_command_exists` / render guard
/// chain shared by every phase.
fn try_expand_rule<F>(
    abbr: &Abbr,
    token: &str,
    shell: Shell,
    command_exists: &F,
    bindings: &Bindings,
) -> Option<ExpandResult>
where
    F: Fn(&str) -> bool,
{
    let template = abbr.expand.for_shell(shell)?;
    if is_exact_self_loop(abbr, template) {
        return None;
    }
    if let Some(cmds) = &abbr.when_command_exists {
        let list = cmds.for_shell(shell)?;
        if !list.iter().all(|c| command_exists(c)) {
            return None;
        }
    }
    let (text, cursor_offset) = render_with_cursor(abbr, shell, bindings)?;
    if is_pattern_self_loop(abbr, &text, token) {
        return None;
    }
    Some(ExpandResult::Expanded { text, cursor_offset })
}

/// An exact rule whose `expand` is its own `key` would rewrite the token
/// to itself; it is skipped before its conditions are evaluated, so
/// `which --why` reports it as a self-loop rather than a failed
/// condition. Glob and regex rules are checked after rendering instead
/// ([`is_pattern_self_loop`]), since their raw template never equals the
/// key. `{number}` rules and exact rules that only add a `{}` keep
/// expanding, as they always have.
fn is_exact_self_loop(abbr: &Abbr, template: &str) -> bool {
    phase_of(abbr) == Phase::Exact && abbr.key == template
}

/// A glob or regex rule whose rendered text equals the token would
/// rewrite the token to itself and hide later rules of its phase
/// (ADR 0005). One that renders to nothing would erase the token; it is
/// skipped too, which also keeps the Git Bash bake path (where an empty
/// result means "no match") in step.
fn is_pattern_self_loop(abbr: &Abbr, rendered_text: &str, token: &str) -> bool {
    matches!(phase_of(abbr), Phase::Glob | Phase::Regex) && (rendered_text == token || rendered_text.is_empty())
}

/// Extract cursor placeholder `{}` from expansion text.
/// Returns the text with `{}` removed and the byte offset where it was.
fn extract_cursor_placeholder(text: &str) -> (String, Option<usize>) {
    if let Some(pos) = text.find(crate::domain::model::CURSOR_PLACEHOLDER) {
        let mut result = String::with_capacity(text.len() - 2);
        result.push_str(&text[..pos]);
        result.push_str(&text[pos + 2..]);
        (result, Some(pos))
    } else {
        (text.to_string(), None)
    }
}

/// Like [`expand`], but records timing data into `timings`.
///
/// Each `command_exists` call is individually timed, and the overall expand
/// phase is recorded as a single phase entry.
pub(crate) fn expand_timed<F>(
    config: &Config,
    token: &str,
    shell: Shell,
    command_exists: F,
    timings: &mut Timings,
) -> ExpandResult
where
    F: Fn(&str) -> bool,
{
    let calls: RefCell<Vec<CommandExistsCall>> = RefCell::new(Vec::new());
    let timer = Instant::now();

    let timed_exists = |cmd: &str| -> bool {
        let t = Instant::now();
        let found = command_exists(cmd);
        let elapsed = t.elapsed();
        calls.borrow_mut().push(CommandExistsCall {
            command: cmd.to_string(),
            found,
            duration: elapsed,
            // Heuristic: if the lookup completed in under 100us, it was likely a cache hit.
            // A real which::which() call takes ~9ms on typical systems.
            cached: elapsed.as_micros() < 100,
        });
        found
    };

    let result = expand(config, token, shell, timed_exists);
    timings.record_phase("expand", timer.elapsed());

    for call in calls.into_inner() {
        timings.record_command_exists(&call.command, call.found, call.duration, call.cached);
    }

    result
}

/// Look up a token and return why it expands (or doesn't).
///
/// Scans rules in the same phase order as `expand()` (exact, then
/// `{number}`, then glob, then regex) so `which_abbr` always agrees with the final
/// outcome of `expand`, even when multiple rules match.
pub(crate) fn which_abbr<F>(config: &Config, token: &str, shell: Shell, command_exists: F) -> WhichResult
where
    F: Fn(&str) -> bool,
{
    let mut skipped: Vec<(usize, SkipReason)> = Vec::new();
    let mut any_key_matched = false;

    for phase in PHASES {
        let rules = config.abbr.iter().enumerate().filter(|(_, abbr)| phase_of(abbr) == phase);
        for (i, abbr) in rules {
            let Some(bindings) = match_rule(abbr, token) else {
                continue;
            };
            any_key_matched = true;
            match try_which_rule(abbr, token, shell, &command_exists, &bindings) {
                WhichOutcome::Hit { expansion, satisfied } => {
                    return WhichResult::Expanded {
                        key: abbr.key.clone(),
                        expansion,
                        rule_index: i,
                        satisfied_conditions: satisfied,
                        skipped,
                    };
                }
                WhichOutcome::Skip(reason) => skipped.push((i, reason)),
            }
        }
    }

    if any_key_matched {
        WhichResult::AllSkipped { token: token.to_string(), skipped }
    } else {
        WhichResult::NoMatch { token: token.to_string() }
    }
}

enum WhichOutcome {
    Hit { expansion: String, satisfied: Vec<String> },
    Skip(SkipReason),
}

fn try_which_rule<F>(
    abbr: &Abbr,
    token: &str,
    shell: Shell,
    command_exists: &F,
    bindings: &Bindings,
) -> WhichOutcome
where
    F: Fn(&str) -> bool,
{
    let Some(template) = abbr.expand.for_shell(shell) else {
        return WhichOutcome::Skip(SkipReason::NoShellEntry);
    };
    if is_exact_self_loop(abbr, template) {
        return WhichOutcome::Skip(SkipReason::SelfLoop);
    }
    let satisfied = if let Some(cmds) = &abbr.when_command_exists {
        match cmds.for_shell(shell) {
            None => return WhichOutcome::Skip(SkipReason::NoShellEntry),
            Some(list) => {
                let (found, missing): (Vec<String>, Vec<String>) =
                    list.iter().cloned().partition(|c| command_exists(c));
                if !missing.is_empty() {
                    return WhichOutcome::Skip(SkipReason::ConditionFailed {
                        found_commands: found,
                        missing_commands: missing,
                    });
                }
                list.to_vec()
            }
        }
    } else {
        Vec::new()
    };
    let Some((text, cursor)) = render_with_cursor(abbr, shell, bindings) else {
        // Render-time guard tripped (length cap, missing unit) — treat as
        // SelfLoop-equivalent skip for now. A dedicated SkipReason can be
        // added later if `which --why` needs to distinguish this case.
        return WhichOutcome::Skip(SkipReason::SelfLoop);
    };
    if is_pattern_self_loop(abbr, &text, token) {
        return WhichOutcome::Skip(SkipReason::SelfLoop);
    }
    WhichOutcome::Hit { expansion: with_cursor_marker(text, cursor), satisfied }
}

/// Put the cursor placeholder back into rendered text for display, so
/// `which` shows where the cursor lands.
fn with_cursor_marker(mut text: String, cursor: Option<usize>) -> String {
    if let Some(pos) = cursor {
        text.insert_str(pos, crate::domain::model::CURSOR_PLACEHOLDER);
    }
    text
}

/// List abbreviations as (key, expand) pairs.
///
/// When `shell` is `Some`, returns only rules that have an entry for that shell,
/// using the resolved expansion string.
/// When `shell` is `None`, uses the `All` value or the `default` field.
///
/// When `filter` is `Some(key)`, only rules whose key exactly matches are
/// returned — case-sensitive, no prefix / substring expansion (issue #2).
pub(crate) fn list<'a>(
    config: &'a Config,
    shell: Option<Shell>,
    filter: Option<&str>,
) -> Vec<(&'a str, String)> {
    config
        .abbr
        .iter()
        .filter(|a| filter.is_none_or(|f| a.key == f))
        .filter_map(|a| {
            let exp = match shell {
                Some(sh) => a.expand.for_shell(sh)?.to_string(),
                None => match &a.expand {
                    crate::domain::model::PerShellString::All(s) => s.clone(),
                    crate::domain::model::PerShellString::ByShell { default, .. } => {
                        default.as_deref()?.to_string()
                    }
                },
            };
            Some((a.key.as_str(), exp))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{Abbr, Config, PerShellCmds, PerShellString};

    fn cfg(abbrs: Vec<Abbr>) -> Config {
        Config {
            version: 1,
            keybind: crate::domain::model::KeybindConfig::default(),
            precache: crate::domain::model::PrecacheConfig::default(),
            abbr: abbrs,
        }
    }

    fn abbr(key: &str, expand: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(expand.into()),
            when_command_exists: None,
            number: None,
            match_kind: None,
        }
    }

    fn abbr_when(key: &str, exp: &str, cmds: Vec<&str>) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(exp.into()),
            when_command_exists: Some(PerShellCmds::All(
                cmds.into_iter().map(String::from).collect(),
            )),
            number: None,
            match_kind: None,
        }
    }

    fn abbr_pershell_expand(key: &str, expand: PerShellString) -> Abbr {
        Abbr {
            key: key.into(),
            expand,
            when_command_exists: None,
            number: None,
            match_kind: None,
        }
    }

    fn abbr_with_number(key: &str, expand: &str, unit: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(expand.into()),
            when_command_exists: None,
            number: Some(unit.into()),
            match_kind: None,
        }
    }

    // ── existing tests (updated signatures) ────────────────────────────────

    #[test]
    fn match_expands() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        assert_eq!(
            expand(&c, "gcm", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "git commit -m".into(), cursor_offset: None }
        );
    }

    #[test]
    fn no_match_passes_through() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        assert_eq!(
            expand(&c, "xyz", Shell::Bash, |_| true),
            ExpandResult::PassThrough("xyz".into())
        );
    }

    #[test]
    fn selects_correct_abbr() {
        let c = cfg(vec![abbr("gcm", "git commit -m"), abbr("gp", "git push")]);
        assert_eq!(
            expand(&c, "gp", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "git push".into(), cursor_offset: None }
        );
    }

    #[test]
    fn key_eq_expand_passes_through() {
        let c = cfg(vec![abbr("ls", "ls")]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| true),
            ExpandResult::PassThrough("ls".into())
        );
    }

    #[test]
    fn when_command_exists_present() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "lsd".into(), cursor_offset: None }
        );
    }

    #[test]
    fn when_command_exists_absent() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| false),
            ExpandResult::PassThrough("ls".into())
        );
    }

    #[test]
    fn duplicate_key_self_loop_then_real_expands() {
        let c = cfg(vec![abbr("ls", "ls"), abbr("ls", "lsd")]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "lsd".into(), cursor_offset: None }
        );
    }

    #[test]
    fn duplicate_key_failed_condition_then_real_expands() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"]), abbr("ls", "ls2")]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| false),
            ExpandResult::Expanded { text: "ls2".into(), cursor_offset: None }
        );
    }

    #[test]
    fn which_abbr_duplicate_self_loop_then_expanded() {
        let c = cfg(vec![abbr("ls", "ls"), abbr("ls", "lsd")]);
        let result = which_abbr(&c, "ls", Shell::Bash, |_| true);
        match result {
            WhichResult::Expanded { expansion, skipped, .. } => {
                assert_eq!(expansion, "lsd");
                assert_eq!(skipped.len(), 1);
                assert_eq!(skipped[0].0, 0);
                assert!(matches!(skipped[0].1, SkipReason::SelfLoop));
            }
            other => panic!("expected Expanded, got {other:?}"),
        }
    }

    #[test]
    fn which_abbr_all_skipped_returns_all_skipped() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let result = which_abbr(&c, "ls", Shell::Bash, |_| false);
        match result {
            WhichResult::AllSkipped { skipped, .. } => {
                assert_eq!(skipped.len(), 1);
                assert!(matches!(
                    &skipped[0].1,
                    SkipReason::ConditionFailed { missing_commands, .. }
                    if missing_commands == &["lsd"]
                ));
            }
            other => panic!("expected AllSkipped, got {other:?}"),
        }
    }

    #[test]
    fn which_abbr_no_match() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        assert!(matches!(
            which_abbr(&c, "xyz", Shell::Bash, |_| true),
            WhichResult::NoMatch { .. }
        ));
    }

    #[test]
    fn list_returns_all_pairs() {
        let c = cfg(vec![abbr("gcm", "git commit -m"), abbr("gp", "git push")]);
        let pairs = list(&c, None, None);
        assert_eq!(
            pairs,
            vec![("gcm", "git commit -m".to_string()), ("gp", "git push".to_string())]
        );
    }

    #[test]
    fn list_with_exact_filter_keeps_only_match() {
        let c = cfg(vec![
            abbr("ll", "ls -la"),
            abbr("ll.", "ls -laF"),
            abbr("gcm", "git commit -m"),
        ]);
        let pairs = list(&c, None, Some("ll"));
        assert_eq!(pairs, vec![("ll", "ls -la".to_string())]);
    }

    #[test]
    fn list_filter_no_match_returns_empty() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        let pairs = list(&c, None, Some("nope"));
        assert!(pairs.is_empty());
    }

    // ── per-shell expand tests ──────────────────────────────────────────────

    #[test]
    fn expand_per_shell_pwsh_uses_pwsh_expand() {
        // key="7z", default="7zip", pwsh="7z.exe" — no self-loop on any shell
        let c = cfg(vec![abbr_pershell_expand(
            "7z",
            PerShellString::ByShell {
                default: Some("7zip".into()),
                pwsh: Some("7z.exe".into()),
                bash: None, zsh: None, nu: None,
            },
        )]);
        assert_eq!(
            expand(&c, "7z", Shell::Pwsh, |_| true),
            ExpandResult::Expanded { text: "7z.exe".into(), cursor_offset: None }
        );
        assert_eq!(
            expand(&c, "7z", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "7zip".into(), cursor_offset: None }
        );
    }

    #[test]
    fn expand_per_shell_skips_when_no_shell_entry() {
        let c = cfg(vec![abbr_pershell_expand(
            "7z",
            PerShellString::ByShell {
                default: None,
                pwsh: Some("7z.exe".into()),
                bash: None, zsh: None, nu: None,
            },
        )]);
        // No entry for bash/default → pass-through
        assert_eq!(
            expand(&c, "7z", Shell::Bash, |_| true),
            ExpandResult::PassThrough("7z".into())
        );
        // pwsh has an entry → expands
        assert_eq!(
            expand(&c, "7z", Shell::Pwsh, |_| true),
            ExpandResult::Expanded { text: "7z.exe".into(), cursor_offset: None }
        );
    }

    #[test]
    fn which_abbr_no_shell_entry_is_skipped() {
        let c = cfg(vec![abbr_pershell_expand(
            "7z",
            PerShellString::ByShell {
                default: None,
                pwsh: Some("7z.exe".into()),
                bash: None, zsh: None, nu: None,
            },
        )]);
        let result = which_abbr(&c, "7z", Shell::Bash, |_| true);
        match result {
            WhichResult::AllSkipped { skipped, .. } => {
                assert_eq!(skipped.len(), 1);
                assert!(matches!(skipped[0].1, SkipReason::NoShellEntry));
            }
            other => panic!("expected AllSkipped, got {other:?}"),
        }
    }

    #[test]
    fn list_with_shell_filters_per_shell() {
        let c = cfg(vec![
            abbr_pershell_expand(
                "7z",
                PerShellString::ByShell {
                    default: Some("7zip".into()),
                    pwsh: Some("7z.exe".into()),
                    bash: None, zsh: None, nu: None,
                },
            ),
            abbr_pershell_expand(
                "pwsh-only",
                PerShellString::ByShell {
                    default: None,
                    pwsh: Some("pwsh-cmd".into()),
                    bash: None, zsh: None, nu: None,
                },
            ),
        ]);
        let bash_list = list(&c, Some(Shell::Bash), None);
        // "7z" has default so shows; "pwsh-only" has no bash/default → filtered out
        assert_eq!(bash_list, vec![("7z", "7zip".to_string())]);

        let pwsh_list = list(&c, Some(Shell::Pwsh), None);
        assert_eq!(
            pwsh_list,
            vec![
                ("7z", "7z.exe".to_string()),
                ("pwsh-only", "pwsh-cmd".to_string()),
            ]
        );
    }

    // ── expand_timed tests ──────────────────────────────────────────────

    #[test]
    fn expand_timed_same_result_as_expand() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let mut timings = crate::domain::timings::Timings::new();
        let result = expand_timed(&c, "ls", Shell::Bash, |_| true, &mut timings);
        assert_eq!(result, ExpandResult::Expanded { text: "lsd".into(), cursor_offset: None });
    }

    #[test]
    fn expand_timed_records_command_exists_calls() {
        let c = cfg(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let mut timings = crate::domain::timings::Timings::new();
        expand_timed(&c, "ls", Shell::Bash, |_| true, &mut timings);
        let calls = timings.command_exists_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].command, "lsd");
        assert!(calls[0].found);
    }

    #[test]
    fn expand_timed_records_expand_phase() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        let mut timings = crate::domain::timings::Timings::new();
        expand_timed(&c, "gcm", Shell::Bash, |_| true, &mut timings);
        let phases = timings.phases();
        assert!(
            phases.iter().any(|p| p.name == "expand"),
            "must record an 'expand' phase, got: {:?}",
            phases.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }

    // ── cursor placeholder tests ────────────────────────────────────────

    #[test]
    fn expand_with_cursor_placeholder() {
        let c = cfg(vec![abbr("gcam", "git commit -am '{}'")] );
        let result = expand(&c, "gcam", Shell::Bash, |_| true);
        assert_eq!(
            result,
            ExpandResult::Expanded {
                text: "git commit -am ''".into(),
                cursor_offset: Some(16), // position between the quotes
            }
        );
    }

    #[test]
    fn expand_without_cursor_placeholder() {
        let c = cfg(vec![abbr("gcm", "git commit -m")]);
        let result = expand(&c, "gcm", Shell::Bash, |_| true);
        assert_eq!(
            result,
            ExpandResult::Expanded { text: "git commit -m".into(), cursor_offset: None }
        );
    }

    #[test]
    fn extract_cursor_placeholder_found() {
        let (text, offset) = extract_cursor_placeholder("git commit -am '{}'");
        assert_eq!(text, "git commit -am ''");
        assert_eq!(offset, Some(16));
    }

    #[test]
    fn extract_cursor_placeholder_not_found() {
        let (text, offset) = extract_cursor_placeholder("git commit -m");
        assert_eq!(text, "git commit -m");
        assert_eq!(offset, None);
    }

    #[test]
    fn extract_cursor_placeholder_at_end() {
        let (text, offset) = extract_cursor_placeholder("echo {}");
        assert_eq!(text, "echo ");
        assert_eq!(offset, Some(5));
    }

    // ── {number} placeholder (issue #1) ────────────────────────────────────

    #[test]
    fn match_abbr_key_exact_no_braces_matches_only_exact() {
        assert_eq!(match_abbr_key("up", "up"), Some(Bindings::empty()));
        assert_eq!(match_abbr_key("up", "up3"), None);
    }

    #[test]
    fn match_abbr_key_pattern_captures_3() {
        assert_eq!(
            match_abbr_key("up{number}", "up3"),
            Some(Bindings { number: Some(3), glob: None, regex: None })
        );
    }

    #[test]
    fn match_abbr_key_pattern_captures_10() {
        assert_eq!(
            match_abbr_key("up{number}", "up10"),
            Some(Bindings { number: Some(10), glob: None, regex: None })
        );
    }

    #[test]
    fn match_abbr_key_pattern_rejects_bare_up() {
        // No digits → pattern miss; the exact `up` rule (if any) must handle it.
        assert_eq!(match_abbr_key("up{number}", "up"), None);
    }

    #[test]
    fn match_abbr_key_pattern_rejects_zero() {
        assert_eq!(match_abbr_key("up{number}", "up0"), None);
    }

    #[test]
    fn match_abbr_key_pattern_rejects_above_max() {
        // 129 > MAX_NUMERIC_REPEAT (128).
        assert_eq!(match_abbr_key("up{number}", "up129"), None);
        // 128 still matches.
        assert_eq!(
            match_abbr_key("up{number}", "up128"),
            Some(Bindings { number: Some(128), glob: None, regex: None })
        );
    }

    #[test]
    fn match_abbr_key_pattern_with_suffix() {
        assert_eq!(
            match_abbr_key("x{number}y", "x3y"),
            Some(Bindings { number: Some(3), glob: None, regex: None })
        );
        assert_eq!(match_abbr_key("x{number}y", "x3z"), None);
        assert_eq!(match_abbr_key("x{number}y", "x3"), None);
    }

    #[test]
    fn match_abbr_key_pattern_rejects_non_ascii_digits() {
        // Full-width digits are not ASCII decimals.
        assert_eq!(match_abbr_key("up{number}", "up３"), None);
    }

    #[test]
    fn match_abbr_key_pattern_rejects_negative_or_sign() {
        assert_eq!(match_abbr_key("up{number}", "up-3"), None);
        assert_eq!(match_abbr_key("up{number}", "up+3"), None);
    }

    #[test]
    fn match_abbr_key_unknown_placeholder_falls_back_to_exact() {
        // Defensive: `{foo}` is not recognised, so it must NOT match `upX`.
        // Validation rejects this shape at parse time; the runtime
        // fallback still has to be safe.
        assert_eq!(match_abbr_key("up{foo}", "upX"), None);
        // Literal compare path: only the exact literal key matches.
        assert_eq!(
            match_abbr_key("up{foo}", "up{foo}"),
            Some(Bindings::empty())
        );
    }

    #[test]
    fn render_expansion_repeats_unit_three_times() {
        let a = abbr_with_number("up{number}", "cd {number}", "../");
        let out = render_number(&a, Shell::Bash, &Bindings { number: Some(3), glob: None, regex: None });
        assert_eq!(out.as_deref(), Some("cd ../../../"));
    }

    #[test]
    fn render_expansion_rejects_when_total_repeat_exceeds_cap() {
        // unit = 50 bytes, n = 128 → 6400 > 4096
        let a = abbr_with_number("u{number}", "{number}", &"X".repeat(50));
        let out = render_number(&a, Shell::Bash, &Bindings { number: Some(128), glob: None, regex: None });
        assert_eq!(out, None);
    }

    #[test]
    fn render_expansion_without_bindings_returns_template() {
        let a = abbr("gcm", "git commit -m");
        let out = render_number(&a, Shell::Bash, &Bindings::empty());
        assert_eq!(out.as_deref(), Some("git commit -m"));
    }

    #[test]
    fn render_expansion_missing_unit_returns_none() {
        // key has {number} but `number = ...` is absent. Validation should
        // catch this at parse; the runtime is defensive.
        let mut a = abbr("up{number}", "cd {number}");
        a.number = None;
        let out = render_number(&a, Shell::Bash, &Bindings { number: Some(3), glob: None, regex: None });
        assert_eq!(out, None);
    }

    #[test]
    fn expand_prefers_exact_over_pattern_for_same_token() {
        let c = cfg(vec![
            // Pattern rule appears first in config order; exact must still win.
            abbr_with_number("up{number}", "cd {number}", "../"),
            abbr("up2", "cd ../../EXACT"),
        ]);
        assert_eq!(
            expand(&c, "up2", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "cd ../../EXACT".into(), cursor_offset: None }
        );
    }

    #[test]
    fn expand_pattern_used_when_no_exact_match() {
        let c = cfg(vec![
            abbr_with_number("up{number}", "cd {number}", "../"),
            abbr("up2", "cd ../../EXACT"),
        ]);
        assert_eq!(
            expand(&c, "up3", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "cd ../../../".into(), cursor_offset: None }
        );
    }

    #[test]
    fn expand_passes_through_bare_when_only_pattern_defined() {
        // `up` has no exact rule and `up{number}` requires digits.
        let c = cfg(vec![abbr_with_number("up{number}", "cd {number}", "../")]);
        assert_eq!(
            expand(&c, "up", Shell::Bash, |_| true),
            ExpandResult::PassThrough("up".into())
        );
    }

    #[test]
    fn expand_passes_through_above_max_repeat() {
        let c = cfg(vec![abbr_with_number("up{number}", "cd {number}", "../")]);
        assert_eq!(
            expand(&c, "up129", Shell::Bash, |_| true),
            ExpandResult::PassThrough("up129".into())
        );
    }

    #[test]
    fn expand_passes_through_non_digit_token() {
        let c = cfg(vec![abbr_with_number("up{number}", "cd {number}", "../")]);
        assert_eq!(
            expand(&c, "upx", Shell::Bash, |_| true),
            ExpandResult::PassThrough("upx".into())
        );
    }

    #[test]
    fn expand_number_placeholder_coexists_with_cursor_placeholder() {
        // {number} substituted first, then {} cursor stripped at the end.
        let a = Abbr {
            key: "wrap{number}".into(),
            expand: PerShellString::All("echo '{number}' '{}'".into()),
            when_command_exists: None,
            number: Some("X".into()),
            match_kind: None,
        };
        let c = cfg(vec![a]);
        // wrap3 → echo 'XXX' '{}' → echo 'XXX' '' with cursor at offset 12
        assert_eq!(
            expand(&c, "wrap3", Shell::Bash, |_| true),
            ExpandResult::Expanded {
                text: "echo 'XXX' ''".into(),
                cursor_offset: Some(12),
            }
        );
    }

    #[test]
    fn which_abbr_pattern_match_returns_expanded() {
        let c = cfg(vec![abbr_with_number("up{number}", "cd {number}", "../")]);
        let result = which_abbr(&c, "up3", Shell::Bash, |_| true);
        match result {
            WhichResult::Expanded { key, expansion, .. } => {
                assert_eq!(key, "up{number}");
                assert_eq!(expansion, "cd ../../../");
            }
            other => panic!("expected Expanded, got {other:?}"),
        }
    }

    #[test]
    fn which_abbr_exact_wins_over_pattern() {
        let c = cfg(vec![
            abbr_with_number("up{number}", "cd {number}", "../"),
            abbr("up2", "cd ../../EXACT"),
        ]);
        let result = which_abbr(&c, "up2", Shell::Bash, |_| true);
        match result {
            WhichResult::Expanded { key, expansion, .. } => {
                assert_eq!(key, "up2");
                assert_eq!(expansion, "cd ../../EXACT");
            }
            other => panic!("expected Expanded, got {other:?}"),
        }
    }

    // ── match = "glob" (issue #19, ADR 0005) ───────────────────────────────

    fn abbr_glob(key: &str, expand: &str) -> Abbr {
        Abbr {
            match_kind: Some(crate::domain::model::MatchKind::Glob),
            ..abbr(key, expand)
        }
    }

    fn expanded(text: &str) -> ExpandResult {
        ExpandResult::Expanded { text: text.into(), cursor_offset: None }
    }

    #[test]
    fn glob_star_captures_the_rest_of_the_token_into_expand() {
        let c = cfg(vec![abbr_glob("g*", "git {*}")]);
        assert_eq!(expand(&c, "gco", Shell::Bash, |_| true), expanded("git co"));
    }

    #[test]
    fn glob_star_matches_zero_characters() {
        let c = cfg(vec![abbr_glob("k*", "kubectl{*}")]);
        assert_eq!(expand(&c, "k", Shell::Bash, |_| true), expanded("kubectl"));
    }

    #[test]
    fn glob_star_in_the_middle_captures_between_prefix_and_suffix() {
        let c = cfg(vec![abbr_glob("d*x", "docker {*}")]);
        assert_eq!(expand(&c, "dpsx", Shell::Bash, |_| true), expanded("docker ps"));
        assert_eq!(
            expand(&c, "dps", Shell::Bash, |_| true),
            ExpandResult::PassThrough("dps".into())
        );
    }

    #[test]
    fn glob_question_mark_matches_exactly_one_character() {
        let c = cfg(vec![abbr_glob("k?", "kubectl")]);
        assert_eq!(expand(&c, "kg", Shell::Bash, |_| true), expanded("kubectl"));
        assert_eq!(expand(&c, "k", Shell::Bash, |_| true), ExpandResult::PassThrough("k".into()));
        assert_eq!(expand(&c, "kgp", Shell::Bash, |_| true), ExpandResult::PassThrough("kgp".into()));
    }

    #[test]
    fn glob_question_mark_counts_characters_not_bytes() {
        let c = cfg(vec![abbr_glob("?x", "matched")]);
        assert_eq!(expand(&c, "äx", Shell::Bash, |_| true), expanded("matched"));
    }

    #[test]
    fn glob_star_capture_keeps_multibyte_characters_whole() {
        let c = cfg(vec![abbr_glob("ä*", "[{*}]")]);
        assert_eq!(expand(&c, "äöü", Shell::Bash, |_| true), expanded("[öü]"));
    }

    #[test]
    fn key_with_star_but_no_match_field_stays_literal() {
        let c = cfg(vec![abbr("g*", "git")]);
        assert_eq!(expand(&c, "gco", Shell::Bash, |_| true), ExpandResult::PassThrough("gco".into()));
        assert_eq!(expand(&c, "g*", Shell::Bash, |_| true), expanded("git"));
    }

    #[test]
    fn exact_rule_beats_an_earlier_glob_rule() {
        let c = cfg(vec![abbr_glob("g*", "git {*}"), abbr("gst", "git status")]);
        assert_eq!(expand(&c, "gst", Shell::Bash, |_| true), expanded("git status"));
    }

    #[test]
    fn number_rule_beats_an_earlier_glob_rule() {
        let c = cfg(vec![
            abbr_glob("u*", "glob {*}"),
            abbr_with_number("up{number}", "cd {number}", "../"),
        ]);
        assert_eq!(expand(&c, "up2", Shell::Bash, |_| true), expanded("cd ../../"));
    }

    #[test]
    fn first_glob_rule_in_config_order_wins_among_globs() {
        let c = cfg(vec![abbr_glob("g*", "first {*}"), abbr_glob("gi*", "second {*}")]);
        assert_eq!(expand(&c, "git", Shell::Bash, |_| true), expanded("first it"));
    }

    #[test]
    fn glob_rule_whose_expansion_equals_the_token_is_skipped() {
        let c = cfg(vec![abbr_glob("g*", "g{*}"), abbr_glob("?x", "fallback")]);
        assert_eq!(expand(&c, "gx", Shell::Bash, |_| true), expanded("fallback"));
    }

    #[test]
    fn glob_rule_respects_when_command_exists() {
        let c = cfg(vec![Abbr {
            match_kind: Some(crate::domain::model::MatchKind::Glob),
            ..abbr_when("g*", "git {*}", vec!["git"])
        }]);
        assert_eq!(expand(&c, "gco", Shell::Bash, |_| false), ExpandResult::PassThrough("gco".into()));
        assert_eq!(expand(&c, "gco", Shell::Bash, |_| true), expanded("git co"));
    }

    #[test]
    fn glob_capture_combines_with_the_cursor_placeholder() {
        let c = cfg(vec![abbr_glob("m*", "git commit -m '{*}{}'")]);
        assert_eq!(
            expand(&c, "mfix", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "git commit -m 'fix'".into(), cursor_offset: Some(18) }
        );
    }

    #[test]
    fn which_reports_the_glob_rule_and_its_rendered_expansion() {
        let c = cfg(vec![abbr("x", "y"), abbr_glob("g*", "git {*}")]);
        match which_abbr(&c, "gco", Shell::Bash, |_| true) {
            WhichResult::Expanded { key, expansion, rule_index, .. } => {
                assert_eq!(key, "g*");
                assert_eq!(expansion, "git co");
                assert_eq!(rule_index, 1);
            }
            other => panic!("expected Expanded, got {other:?}"),
        }
    }

    #[test]
    fn which_reports_a_self_looping_glob_rule_as_skipped() {
        let c = cfg(vec![abbr_glob("g*", "g{*}")]);
        match which_abbr(&c, "gx", Shell::Bash, |_| true) {
            WhichResult::AllSkipped { skipped, .. } => {
                assert_eq!(skipped.len(), 1);
                assert!(matches!(skipped[0].1, SkipReason::SelfLoop));
            }
            other => panic!("expected AllSkipped, got {other:?}"),
        }
    }

    // ── regressions found in review of #46 ─────────────────────────────────

    #[test]
    fn exact_rule_that_only_adds_a_cursor_placeholder_still_expands() {
        let c = cfg(vec![abbr("ls", "ls{}"), abbr("ls", "lsd")]);
        assert_eq!(
            expand(&c, "ls", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "ls".into(), cursor_offset: Some(2) }
        );
    }

    #[test]
    fn number_rule_rendering_equal_to_the_token_still_expands() {
        let c = cfg(vec![abbr_with_number("x{number}", "x{number}", "1"), abbr("x1", "never")]);
        assert_eq!(expand(&c, "x1", Shell::Bash, |_| true), expanded("never"));
        let only_number = cfg(vec![abbr_with_number("x{number}", "x{number}", "1")]);
        assert_eq!(expand(&only_number, "x1", Shell::Bash, |_| true), expanded("x1"));
    }

    #[test]
    fn glob_capture_containing_braces_does_not_move_the_cursor_placeholder() {
        let c = cfg(vec![abbr_glob("m*", "git commit -m '{*}{}'")]);
        assert_eq!(
            expand(&c, "mA{}B", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "git commit -m 'A{}B'".into(), cursor_offset: Some(19) }
        );
    }

    #[test]
    fn glob_capture_containing_braces_is_inserted_literally_without_a_placeholder() {
        let c = cfg(vec![abbr_glob("k*", "kubectl {*}")]);
        assert_eq!(expand(&c, "k{}x", Shell::Bash, |_| true), expanded("kubectl {}x"));
    }

    #[test]
    fn glob_render_over_the_cap_passes_through() {
        let c = cfg(vec![abbr_glob("k*", &"{*}".repeat(100))]);
        let token = format!("k{}", "x".repeat(50));
        assert_eq!(expand(&c, &token, Shell::Bash, |_| true), ExpandResult::PassThrough(token.clone()));
    }

    #[test]
    fn glob_rule_that_renders_to_nothing_is_skipped() {
        let c = cfg(vec![abbr_glob("e*", "{}{*}")]);
        assert_eq!(expand(&c, "e", Shell::Bash, |_| true), ExpandResult::PassThrough("e".into()));
    }

    #[test]
    fn glob_rule_without_a_condition_for_this_shell_is_skipped() {
        let rule = Abbr {
            match_kind: Some(crate::domain::model::MatchKind::Glob),
            when_command_exists: Some(PerShellCmds::ByShell {
                default: None,
                bash: None,
                zsh: None,
                pwsh: Some(vec!["git".into()]),
                nu: None,
            }),
            ..abbr("p*", "pwshonly {*}")
        };
        let c = cfg(vec![rule]);
        assert_eq!(expand(&c, "px", Shell::Bash, |_| true), ExpandResult::PassThrough("px".into()));
    }

    /// `which` must reach the same verdict as `expand` at the length cap;
    /// the cursor placeholder is not part of the inserted text.
    #[test]
    fn which_agrees_with_expand_at_the_length_cap() {
        let template = format!("{}{{*}}{{}}", "a".repeat(3073));
        let c = cfg(vec![abbr_glob("z*", &template)]);
        let token = format!("z{}", "b".repeat(1023));
        assert!(matches!(expand(&c, &token, Shell::Bash, |_| true), ExpandResult::Expanded { .. }));
        assert!(
            matches!(which_abbr(&c, &token, Shell::Bash, |_| true), WhichResult::Expanded { .. }),
            "which must not report a rule that expand fires as skipped"
        );
    }

    // ── match = "regex" (issue #20) ────────────────────────────────────────

    fn abbr_regex(key: &str, expand: &str) -> Abbr {
        Abbr {
            match_kind: Some(crate::domain::model::MatchKind::Regex),
            ..abbr(key, expand)
        }
    }

    fn passes(token: &str) -> ExpandResult {
        ExpandResult::PassThrough(token.into())
    }

    #[test]
    fn regex_numbered_group_is_substituted_into_expand() {
        let c = cfg(vec![abbr_regex(r"k(\w+)", "kubectl {1}")]);
        assert_eq!(expand(&c, "kgp", Shell::Bash, |_| true), expanded("kubectl gp"));
    }

    #[test]
    fn regex_must_match_the_whole_token() {
        let c = cfg(vec![abbr_regex(r"k(\w+)", "kubectl {1}")]);
        assert_eq!(expand(&c, "xkgp", Shell::Bash, |_| true), passes("xkgp"));
    }

    #[test]
    fn regex_named_group_is_substituted_by_name() {
        let c = cfg(vec![abbr_regex("g(?P<rest>.+)", "git {rest}")]);
        assert_eq!(expand(&c, "gco", Shell::Bash, |_| true), expanded("git co"));
    }

    #[test]
    fn regex_counted_repetition_is_anchored_at_the_end() {
        let c = cfg(vec![abbr_regex("d([a-z]{2})", "docker {1}")]);
        assert_eq!(expand(&c, "dps", Shell::Bash, |_| true), expanded("docker ps"));
        assert_eq!(expand(&c, "dpsx", Shell::Bash, |_| true), passes("dpsx"));
    }

    #[test]
    fn regex_group_that_did_not_participate_renders_empty() {
        let c = cfg(vec![abbr_regex("a(x)?b", "[{1}]")]);
        assert_eq!(expand(&c, "ab", Shell::Bash, |_| true), expanded("[]"));
    }

    #[test]
    fn regex_unknown_name_placeholder_stays_literal() {
        let c = cfg(vec![abbr_regex(r"k(\w+)", "awk '{print}' {1}")]);
        assert_eq!(expand(&c, "kz", Shell::Bash, |_| true), expanded("awk '{print}' z"));
    }

    #[test]
    fn regex_capture_containing_braces_does_not_move_the_cursor_placeholder() {
        let c = cfg(vec![abbr_regex("m(.+)", "git commit -m '{1}{}'")]);
        assert_eq!(
            expand(&c, "mA{}B", Shell::Bash, |_| true),
            ExpandResult::Expanded { text: "git commit -m 'A{}B'".into(), cursor_offset: Some(19) }
        );
    }

    #[test]
    fn exact_rule_beats_an_earlier_regex_rule() {
        let c = cfg(vec![abbr_regex(r"k(\w+)", "regex {1}"), abbr("kgp", "exact")]);
        assert_eq!(expand(&c, "kgp", Shell::Bash, |_| true), expanded("exact"));
    }

    #[test]
    fn glob_rule_beats_an_earlier_regex_rule() {
        let c = cfg(vec![abbr_regex(r"k(\w+)", "regex {1}"), abbr_glob("k*", "glob {*}")]);
        assert_eq!(expand(&c, "kgp", Shell::Bash, |_| true), expanded("glob gp"));
    }

    #[test]
    fn regex_rule_whose_expansion_equals_the_token_is_skipped() {
        let c = cfg(vec![abbr_regex("x(.*)", "x{1}")]);
        assert_eq!(expand(&c, "xa", Shell::Bash, |_| true), passes("xa"));
    }

    #[test]
    fn regex_rule_that_renders_to_nothing_is_skipped_for_the_next_one() {
        let c = cfg(vec![abbr_regex("e(.*)", "{1}{}"), abbr_regex("e(.*)", "E2 {1}")]);
        assert_eq!(expand(&c, "e", Shell::Bash, |_| true), expanded("E2 "));
    }

    #[test]
    fn regex_rule_respects_when_command_exists() {
        let c = cfg(vec![Abbr {
            match_kind: Some(crate::domain::model::MatchKind::Regex),
            ..abbr_when(r"k(\w+)", "kubectl {1}", vec!["git"])
        }]);
        assert_eq!(expand(&c, "kgp", Shell::Bash, |_| false), passes("kgp"));
        assert_eq!(expand(&c, "kgp", Shell::Bash, |_| true), expanded("kubectl gp"));
    }

    #[test]
    fn regex_render_over_the_cap_passes_through() {
        let c = cfg(vec![abbr_regex("l(.*)", &"{1}".repeat(5))]);
        let token = format!("l{}", "a".repeat(900));
        assert_eq!(expand(&c, &token, Shell::Bash, |_| true), passes(&token));
        let at_cap = format!("l{}", "a".repeat(819));
        assert!(matches!(expand(&c, &at_cap, Shell::Bash, |_| true), ExpandResult::Expanded { .. }));
    }

    /// Wrapped as `^(?:a)|(b)$`, this key would compile and match any
    /// token starting with `a`, so the standalone check must refuse it.
    #[test]
    fn regex_key_that_only_compiles_once_wrapped_is_refused() {
        assert!(regex_key_group_count("a)|(b").is_err());
        assert_eq!(regex_key_group_count(r"k(\w+)(?P<x>.)?").ok(), Some(2));
    }

    #[test]
    fn which_reports_the_regex_rule_and_its_rendered_expansion() {
        let c = cfg(vec![abbr("x", "y"), abbr_regex(r"k(\w+)", "kubectl {1}")]);
        match which_abbr(&c, "kgp", Shell::Bash, |_| true) {
            WhichResult::Expanded { key, expansion, rule_index, .. } => {
                assert_eq!(key, r"k(\w+)");
                assert_eq!(expansion, "kubectl gp");
                assert_eq!(rule_index, 1);
            }
            other => panic!("expected Expanded, got {other:?}"),
        }
    }
}
