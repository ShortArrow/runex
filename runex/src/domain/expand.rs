use serde::Serialize;

use std::cell::RefCell;
use std::time::Instant;

use crate::domain::model::{Abbr, Config, ExpandResult};
use crate::domain::shell::Shell;
use crate::domain::timings::{CommandExistsCall, Timings};

/// `{number}` placeholder marker (issue #1).
pub(crate) const NUMBER_PLACEHOLDER: &str = "{number}";

/// Upper bound on the value captured by `{number}`. Above this the
/// pattern simply fails to match — the user-visible effect is the
/// same as typing an unknown token. Picked to bound the rendered
/// length given a `MAX_NUMBER_UNIT_BYTES = 32` per-unit cap
/// (32 * 128 = 4096 = MAX_RENDERED_EXPAND_BYTES).
pub(crate) const MAX_NUMERIC_REPEAT: u32 = 128;

/// Hard ceiling on `render_expansion` output. Matches the static
/// `MAX_EXPAND_BYTES = 4096` from config validation so a dynamic
/// repetition cannot exceed what a hand-written expansion could.
pub(crate) const MAX_RENDERED_EXPAND_BYTES: usize = 4_096;

/// Captures extracted from a token by `match_abbr_key`. Currently
/// only `{number}` is supported; the struct exists so future
/// `{string}` / `{path}` placeholders can land without rewiring
/// every caller.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Bindings {
    pub number: Option<u32>,
}

impl Bindings {
    pub(crate) fn empty() -> Self {
        Self { number: None }
    }
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
    Some(Bindings { number: Some(n) })
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

/// Render `abbr.expand` into a final string given the bindings
/// captured from the token. Returns `None` when the rendered output
/// would exceed `MAX_RENDERED_EXPAND_BYTES` or when a required
/// binding has no corresponding unit (the validator catches that
/// shape at parse time; this is a defensive `None`).
pub(crate) fn render_expansion(
    abbr: &Abbr,
    shell: Shell,
    bindings: &Bindings,
) -> Option<String> {
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
/// Matching is two-phase (issue #1): exact rules first, then
/// `{number}`-pattern rules. Within each phase the config's rule
/// order is preserved (first match wins). This makes exact rules
/// always beat a pattern that would also accept the token, even
/// when the pattern rule appears earlier in the config.
pub(crate) fn expand<F>(config: &Config, token: &str, shell: Shell, command_exists: F) -> ExpandResult
where
    F: Fn(&str) -> bool,
{
    // Phase 1: exact rules.
    for abbr in &config.abbr {
        if abbr.key.contains('{') || abbr.key != token {
            continue;
        }
        if let Some(result) = try_expand_rule(abbr, token, shell, &command_exists, &Bindings::empty()) {
            return result;
        }
    }
    // Phase 2: pattern rules (key contains a `{...}` placeholder).
    for abbr in &config.abbr {
        if !abbr.key.contains('{') {
            continue;
        }
        let Some(bindings) = match_abbr_key(&abbr.key, token) else {
            continue;
        };
        if let Some(result) = try_expand_rule(abbr, token, shell, &command_exists, &bindings) {
            return result;
        }
    }
    ExpandResult::PassThrough(token.to_string())
}

/// Apply one rule with prepared bindings. Returns `Some(Expanded)` when
/// the rule fires, `None` to skip and continue scanning. Encapsulates
/// the shell-entry / self-loop / `when_command_exists` / render guard
/// chain shared by both phases.
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
    let _ = token; // reserved for future skip-reason plumbing
    let _ = abbr.expand.for_shell(shell)?; // no entry for this shell → skip
    // Self-loop guard: only meaningful for exact rules (`bindings.number`
    // is `None`). A pattern key like `up{number}` cannot equal its raw
    // template `cd {number}`, so the check is moot in that path.
    if bindings.number.is_none()
        && abbr.key == abbr.expand.for_shell(shell).unwrap_or("")
    {
        return None;
    }
    if let Some(cmds) = &abbr.when_command_exists {
        let list = cmds.for_shell(shell)?;
        if !list.iter().all(|c| command_exists(c)) {
            return None;
        }
    }
    let rendered = render_expansion(abbr, shell, bindings)?;
    let (text, cursor_offset) = extract_cursor_placeholder(&rendered);
    Some(ExpandResult::Expanded { text, cursor_offset })
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
/// Scans rules in the same two-phase order as `expand()` (exact rules
/// first, then `{number}`-pattern rules) so `which_abbr` always agrees
/// with the final outcome of `expand`, even when multiple rules match.
pub(crate) fn which_abbr<F>(config: &Config, token: &str, shell: Shell, command_exists: F) -> WhichResult
where
    F: Fn(&str) -> bool,
{
    let mut skipped: Vec<(usize, SkipReason)> = Vec::new();
    let mut any_key_matched = false;

    // Phase 1: exact rules.
    for (i, abbr) in config.abbr.iter().enumerate() {
        if abbr.key.contains('{') {
            continue;
        }
        if abbr.key != token {
            continue;
        }
        any_key_matched = true;
        match try_which_rule(abbr, shell, &command_exists, &Bindings::empty()) {
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
    // Phase 2: pattern rules.
    for (i, abbr) in config.abbr.iter().enumerate() {
        if !abbr.key.contains('{') {
            continue;
        }
        let Some(bindings) = match_abbr_key(&abbr.key, token) else {
            continue;
        };
        any_key_matched = true;
        match try_which_rule(abbr, shell, &command_exists, &bindings) {
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
    if bindings.number.is_none() && abbr.key == template {
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
    let Some(expansion) = render_expansion(abbr, shell, bindings) else {
        // Render-time guard tripped (length cap, missing unit) — treat as
        // SelfLoop-equivalent skip for now. A dedicated SkipReason can be
        // added later if `which --why` needs to distinguish this case.
        return WhichOutcome::Skip(SkipReason::SelfLoop);
    };
    WhichOutcome::Hit { expansion, satisfied }
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
        }
    }

    fn abbr_pershell_expand(key: &str, expand: PerShellString) -> Abbr {
        Abbr {
            key: key.into(),
            expand,
            when_command_exists: None,
            number: None,
        }
    }

    fn abbr_with_number(key: &str, expand: &str, unit: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(expand.into()),
            when_command_exists: None,
            number: Some(unit.into()),
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
            Some(Bindings { number: Some(3) })
        );
    }

    #[test]
    fn match_abbr_key_pattern_captures_10() {
        assert_eq!(
            match_abbr_key("up{number}", "up10"),
            Some(Bindings { number: Some(10) })
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
            Some(Bindings { number: Some(128) })
        );
    }

    #[test]
    fn match_abbr_key_pattern_with_suffix() {
        assert_eq!(
            match_abbr_key("x{number}y", "x3y"),
            Some(Bindings { number: Some(3) })
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
        let out = render_expansion(&a, Shell::Bash, &Bindings { number: Some(3) });
        assert_eq!(out.as_deref(), Some("cd ../../../"));
    }

    #[test]
    fn render_expansion_rejects_when_total_repeat_exceeds_cap() {
        // unit = 50 bytes, n = 128 → 6400 > 4096
        let a = abbr_with_number("u{number}", "{number}", &"X".repeat(50));
        let out = render_expansion(&a, Shell::Bash, &Bindings { number: Some(128) });
        assert_eq!(out, None);
    }

    #[test]
    fn render_expansion_without_bindings_returns_template() {
        let a = abbr("gcm", "git commit -m");
        let out = render_expansion(&a, Shell::Bash, &Bindings::empty());
        assert_eq!(out.as_deref(), Some("git commit -m"));
    }

    #[test]
    fn render_expansion_missing_unit_returns_none() {
        // key has {number} but `number = ...` is absent. Validation should
        // catch this at parse; the runtime is defensive.
        let mut a = abbr("up{number}", "cd {number}");
        a.number = None;
        let out = render_expansion(&a, Shell::Bash, &Bindings { number: Some(3) });
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
}
