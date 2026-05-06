use std::path::PathBuf;

use crate::domain::model::Config;
use crate::domain::sanitize::is_deceptive_unicode;

const MAX_ABBR_RULES: usize = 10_000;
const MAX_KEY_BYTES: usize = 1_024;
const MAX_EXPAND_BYTES: usize = 4_096;
const MAX_CMD_BYTES: usize = 255;
const MAX_CMD_LIST_LEN: usize = 64;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    #[error("{0}")]
    Parse(#[from] toml::de::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot determine config directory")]
    NoConfigDir,
    #[error("config file exceeds maximum size of 10 MB")]
    FileTooLarge,
    #[error("config has too many abbr rules (max {MAX_ABBR_RULES})")]
    TooManyRules,
    #[error("abbr rule #{0}: key exceeds maximum length of {MAX_KEY_BYTES} bytes")]
    KeyTooLong(usize),
    #[error("abbr rule #{0}: expand exceeds maximum length of {MAX_EXPAND_BYTES} bytes")]
    ExpandTooLong(usize),
    #[error("abbr rule #{0}: key contains a NUL byte")]
    KeyContainsNul(usize),
    #[error("abbr rule #{0}: expand contains a NUL byte")]
    ExpandContainsNul(usize),
    #[error("abbr rule #{0}: when_command_exists entry exceeds maximum length of {MAX_CMD_BYTES} bytes")]
    CmdTooLong(usize),
    #[error("abbr rule #{0}: when_command_exists entry contains a NUL byte")]
    CmdContainsNul(usize),
    #[error("abbr rule #{0}: when_command_exists entry contains an ASCII control character (use printable characters only)")]
    CmdContainsControlChar(usize),
    #[error("abbr rule #{0}: key contains an ASCII control character (use printable characters only)")]
    KeyContainsControlChar(usize),
    #[error("abbr rule #{0}: expand contains an ASCII control character (use printable characters only)")]
    ExpandContainsControlChar(usize),
    #[error("abbr rule #{0}: key is empty (an empty key can never match anything)")]
    KeyEmpty(usize),
    #[error("abbr rule #{0}: key contains only whitespace (a whitespace-only key can never match)")]
    KeyWhitespaceOnly(usize),
    #[error("abbr rule #{0}: when_command_exists entry is empty (an empty command name can never be found)")]
    CmdEmpty(usize),
    #[error("abbr rule #{0}: when_command_exists entry contains only whitespace (a whitespace-only command name can never be found)")]
    CmdWhitespaceOnly(usize),
    #[error("abbr rule #{0}: key contains a Unicode visual-deception character (invisible/directional char that makes the key unmatchable or misleading)")]
    KeyContainsDeceptiveUnicode(usize),
    #[error("abbr rule #{0}: expand contains a Unicode visual-deception character (invisible/directional char that makes the expansion misleading)")]
    ExpandContainsDeceptiveUnicode(usize),
    #[error("abbr rule #{0}: when_command_exists entry contains a Unicode visual-deception character")]
    CmdContainsDeceptiveUnicode(usize),
    #[error("abbr rule #{0}: when_command_exists entry contains a path separator ('/', '\\\\', or ':'); only bare command names are allowed")]
    CmdContainsPathSeparator(usize),
    #[error("abbr rule #{0}: when_command_exists entry contains a shell metacharacter or glob pattern; only bare command names are allowed")]
    CmdContainsMetacharacter(usize),
    #[error("abbr rule #{0}: when_command_exists has too many entries (max {MAX_CMD_LIST_LEN})")]
    TooManyCmds(usize),
    #[error("unsupported config version {0}; only version 1 is supported")]
    UnsupportedVersion(u32),
    #[error("abbr rule #{0}: expand is empty (an empty expansion would silently delete the typed token)")]
    ExpandEmpty(usize),
    #[error("abbr rule #{0}: expand contains only whitespace (a whitespace-only expansion is almost certainly a config mistake)")]
    ExpandWhitespaceOnly(usize),
    #[error("[keybind.trigger.{0}] = \"ctrl-v\" is not a valid trigger; ctrl-v is reserved for [keybind.paste_intercept]")]
    CtrlVAsTrigger(&'static str),
    #[error("[keybind.self_insert.{0}] = \"ctrl-v\" is not a valid self-insert binding; ctrl-v is reserved for [keybind.paste_intercept]")]
    CtrlVAsSelfInsert(&'static str),
    #[error("[keybind.paste_intercept.{0}] is not supported (paste_intercept currently supports only nu); remove or move to nu")]
    PasteInterceptUnsupportedShell(&'static str),
    #[error("[keybind.paste_intercept.{0}] = \"{1}\" is not a valid paste-intercept binding; only ctrl-v is supported")]
    PasteInterceptInvalidKey(&'static str, &'static str),
}

/// Reason a validation check failed. Shared across the walker (for doctor
/// diagnostics) and the first-error adapter (for `parse_config`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ValidationReason {
    // config-scope
    TooManyRules,

    // key (rule-scope)
    KeyEmpty,
    KeyWhitespaceOnly,
    KeyTooLong,
    KeyContainsNul,
    KeyContainsControlChar,
    KeyContainsDeceptiveUnicode,

    // expand (rule-scope)
    ExpandEmpty,
    ExpandWhitespaceOnly,
    ExpandTooLong,
    ExpandContainsNul,
    ExpandContainsControlChar,
    ExpandContainsDeceptiveUnicode,

    // when_command_exists list-level (rule-scope)
    TooManyCmds,

    // when_command_exists entry (rule-scope)
    CmdEmpty,
    CmdWhitespaceOnly,
    CmdTooLong,
    CmdContainsNul,
    CmdContainsControlChar,
    CmdContainsDeceptiveUnicode,
    CmdContainsPathSeparator,
    CmdContainsMetacharacter,
}

/// A single validation failure.
///
/// `ValidationIssue` carries enough information for `doctor` to produce a
/// field-path-aware warning ("abbr[3].expand.pwsh rejected: expand is empty")
/// while `parse_config` can still convert it back into a single `ConfigError`.
///
/// `field_path` is a *logical* path — it mirrors the in-memory `PerShellString`
/// / `PerShellCmds` shape, not the literal TOML syntax. Array indices inside
/// `field_path` are 1-based (consistent with `rule_index`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ValidationIssue {
    Config {
        reason: ValidationReason,
    },
    Rule {
        rule_index: usize,
        field_path: String,
        reason: ValidationReason,
    },
}

impl ValidationIssue {
    /// Human-readable reason phrase (no leading "abbr rule #N: " prefix).
    /// Used by `doctor` for WARN detail text.
    pub(crate) fn reason_text(&self) -> &'static str {
        let reason = match self {
            ValidationIssue::Config { reason } => reason,
            ValidationIssue::Rule { reason, .. } => reason,
        };
        match reason {
            ValidationReason::TooManyRules => "config has too many abbr rules",
            ValidationReason::KeyEmpty => "key is empty",
            ValidationReason::KeyWhitespaceOnly => "key contains only whitespace",
            ValidationReason::KeyTooLong => "key exceeds the maximum length",
            ValidationReason::KeyContainsNul => "key contains a NUL byte",
            ValidationReason::KeyContainsControlChar => "key contains an ASCII control character",
            ValidationReason::KeyContainsDeceptiveUnicode => "key contains a Unicode visual-deception character",
            ValidationReason::ExpandEmpty => "expand is empty",
            ValidationReason::ExpandWhitespaceOnly => "expand contains only whitespace",
            ValidationReason::ExpandTooLong => "expand exceeds the maximum length",
            ValidationReason::ExpandContainsNul => "expand contains a NUL byte",
            ValidationReason::ExpandContainsControlChar => "expand contains an ASCII control character",
            ValidationReason::ExpandContainsDeceptiveUnicode => "expand contains a Unicode visual-deception character",
            ValidationReason::TooManyCmds => "when_command_exists has too many entries",
            ValidationReason::CmdEmpty => "when_command_exists entry is empty",
            ValidationReason::CmdWhitespaceOnly => "when_command_exists entry contains only whitespace",
            ValidationReason::CmdTooLong => "when_command_exists entry exceeds the maximum length",
            ValidationReason::CmdContainsNul => "when_command_exists entry contains a NUL byte",
            ValidationReason::CmdContainsControlChar => "when_command_exists entry contains an ASCII control character",
            ValidationReason::CmdContainsDeceptiveUnicode => "when_command_exists entry contains a Unicode visual-deception character",
            ValidationReason::CmdContainsPathSeparator => "when_command_exists entry contains a path separator",
            ValidationReason::CmdContainsMetacharacter => "when_command_exists entry contains a shell metacharacter or glob pattern",
        }
    }

    /// Convert back to a `ConfigError` for `parse_config`.
    pub(crate) fn to_config_error(&self) -> ConfigError {
        let (reason, n_for_rule) = match self {
            ValidationIssue::Config { reason } => (reason, 0usize),
            ValidationIssue::Rule { reason, rule_index, .. } => (reason, *rule_index),
        };
        let n = n_for_rule;
        match reason {
            ValidationReason::TooManyRules => ConfigError::TooManyRules,
            ValidationReason::KeyEmpty => ConfigError::KeyEmpty(n),
            ValidationReason::KeyWhitespaceOnly => ConfigError::KeyWhitespaceOnly(n),
            ValidationReason::KeyTooLong => ConfigError::KeyTooLong(n),
            ValidationReason::KeyContainsNul => ConfigError::KeyContainsNul(n),
            ValidationReason::KeyContainsControlChar => ConfigError::KeyContainsControlChar(n),
            ValidationReason::KeyContainsDeceptiveUnicode => ConfigError::KeyContainsDeceptiveUnicode(n),
            ValidationReason::ExpandEmpty => ConfigError::ExpandEmpty(n),
            ValidationReason::ExpandWhitespaceOnly => ConfigError::ExpandWhitespaceOnly(n),
            ValidationReason::ExpandTooLong => ConfigError::ExpandTooLong(n),
            ValidationReason::ExpandContainsNul => ConfigError::ExpandContainsNul(n),
            ValidationReason::ExpandContainsControlChar => ConfigError::ExpandContainsControlChar(n),
            ValidationReason::ExpandContainsDeceptiveUnicode => ConfigError::ExpandContainsDeceptiveUnicode(n),
            ValidationReason::TooManyCmds => ConfigError::TooManyCmds(n),
            ValidationReason::CmdEmpty => ConfigError::CmdEmpty(n),
            ValidationReason::CmdWhitespaceOnly => ConfigError::CmdWhitespaceOnly(n),
            ValidationReason::CmdTooLong => ConfigError::CmdTooLong(n),
            ValidationReason::CmdContainsNul => ConfigError::CmdContainsNul(n),
            ValidationReason::CmdContainsControlChar => ConfigError::CmdContainsControlChar(n),
            ValidationReason::CmdContainsDeceptiveUnicode => ConfigError::CmdContainsDeceptiveUnicode(n),
            ValidationReason::CmdContainsPathSeparator => ConfigError::CmdContainsPathSeparator(n),
            ValidationReason::CmdContainsMetacharacter => ConfigError::CmdContainsMetacharacter(n),
        }
    }
}

/// Validate the `key` field of an abbreviation rule.
///
/// Rejects keys that are empty, whitespace-only, or exceed [`MAX_KEY_BYTES`].
/// Also rejects keys containing NUL bytes, ASCII control characters, or Unicode
/// visual-deception characters — all of which would make the key unmatchable or
/// cause it to display differently from its actual byte content.
fn check_abbr_key(key: &str) -> Option<ValidationReason> {
    if key.is_empty() {
        return Some(ValidationReason::KeyEmpty);
    }
    if key.trim().is_empty() {
        return Some(ValidationReason::KeyWhitespaceOnly);
    }
    if key.len() > MAX_KEY_BYTES {
        return Some(ValidationReason::KeyTooLong);
    }
    if key.contains('\0') {
        return Some(ValidationReason::KeyContainsNul);
    }
    if key.chars().any(|c| c.is_ascii_control()) {
        return Some(ValidationReason::KeyContainsControlChar);
    }
    if key.chars().any(is_deceptive_unicode) {
        return Some(ValidationReason::KeyContainsDeceptiveUnicode);
    }
    None
}

/// Validate a single expand string value.
fn check_expand_value(expand: &str) -> Option<ValidationReason> {
    if expand.is_empty() {
        return Some(ValidationReason::ExpandEmpty);
    }
    if expand.trim().is_empty() {
        return Some(ValidationReason::ExpandWhitespaceOnly);
    }
    if expand.len() > MAX_EXPAND_BYTES {
        return Some(ValidationReason::ExpandTooLong);
    }
    if expand.contains('\0') {
        return Some(ValidationReason::ExpandContainsNul);
    }
    if expand.chars().any(|c| c.is_ascii_control()) {
        return Some(ValidationReason::ExpandContainsControlChar);
    }
    if expand.chars().any(is_deceptive_unicode) {
        return Some(ValidationReason::ExpandContainsDeceptiveUnicode);
    }
    None
}

/// Validate a single `when_command_exists` entry.
///
/// Rejects entries that are empty, whitespace-only, or exceed [`MAX_CMD_BYTES`].
/// Also rejects entries containing NUL bytes, ASCII control characters, Unicode
/// visual-deception characters, or path separators (`/`, `\`, `:`).
/// Only bare command names are allowed — filesystem paths would bypass the intent
/// of checking only within `path_prepend`.
fn check_cmd_entry(cmd: &str) -> Option<ValidationReason> {
    if cmd.is_empty() {
        return Some(ValidationReason::CmdEmpty);
    }
    if cmd.trim().is_empty() {
        return Some(ValidationReason::CmdWhitespaceOnly);
    }
    if cmd.len() > MAX_CMD_BYTES {
        return Some(ValidationReason::CmdTooLong);
    }
    if cmd.contains('\0') {
        return Some(ValidationReason::CmdContainsNul);
    }
    if cmd.chars().any(|c| c.is_ascii_control()) {
        return Some(ValidationReason::CmdContainsControlChar);
    }
    if cmd.chars().any(is_deceptive_unicode) {
        return Some(ValidationReason::CmdContainsDeceptiveUnicode);
    }
    if cmd.contains('/') || cmd.contains('\\') || cmd.contains(':') {
        return Some(ValidationReason::CmdContainsPathSeparator);
    }
    // Reject shell metacharacters, cmd.exe metacharacters, glob patterns, and
    // the `,`/`=` delimiters used by the precache resolved protocol.
    // Only bare alphanumeric command names (plus `-`, `_`, `.`, `+`) are allowed.
    const METACHARS: &[char] = &[
        // POSIX shell
        '&', '|', ';', '<', '>', '`', '$', '(', ')', '{', '}', '\'', '"',
        // cmd.exe
        '%', '^',
        // Whitespace that breaks shell/cmd tokenization
        ' ', '\t',
        // Glob patterns (matched by Get-Command -Name in pwsh)
        '*', '?', '[', ']',
        // Precache resolved protocol delimiters
        ',', '=',
        // Other risky punctuation in some shells
        '!', '#', '~',
    ];
    if cmd.chars().any(|c| METACHARS.contains(&c)) {
        return Some(ValidationReason::CmdContainsMetacharacter);
    }
    None
}

/// Shell labels for per-shell variant field paths. Order matches
/// `PerShellString::all_values` / `PerShellCmds::all_values`.
const PER_SHELL_LABELS: &[&str] = &["default", "bash", "zsh", "pwsh", "nu"];

/// Walk all rule-scope expand values for a `PerShellString`.
/// Visits the same values in the same order as `PerShellString::all_values()`.
fn walk_expand_issues(
    expand: &crate::domain::model::PerShellString,
    rule_index: usize,
    mut f: impl FnMut(ValidationIssue) -> std::ops::ControlFlow<()>,
) -> std::ops::ControlFlow<()> {
    use crate::domain::model::PerShellString;
    match expand {
        PerShellString::All(s) => {
            if let Some(reason) = check_expand_value(s) {
                f(ValidationIssue::Rule {
                    rule_index,
                    field_path: "expand".into(),
                    reason,
                })?;
            }
        }
        PerShellString::ByShell { default, bash, zsh, pwsh, nu } => {
            let variants: [(&&str, &Option<String>); 5] = [
                (&PER_SHELL_LABELS[0], default),
                (&PER_SHELL_LABELS[1], bash),
                (&PER_SHELL_LABELS[2], zsh),
                (&PER_SHELL_LABELS[3], pwsh),
                (&PER_SHELL_LABELS[4], nu),
            ];
            for (label, value) in variants {
                if let Some(s) = value {
                    if let Some(reason) = check_expand_value(s) {
                        f(ValidationIssue::Rule {
                            rule_index,
                            field_path: format!("expand.{}", label),
                            reason,
                        })?;
                    }
                }
            }
        }
    }
    std::ops::ControlFlow::Continue(())
}

/// Walk `when_command_exists`, including the list-level `TooManyCmds` check
/// before per-entry checks. Preserves `parse_config`'s ordering.
fn walk_cmds_issues(
    cmds: &crate::domain::model::PerShellCmds,
    rule_index: usize,
    mut f: impl FnMut(ValidationIssue) -> std::ops::ControlFlow<()>,
) -> std::ops::ControlFlow<()> {
    use crate::domain::model::PerShellCmds;

    // Variant-level walk: always in [All] or [default, bash, zsh, pwsh, nu] order.
    let variants: Vec<(Option<&str>, &[String])> = match cmds {
        PerShellCmds::All(v) => vec![(None, v.as_slice())],
        PerShellCmds::ByShell { default, bash, zsh, pwsh, nu } => {
            let mut out = Vec::with_capacity(5);
            for (label, value) in [
                (PER_SHELL_LABELS[0], default),
                (PER_SHELL_LABELS[1], bash),
                (PER_SHELL_LABELS[2], zsh),
                (PER_SHELL_LABELS[3], pwsh),
                (PER_SHELL_LABELS[4], nu),
            ] {
                if let Some(v) = value {
                    out.push((Some(label), v.as_slice()));
                }
            }
            out
        }
    };

    for (label, list) in variants {
        // (list-level) TooManyCmds before per-entry validation.
        if list.len() > MAX_CMD_LIST_LEN {
            let path = match label {
                Some(l) => format!("when_command_exists.{}", l),
                None => "when_command_exists".into(),
            };
            f(ValidationIssue::Rule {
                rule_index,
                field_path: path,
                reason: ValidationReason::TooManyCmds,
            })?;
            continue; // skip per-entry walk for an oversize list
        }
        // Per-entry walk
        for (j, cmd) in list.iter().enumerate() {
            if let Some(reason) = check_cmd_entry(cmd) {
                let path = match label {
                    Some(l) => format!("when_command_exists.{}[{}]", l, j + 1),
                    None => format!("when_command_exists[{}]", j + 1),
                };
                f(ValidationIssue::Rule {
                    rule_index,
                    field_path: path,
                    reason,
                })?;
            }
        }
    }
    std::ops::ControlFlow::Continue(())
}

/// Visit every validation issue in the config in `parse_config` order.
///
/// The caller chooses `Continue` (collect all) or `Break` (stop at first).
fn visit_validation_issues(
    config: &Config,
    mut f: impl FnMut(ValidationIssue) -> std::ops::ControlFlow<()>,
) {
    if config.abbr.len() > MAX_ABBR_RULES {
        let _ = f(ValidationIssue::Config { reason: ValidationReason::TooManyRules });
        return;
    }
    for (i, abbr) in config.abbr.iter().enumerate() {
        let rule_index = i + 1;
        // (b-1) key
        if let Some(reason) = check_abbr_key(&abbr.key) {
            if f(ValidationIssue::Rule {
                rule_index,
                field_path: "key".into(),
                reason,
            })
            .is_break()
            {
                return;
            }
        }
        // (b-2) expand (per-shell aware)
        if walk_expand_issues(&abbr.expand, rule_index, &mut f).is_break() {
            return;
        }
        // (b-3) when_command_exists (per-shell aware + list-level + per-entry)
        if let Some(cmds) = &abbr.when_command_exists {
            if walk_cmds_issues(cmds, rule_index, &mut f).is_break() {
                return;
            }
        }
    }
}

/// Collect every validation issue in the config (used by `doctor`).
pub(crate) fn collect_validation_issues(config: &Config) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    visit_validation_issues(config, |issue| {
        issues.push(issue);
        std::ops::ControlFlow::Continue(())
    });
    issues
}

/// Return the first validation error, preserving `parse_config` ordering.
fn first_validation_error(config: &Config) -> Option<ConfigError> {
    let mut first = None;
    visit_validation_issues(config, |issue| {
        first = Some(issue.to_config_error());
        std::ops::ControlFlow::Break(())
    });
    first
}

/// Deserialize a TOML string to `Config` without running validation.
/// Used by `doctor` to walk the config for issues even when `parse_config`
/// would fail. Version check is still enforced.
pub(crate) fn parse_config_lenient(s: &str) -> Result<Config, ConfigError> {
    let config: Config = toml::from_str(s)?;
    if config.version != 1 {
        return Err(ConfigError::UnsupportedVersion(config.version));
    }
    Ok(config)
}

/// Parse a TOML string into a [`Config`].
///
/// Only version 1 is accepted. All abbreviation rules are validated via
/// [`visit_validation_issues`]; the first violation is returned as a
/// `ConfigError`. Validation order is pinned by the `visit_validation_issues`
/// traversal order.
pub(crate) fn parse_config(s: &str) -> Result<Config, ConfigError> {
    let config = parse_config_lenient(s)?;
    if let Some(e) = first_keybind_error(&config) {
        return Err(e);
    }
    if let Some(e) = first_validation_error(&config) {
        return Err(e);
    }
    Ok(config)
}

/// Reject keybind configurations that are syntactically valid (the
/// enum variant exists) but semantically wrong:
///
/// - `[keybind.trigger]` / `[keybind.self_insert]` with `ctrl-v`:
///   `CtrlV` is exclusively for `[keybind.paste_intercept]`. Using
///   it as a trigger would route Ctrl+V into the abbreviation
///   engine, which has no chord representation for it.
/// - `[keybind.paste_intercept]` for any shell other than nu:
///   only nu has the paste-mid-trigger limitation that
///   paste_intercept solves; other shells either have no problem
///   (bash/zsh/clink) or already short-circuit via `paste_pending`
///   (pwsh).
/// - `[keybind.paste_intercept]` set to anything other than
///   `ctrl-v`: paste interception is bound to a single chord by
///   design (a chord that paste streams cannot contain).
fn first_keybind_error(config: &Config) -> Option<ConfigError> {
    use crate::domain::model::TriggerKey;

    let trigger = &config.keybind.trigger;
    for (label, value) in [
        ("default", trigger.default),
        ("bash", trigger.bash),
        ("zsh", trigger.zsh),
        ("pwsh", trigger.pwsh),
        ("nu", trigger.nu),
    ] {
        if value == Some(TriggerKey::CtrlV) {
            return Some(ConfigError::CtrlVAsTrigger(label));
        }
    }

    let self_insert = &config.keybind.self_insert;
    for (label, value) in [
        ("default", self_insert.default),
        ("bash", self_insert.bash),
        ("zsh", self_insert.zsh),
        ("pwsh", self_insert.pwsh),
        ("nu", self_insert.nu),
    ] {
        if value == Some(TriggerKey::CtrlV) {
            return Some(ConfigError::CtrlVAsSelfInsert(label));
        }
    }

    let paste = &config.keybind.paste_intercept;
    for (label, value) in [
        ("default", paste.default),
        ("bash", paste.bash),
        ("zsh", paste.zsh),
        ("pwsh", paste.pwsh),
    ] {
        if value.is_some() {
            return Some(ConfigError::PasteInterceptUnsupportedShell(label));
        }
    }
    if let Some(k) = paste.nu {
        if k != TriggerKey::CtrlV {
            let key_str = match k {
                TriggerKey::Space => "space",
                TriggerKey::Tab => "tab",
                TriggerKey::AltSpace => "alt-space",
                TriggerKey::ShiftSpace => "shift-space",
                TriggerKey::CtrlV => unreachable!(),
            };
            return Some(ConfigError::PasteInterceptInvalidKey("nu", key_str));
        }
    }

    None
}

/// Default config file path. Re-exported wrapper around
/// [`crate::infra::config_store::default_config_path`] so existing
/// `crate::app::config::default_config_path` call sites keep working
/// after the D3 split. Phase D D4 / future cleanup may inline these
/// shims; the function itself lives in `infra` because it touches
/// `RUNEX_CONFIG` env / `XDG_CONFIG_HOME` resolution.
pub(crate) fn default_config_path() -> Result<PathBuf, ConfigError> {
    crate::infra::config_store::default_config_path()
}

/// Load config from a file path. The I/O happens in `infra`; this
/// function pairs the read with `parse_config` so callers get a typed
/// `Config` back.
///
/// Symlink handling, size cap, and TOCTOU avoidance are all enforced
/// inside [`crate::infra::config_store::read_config_source`]. The
/// rationale (dotfiles symlink idiom etc.) is documented there.
pub(crate) fn load_config(path: &std::path::Path) -> Result<Config, ConfigError> {
    let content = crate::infra::config_store::read_config_source(path)?;
    parse_config(&content)
}

/// Append an abbreviation rule to a config file.
///
/// Validates the new rule (using the pure `check_*` helpers), then
/// delegates the file append to
/// [`crate::infra::config_store::append_abbr_block`]. Phase D D4
/// will move this wrapper to `app::abbr` and have `cmd/add_remove`
/// call that instead.
pub(crate) fn append_abbr_to_file(
    path: &std::path::Path,
    key: &str,
    expand: &str,
    when_command_exists: Option<&[String]>,
) -> Result<(), ConfigError> {
    let n = 0; // validation uses 1-indexed rule numbers, but we use 0 for "new rule"
    if let Some(reason) = check_abbr_key(key) {
        return Err(ValidationIssue::Rule { rule_index: n, field_path: "key".into(), reason }.to_config_error());
    }
    if let Some(reason) = check_expand_value(expand) {
        return Err(ValidationIssue::Rule { rule_index: n, field_path: "expand".into(), reason }.to_config_error());
    }
    if let Some(cmds) = when_command_exists {
        for cmd in cmds {
            if let Some(reason) = check_cmd_entry(cmd) {
                return Err(ValidationIssue::Rule { rule_index: n, field_path: "when_command_exists".into(), reason }.to_config_error());
            }
        }
    }
    crate::infra::config_store::append_abbr_block(path, key, expand, when_command_exists)
}

/// Remove all abbreviation rules with the given key from a config
/// file. Thin wrapper around
/// [`crate::infra::config_store::remove_abbr_block`]; kept here for
/// call-site continuity until Phase D D4 routes `cmd/add_remove`
/// through `app::abbr`.
pub(crate) fn remove_abbr_from_file(path: &std::path::Path, key: &str) -> Result<usize, ConfigError> {
    crate::infra::config_store::remove_abbr_block(path, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::TriggerKey;

    mod parsing {
        use super::*;

    #[test]
    fn parse_minimal_toml() {
        let toml = r#"
version = 1

[[abbr]]
key = "gcm"
expand = "git commit -m"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(config.abbr.len(), 1);
        assert_eq!(config.abbr[0].key, "gcm");
        assert_eq!(config.abbr[0].expand, crate::domain::model::PerShellString::All("git commit -m".into()));
    }

    #[test]
    fn parse_with_when_command_exists() {
        let toml = r#"
version = 1

[[abbr]]
key = "ls"
expand = "lsd"
when_command_exists = ["lsd"]
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(
            config.abbr[0].when_command_exists,
            Some(crate::domain::model::PerShellCmds::All(vec!["lsd".to_string()]))
        );
    }

    #[test]
    fn parse_with_keybind() {
        let toml = r#"
version = 1

[keybind.trigger]
default = "space"
bash = "alt-space"
zsh = "space"
pwsh = "tab"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.keybind.trigger.default, Some(TriggerKey::Space));
        assert_eq!(config.keybind.trigger.bash, Some(TriggerKey::AltSpace));
        assert_eq!(config.keybind.trigger.zsh, Some(TriggerKey::Space));
        assert_eq!(config.keybind.trigger.pwsh, Some(TriggerKey::Tab));
        assert_eq!(config.keybind.trigger.nu, None);
    }

    #[test]
    fn parse_config_with_subtable_trigger() {
        let toml = r#"
version = 1

[keybind.trigger]
default = "space"
bash = "alt-space"
pwsh = "tab"

[keybind.self_insert]
pwsh = "shift-space"
nu   = "shift-space"
"#;
        let config = parse_config(toml).unwrap();
        assert_eq!(config.keybind.trigger.default, Some(TriggerKey::Space));
        assert_eq!(config.keybind.trigger.bash, Some(TriggerKey::AltSpace));
        assert_eq!(config.keybind.trigger.pwsh, Some(TriggerKey::Tab));
        assert_eq!(config.keybind.trigger.zsh, None);
        assert_eq!(config.keybind.self_insert.pwsh, Some(TriggerKey::ShiftSpace));
        assert_eq!(config.keybind.self_insert.nu, Some(TriggerKey::ShiftSpace));
        assert_eq!(config.keybind.self_insert.bash, None);
    }

    #[test]
    fn parse_config_keybind_absent_gives_all_none() {
        let toml = "version = 1\n";
        let config = parse_config(toml).unwrap();
        assert_eq!(config.keybind.trigger.default, None);
        assert_eq!(config.keybind.trigger.bash, None);
        assert_eq!(config.keybind.self_insert.pwsh, None);
    }

    /// TOML allows any string for `trigger`, but only known variants are valid.
    /// An unknown value must be rejected so the user gets an explicit error rather than
    /// silently falling back to a default they didn't request.
    #[test]
    fn parse_config_rejects_invalid_trigger_key() {
        let toml = "version = 1\n[keybind.trigger]\ndefault = \"invalid-key\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject unknown trigger key value 'invalid-key'"
        );
    }

    #[test]
    fn parse_config_rejects_invalid_per_shell_keybind() {
        for field in ["bash", "zsh", "pwsh", "nu"] {
            let toml = format!("version = 1\n[keybind.trigger]\n{field} = \"unknown-keybind\"\n");
            assert!(
                parse_config(&toml).is_err(),
                "must reject unknown keybind value for field '{field}'"
            );
        }
    }

    /// `ctrl-v` is reserved for `[keybind.paste_intercept]`; it must
    /// not be silently accepted as a regular trigger or self-insert.
    #[test]
    fn parse_config_rejects_ctrl_v_as_trigger() {
        for field in ["default", "bash", "zsh", "pwsh", "nu"] {
            let toml = format!("version = 1\n[keybind.trigger]\n{field} = \"ctrl-v\"\n");
            let err = parse_config(&toml).expect_err(
                "ctrl-v must be rejected when used as a regular trigger",
            );
            assert!(
                matches!(err, ConfigError::CtrlVAsTrigger(label) if label == field),
                "expected CtrlVAsTrigger({field}), got {err:?}"
            );
        }
    }

    #[test]
    fn parse_config_rejects_ctrl_v_as_self_insert() {
        for field in ["default", "bash", "zsh", "pwsh", "nu"] {
            let toml = format!(
                "version = 1\n[keybind.self_insert]\n{field} = \"ctrl-v\"\n"
            );
            let err = parse_config(&toml).expect_err(
                "ctrl-v must be rejected when used as a self-insert binding",
            );
            assert!(
                matches!(err, ConfigError::CtrlVAsSelfInsert(label) if label == field),
                "expected CtrlVAsSelfInsert({field}), got {err:?}"
            );
        }
    }

    /// `[keybind.paste_intercept]` only supports nu today; setting it
    /// for any other shell must be rejected so users don't think they
    /// configured something useful.
    #[test]
    fn parse_config_rejects_paste_intercept_on_non_nu_shells() {
        for field in ["default", "bash", "zsh", "pwsh"] {
            let toml = format!(
                "version = 1\n[keybind.paste_intercept]\n{field} = \"ctrl-v\"\n"
            );
            let err = parse_config(&toml).expect_err(
                "paste_intercept on non-nu shells must be rejected",
            );
            assert!(
                matches!(err, ConfigError::PasteInterceptUnsupportedShell(label) if label == field),
                "expected PasteInterceptUnsupportedShell({field}), got {err:?}"
            );
        }
    }

    /// nu's paste_intercept must be `ctrl-v` (the only chord paste
    /// streams cannot contain). Anything else is rejected so we don't
    /// silently degrade back to the paste-mid-trigger limitation.
    #[test]
    fn parse_config_rejects_paste_intercept_non_ctrl_v_for_nu() {
        for key in ["space", "tab", "alt-space", "shift-space"] {
            let toml = format!(
                "version = 1\n[keybind.paste_intercept]\nnu = \"{key}\"\n"
            );
            let err = parse_config(&toml).expect_err(
                "paste_intercept.nu must require ctrl-v",
            );
            assert!(
                matches!(&err, ConfigError::PasteInterceptInvalidKey(shell, k) if *shell == "nu" && *k == key),
                "expected PasteInterceptInvalidKey(nu, {key}), got {err:?}"
            );
        }
    }

    /// Sanity check: `paste_intercept.nu = "ctrl-v"` is the
    /// canonical accepted configuration.
    #[test]
    fn parse_config_accepts_paste_intercept_ctrl_v_for_nu() {
        let toml = "version = 1\n[keybind.paste_intercept]\nnu = \"ctrl-v\"\n";
        let config = parse_config(toml).expect("ctrl-v on nu must be accepted");
        assert_eq!(
            config.keybind.paste_intercept.nu,
            Some(TriggerKey::CtrlV),
            "round-trip must preserve CtrlV"
        );
    }

    #[test]
    fn parse_missing_version_is_err() {
        let toml = r#"
[[abbr]]
key = "gcm"
expand = "git commit -m"
"#;
        assert!(parse_config(toml).is_err());
    }

    #[test]
    fn parse_empty_abbr_list() {
        let toml = "version = 1\n";
        let config = parse_config(toml).unwrap();
        assert!(config.abbr.is_empty());
    }

    // File I/O tests for `load_config` / `default_config_path`
    // moved to `infra::config_store::tests` in Phase D D3b along
    // with the underlying functions. `xdg_config_home_*` tests
    // moved to `infra::env::tests`.

    #[test]
    fn parse_config_rejects_too_many_abbr() {
        let mut s = String::from("version = 1\n");
        for i in 0..10_001 {
            s.push_str(&format!("[[abbr]]\nkey = \"k{i}\"\nexpand = \"v{i}\"\n"));
        }
        assert!(parse_config(&s).is_err(), "must reject configs with more than 10,000 abbr rules");
    }

    #[test]
    fn parse_config_accepts_max_abbr() {
        let mut s = String::from("version = 1\n");
        for i in 0..10_000 {
            s.push_str(&format!("[[abbr]]\nkey = \"k{i}\"\nexpand = \"v{i}\"\n"));
        }
        assert!(parse_config(&s).is_ok(), "must accept exactly 10,000 abbr rules");
    }

    // `load_config_rejects_*` and `load_config_follows_symlink_*`
    // tests moved to `infra::config_store::tests` in Phase D D3b
    // (they exercise the `read_config_source` filesystem layer).

    } // mod parsing

    mod field_validation {
        use super::*;

    #[test]
    fn parse_config_rejects_oversized_key() {
        let huge_key = "k".repeat(1025);
        let toml = format!("version = 1\n[[abbr]]\nkey = \"{huge_key}\"\nexpand = \"v\"\n");
        assert!(parse_config(&toml).is_err(), "must reject key longer than 1024 bytes");
    }

    #[test]
    fn parse_config_accepts_max_key_length() {
        let max_key = "k".repeat(1024);
        let toml = format!("version = 1\n[[abbr]]\nkey = \"{max_key}\"\nexpand = \"v\"\n");
        assert!(parse_config(&toml).is_ok(), "must accept key of exactly 1024 bytes");
    }

    #[test]
    fn parse_config_rejects_oversized_expand() {
        let huge_expand = "x".repeat(4097);
        let toml = format!("version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"{huge_expand}\"\n");
        assert!(parse_config(&toml).is_err(), "must reject expand longer than 4096 bytes");
    }

    #[test]
    fn parse_config_accepts_max_expand_length() {
        let max_expand = "x".repeat(4096);
        let toml = format!("version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"{max_expand}\"\n");
        assert!(parse_config(&toml).is_ok(), "must accept expand of exactly 4096 bytes");
    }

    #[test]
    fn parse_config_rejects_oversized_when_command_exists_entry() {
        let huge_cmd = "c".repeat(256);
        let toml = format!(
            "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [\"{huge_cmd}\"]\n"
        );
        assert!(parse_config(&toml).is_err(), "must reject when_command_exists entry longer than 255 bytes");
    }

    #[test]
    fn parse_config_accepts_max_when_command_exists_entry() {
        let max_cmd = "c".repeat(255);
        let toml = format!(
            "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [\"{max_cmd}\"]\n"
        );
        assert!(parse_config(&toml).is_ok(), "must accept when_command_exists entry of exactly 255 bytes");
    }

    #[test]
    fn parse_config_rejects_nul_byte_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [\"cmd\\u0000evil\"]\n";
        assert!(parse_config(toml).is_err(), "must reject when_command_exists entry containing NUL byte");
    }

    #[test]
    fn parse_config_rejects_nul_byte_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\\u0000evil\"\nexpand = \"v\"\n";
        assert!(parse_config(toml).is_err(), "must reject key containing NUL byte");
    }

    #[test]
    fn parse_config_rejects_nul_byte_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\\u0000evil\"\n";
        assert!(parse_config(toml).is_err(), "must reject expand containing NUL byte");
    }

    } // mod field_validation

    /// TOML allows `\uXXXX` escapes for any Unicode code point, including ASCII
    /// control characters (U+0001–U+001F, U+007F). These pass through `toml::from_str`
    /// but must be rejected by `parse_config` because:
    /// - key: quoting functions silently drop them, making the key unmatchable
    /// - expand: the expansion is silently mangled when printed
    /// - both: users get silent wrong behavior instead of a clear error
    mod control_char_rejection {
        use super::*;

    #[test]
    fn parse_config_rejects_control_char_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\\u001Bevil\"\nexpand = \"v\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject key containing ASCII control char (\\u001B)"
        );
    }

    #[test]
    fn parse_config_rejects_control_char_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\\u001Bevil\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject expand containing ASCII control char (\\u001B)"
        );
    }

    #[test]
    fn parse_config_rejects_del_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\\u007Fevil\"\nexpand = \"v\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject key containing DEL (\\u007F)"
        );
    }

    #[test]
    fn parse_config_rejects_del_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\\u007Fevil\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject expand containing DEL (\\u007F)"
        );
    }

    #[test]
    fn parse_config_accepts_key_without_control_chars() {
        let toml = "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n";
        assert!(parse_config(toml).is_ok(), "must accept key without control chars");
    }

    /// An empty key produces `''` in bash/zsh case statements, which matches
    /// the empty string — any empty-token expansion would silently fire.
    /// Reject early with a clear error rather than producing a broken script.
    #[test]
    fn parse_config_rejects_empty_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"\"\nexpand = \"git commit -m\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject an abbr rule with an empty key"
        );
    }

    /// A key consisting only of spaces would be silently dropped by quoting functions,
    /// making the rule unmatchable while appearing valid.
    #[test]
    fn parse_config_rejects_whitespace_only_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"   \"\nexpand = \"git commit -m\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject an abbr rule with a whitespace-only key"
        );
    }

    /// An empty string in `when_command_exists` is meaningless: `which::which("")` always
    /// fails, silently causing the rule to never expand.
    #[test]
    fn parse_config_rejects_empty_when_command_exists_entry() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry that is an empty string"
        );
    }

    /// A whitespace-only command name silently makes the rule permanently inactive.
    #[test]
    fn parse_config_rejects_whitespace_only_when_command_exists_entry() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"   \"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry that is whitespace-only"
        );
    }

    #[test]
    fn parse_config_rejects_control_char_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [\"cmd\\u001Bevil\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry containing ASCII control char (\\u001B)"
        );
    }

    #[test]
    fn parse_config_rejects_del_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [\"cmd\\u007Fevil\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry containing DEL (\\u007F)"
        );
    }

    } // mod control_char_rejection

    /// Characters such as U+FEFF (BOM/zero-width no-break space), U+202E (Right-to-Left
    /// Override), and other Unicode formatting/invisible characters cannot be seen in most
    /// terminals and text editors. If embedded in `key`, `expand`, or `when_command_exists`,
    /// they cause:
    /// - `key`: rule appears valid but never matches (invisible difference from real command)
    /// - `expand`: expansion contains invisible/deceptive text printed to terminal
    /// - `when_command_exists`: command lookup silently fails forever
    /// - `list` output: shows a key that looks like "ls" but is really `"\u{FEFF}ls"`
    ///
    /// These must be rejected early with a clear error.
    mod deceptive_unicode {
        use super::*;

    #[test]
    fn parse_config_rejects_bom_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"\\uFEFFls\"\nexpand = \"lsd\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject key containing U+FEFF (BOM / zero-width no-break space)"
        );
    }

    #[test]
    fn parse_config_rejects_rlo_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ab\\u202Ecd\"\nexpand = \"v\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject key containing U+202E (Right-to-Left Override)"
        );
    }

    #[test]
    fn parse_config_rejects_rlo_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"rm -rf \\u202E/ echo safe\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject expand containing U+202E (Right-to-Left Override)"
        );
    }

    #[test]
    fn parse_config_rejects_bom_in_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\\uFEFF\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject expand containing U+FEFF (BOM)"
        );
    }

    #[test]
    fn parse_config_rejects_bom_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"\\uFEFFlsd\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry containing U+FEFF (BOM)"
        );
    }

    #[test]
    fn parse_config_rejects_zwsp_in_key() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\\u200Bcd\"\nexpand = \"v\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject key containing U+200B (Zero-Width Space)"
        );
    }

    /// `when_command_exists` values must be bare command names, not filesystem paths.
    /// A value like `"/usr/bin/ls"` is a path traversal attempt: `dir.join("/usr/bin/ls")`
    /// on Unix resolves to an absolute path, bypassing the intended restriction to check
    /// only within `path_prepend`.
    #[test]
    fn parse_config_rejects_path_separator_in_when_command_exists() {
        for bad in ["/usr/bin/ls", "../../evil", "../bin/sh"] {
            let toml = format!(
                "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"{bad}\"]\n"
            );
            assert!(
                parse_config(&toml).is_err(),
                "must reject when_command_exists entry containing '/': {bad:?}"
            );
        }
    }

    /// On Windows, backslash is a path separator. Paths like `C:\bin\ls` must be
    /// caught at parse time before they reach `make_command_exists`.
    #[test]
    fn parse_config_rejects_backslash_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"bin\\\\ls\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry containing backslash"
        );
    }

    /// A colon introduces a Windows drive letter (e.g. `C:ls`) or acts as a
    /// PATH-like separator in some contexts.
    #[test]
    fn parse_config_rejects_colon_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"C:ls\"]\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject when_command_exists entry containing colon"
        );
    }

    /// Shell/cmd metacharacters could be injected into shell-side precache
    /// detection loops (e.g. clink's `io.popen("where " .. cmd)`) and allow
    /// arbitrary command execution at shell startup.
    #[test]
    fn parse_config_rejects_shell_metacharacters_in_when_command_exists() {
        let bad_entries = [
            "a&b", "a|b", "a;b", "a<b", "a>b", "a`b", "a$b",
            "a(b", "a)b", "a{b", "a}b", "a\"b", "a'b",
            "a%b", "a^b",  // cmd.exe
            "a b", "a\tb",  // whitespace breaks tokenization
            "a*b", "a?b", "a[b", "a]b",  // glob
            "a,b", "a=b",  // precache --resolved delimiters
            "a!b", "a#b", "a~b",  // other risky punctuation
        ];
        for bad in &bad_entries {
            let toml = format!(
                "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"{bad}\"]\n"
            );
            assert!(
                parse_config(&toml).is_err(),
                "must reject when_command_exists entry containing metachar: {bad:?}"
            );
        }
    }

    #[test]
    fn parse_config_accepts_bare_command_name_in_when_command_exists() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"lsd\"]\n";
        assert!(
            parse_config(toml).is_ok(),
            "must accept bare command name in when_command_exists"
        );
    }

    } // mod deceptive_unicode

    /// Each abbr rule's `when_command_exists` list is iterated on every expand call.
    /// Without a cap, a config with 100,000 entries would cause:
    /// - ~25 MB memory per rule (100,000 × 255 bytes)
    /// - 100,000 `which::which()` calls per keystroke — CPU/I/O DoS
    ///
    /// Capped at `MAX_CMD_LIST_LEN` entries per rule.
    mod when_command_exists_limit {
        use super::*;

    #[test]
    fn parse_config_rejects_too_many_when_command_exists_entries() {
        let cmds: Vec<String> = (0..=64).map(|i| format!("\"cmd{i}\"")).collect();
        let toml = format!(
            "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [{}]\n",
            cmds.join(", ")
        );
        assert!(
            parse_config(&toml).is_err(),
            "must reject when_command_exists with more than 64 entries"
        );
    }

    #[test]
    fn parse_config_accepts_max_when_command_exists_entries() {
        let cmds: Vec<String> = (0..64).map(|i| format!("\"cmd{i}\"")).collect();
        let toml = format!(
            "version = 1\n[[abbr]]\nkey = \"k\"\nexpand = \"v\"\nwhen_command_exists = [{}]\n",
            cmds.join(", ")
        );
        assert!(
            parse_config(&toml).is_ok(),
            "must accept when_command_exists with exactly 64 entries"
        );
    }

    } // mod when_command_exists_limit

    /// The only supported config schema version is 1. A config file with version=2
    /// (or any other value) was written for a different schema and must be rejected
    /// rather than silently processed as version=1. Accepting unknown versions risks
    /// missing new validation rules introduced in a later schema.
    mod version_validation {
        use super::*;

    #[test]
    fn parse_config_rejects_version_0() {
        let toml = "version = 0\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject version=0 (unsupported schema version)"
        );
    }

    #[test]
    fn parse_config_rejects_version_2() {
        let toml = "version = 2\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject version=2 (unsupported schema version)"
        );
    }

    #[test]
    fn parse_config_rejects_version_99() {
        let toml = "version = 99\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject version=99 (unsupported schema version)"
        );
    }

    #[test]
    fn parse_config_accepts_version_1() {
        let toml = "version = 1\n";
        assert!(
            parse_config(toml).is_ok(),
            "must accept version=1 (the current supported schema)"
        );
    }

    } // mod version_validation

    /// An expand value that is empty or whitespace-only is functionally broken:
    /// - Empty: pressing the trigger key replaces the token with nothing — almost certainly a mistake.
    /// - Whitespace-only: replaces the token with invisible characters — confusing and unintended.
    ///
    /// Both are rejected early so users get a clear error rather than silent breakage.
    mod expand_validation {
        use super::*;

    #[test]
    fn parse_config_rejects_empty_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"\"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject an abbr rule with an empty expand"
        );
    }

    #[test]
    fn parse_config_rejects_whitespace_only_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"   \"\n";
        assert!(
            parse_config(toml).is_err(),
            "must reject an abbr rule with a whitespace-only expand"
        );
    }

    #[test]
    fn parse_config_accepts_normal_expand() {
        let toml = "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n";
        assert!(
            parse_config(toml).is_ok(),
            "must accept a normal non-empty expand value"
        );
    }

    } // mod expand_validation

    mod add_remove {
        use super::*;

    #[test]
    fn append_abbr_creates_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "version = 1\n").unwrap();

        append_abbr_to_file(&path, "gcm", "git commit -m", None).unwrap();

        let config = load_config(&path).unwrap();
        assert_eq!(config.abbr.len(), 1);
        assert_eq!(config.abbr[0].key, "gcm");
    }

    #[test]
    fn append_abbr_with_when_command_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "version = 1\n").unwrap();

        let cmds = vec!["lsd".to_string()];
        append_abbr_to_file(&path, "ls", "lsd", Some(&cmds)).unwrap();

        let config = load_config(&path).unwrap();
        assert_eq!(config.abbr[0].key, "ls");
        assert!(config.abbr[0].when_command_exists.is_some());
    }

    #[test]
    fn append_abbr_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, r#"version = 1

[[abbr]]
key = "gp"
expand = "git push"
"#).unwrap();

        append_abbr_to_file(&path, "gcm", "git commit -m", None).unwrap();

        let config = load_config(&path).unwrap();
        assert_eq!(config.abbr.len(), 2);
        assert_eq!(config.abbr[0].key, "gp");
        assert_eq!(config.abbr[1].key, "gcm");
    }

    #[test]
    fn append_abbr_rejects_invalid_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "version = 1\n").unwrap();

        assert!(append_abbr_to_file(&path, "", "git commit", None).is_err());
    }

    #[test]
    fn remove_abbr_deletes_matching_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, r#"version = 1

[[abbr]]
key = "gcm"
expand = "git commit -m"

[[abbr]]
key = "gp"
expand = "git push"
"#).unwrap();

        let removed = remove_abbr_from_file(&path, "gcm").unwrap();
        assert_eq!(removed, 1);

        let config = load_config(&path).unwrap();
        assert_eq!(config.abbr.len(), 1);
        assert_eq!(config.abbr[0].key, "gp");
    }

    #[test]
    fn remove_abbr_returns_zero_for_missing_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, r#"version = 1

[[abbr]]
key = "gcm"
expand = "git commit -m"
"#).unwrap();

        let removed = remove_abbr_from_file(&path, "xyz").unwrap();
        assert_eq!(removed, 0);
    }

    } // mod add_remove

    mod validation_walker {
        use super::*;
        use crate::domain::model::{Abbr, PerShellCmds, PerShellString};

        fn make_config(abbrs: Vec<Abbr>) -> Config {
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
            }
        }

        #[test]
        fn collect_issues_empty_for_valid_config() {
            let cfg = make_config(vec![abbr("gcm", "git commit -m")]);
            assert!(collect_validation_issues(&cfg).is_empty());
        }

        #[test]
        fn collect_issues_finds_multiple_rejected_rules() {
            let cfg = make_config(vec![
                abbr("", "not empty"),                // empty key
                abbr("lsa", ""),                      // empty expand
                abbr("valid", "echo ok"),
            ]);
            let issues = collect_validation_issues(&cfg);
            assert_eq!(issues.len(), 2);
            // ordering: rule[0].key then rule[1].expand
            match &issues[0] {
                ValidationIssue::Rule { rule_index, field_path, reason } => {
                    assert_eq!(*rule_index, 1);
                    assert_eq!(field_path, "key");
                    assert_eq!(*reason, ValidationReason::KeyEmpty);
                }
                other => panic!("expected Rule, got {other:?}"),
            }
            match &issues[1] {
                ValidationIssue::Rule { rule_index, field_path, reason } => {
                    assert_eq!(*rule_index, 2);
                    assert_eq!(field_path, "expand");
                    assert_eq!(*reason, ValidationReason::ExpandEmpty);
                }
                other => panic!("expected Rule, got {other:?}"),
            }
        }

        #[test]
        fn collect_issues_reports_per_shell_expand_path() {
            // `default` is valid, `pwsh` is empty.
            let cfg = make_config(vec![Abbr {
                key: "gcm".into(),
                expand: PerShellString::ByShell {
                    default: Some("git commit -m".into()),
                    bash: None,
                    zsh: None,
                    pwsh: Some("".into()),
                    nu: None,
                },
                when_command_exists: None,
            }]);
            let issues = collect_validation_issues(&cfg);
            assert_eq!(issues.len(), 1);
            match &issues[0] {
                ValidationIssue::Rule { rule_index, field_path, reason } => {
                    assert_eq!(*rule_index, 1);
                    assert_eq!(field_path, "expand.pwsh");
                    assert_eq!(*reason, ValidationReason::ExpandEmpty);
                }
                other => panic!("expected Rule, got {other:?}"),
            }
        }

        #[test]
        fn collect_issues_reports_when_command_exists_index_1_based() {
            let cfg = make_config(vec![Abbr {
                key: "ls".into(),
                expand: PerShellString::All("lsd".into()),
                when_command_exists: Some(PerShellCmds::All(vec![
                    "good".into(),
                    "bad&inject".into(),  // metachar at list position 2 (1-based)
                ])),
            }]);
            let issues = collect_validation_issues(&cfg);
            assert_eq!(issues.len(), 1);
            match &issues[0] {
                ValidationIssue::Rule { rule_index, field_path, reason } => {
                    assert_eq!(*rule_index, 1);
                    assert_eq!(field_path, "when_command_exists[2]");
                    assert_eq!(*reason, ValidationReason::CmdContainsMetacharacter);
                }
                other => panic!("expected Rule, got {other:?}"),
            }
        }

        #[test]
        fn collect_issues_reports_per_shell_cmds_path() {
            let cfg = make_config(vec![Abbr {
                key: "ls".into(),
                expand: PerShellString::All("lsd".into()),
                when_command_exists: Some(PerShellCmds::ByShell {
                    default: Some(vec!["ok".into()]),
                    bash: None,
                    zsh: None,
                    pwsh: Some(vec!["Get-Item".into(), "bad|cmd".into()]),
                    nu: None,
                }),
            }]);
            let issues = collect_validation_issues(&cfg);
            assert_eq!(issues.len(), 1);
            match &issues[0] {
                ValidationIssue::Rule { field_path, .. } => {
                    assert_eq!(field_path, "when_command_exists.pwsh[2]");
                }
                other => panic!("expected Rule, got {other:?}"),
            }
        }

        #[test]
        fn first_validation_error_preserves_rule_order() {
            let cfg = make_config(vec![
                abbr("", "x"),       // empty key (rule #1)
                abbr("gcm", ""),     // empty expand (rule #2) — earlier rule wins
            ]);
            let err = first_validation_error(&cfg).expect("must fail");
            assert!(matches!(err, ConfigError::KeyEmpty(1)), "got {err:?}");
        }

        #[test]
        fn first_validation_error_preserves_key_before_expand() {
            let cfg = make_config(vec![abbr("", "")]);
            let err = first_validation_error(&cfg).expect("must fail");
            assert!(matches!(err, ConfigError::KeyEmpty(1)), "got {err:?}");
        }

        #[test]
        fn first_validation_error_preserves_too_many_rules_before_rule_validation() {
            let mut abbrs = Vec::new();
            for _ in 0..=MAX_ABBR_RULES {
                abbrs.push(abbr("", "x")); // every one is invalid
            }
            let cfg = make_config(abbrs);
            let err = first_validation_error(&cfg).expect("must fail");
            assert!(matches!(err, ConfigError::TooManyRules), "got {err:?}");
        }

        #[test]
        fn first_validation_error_preserves_too_many_cmds_before_bad_cmd_entry() {
            let mut cmds = Vec::new();
            for _ in 0..=MAX_CMD_LIST_LEN {
                cmds.push("bad&entry".into()); // every one is invalid
            }
            let cfg = make_config(vec![Abbr {
                key: "ls".into(),
                expand: PerShellString::All("lsd".into()),
                when_command_exists: Some(PerShellCmds::All(cmds)),
            }]);
            let err = first_validation_error(&cfg).expect("must fail");
            assert!(matches!(err, ConfigError::TooManyCmds(1)), "got {err:?}");
        }

        #[test]
        fn first_validation_error_preserves_per_shell_expand_order() {
            // both `default` and `pwsh` are empty → default (index 0) should win.
            let cfg = make_config(vec![Abbr {
                key: "gcm".into(),
                expand: PerShellString::ByShell {
                    default: Some("".into()),
                    bash: None,
                    zsh: None,
                    pwsh: Some("".into()),
                    nu: None,
                },
                when_command_exists: None,
            }]);
            let err = first_validation_error(&cfg).expect("must fail");
            assert!(matches!(err, ConfigError::ExpandEmpty(1)), "got {err:?}");
        }

        #[test]
        fn collect_issues_reports_multiple_issues_in_one_rule_in_order() {
            // Bad key + bad expand.pwsh + bad cmd entry — all three should be reported
            // in key → expand.pwsh → when_command_exists[1] order.
            let cfg = make_config(vec![Abbr {
                key: "".into(),
                expand: PerShellString::ByShell {
                    default: Some("git commit -m".into()),
                    bash: None,
                    zsh: None,
                    pwsh: Some("".into()),
                    nu: None,
                },
                when_command_exists: Some(PerShellCmds::All(vec!["bad&entry".into()])),
            }]);
            let issues = collect_validation_issues(&cfg);
            assert_eq!(issues.len(), 3);
            match &issues[0] {
                ValidationIssue::Rule { field_path, reason, .. } => {
                    assert_eq!(field_path, "key");
                    assert_eq!(*reason, ValidationReason::KeyEmpty);
                }
                other => panic!("expected Rule, got {other:?}"),
            }
            match &issues[1] {
                ValidationIssue::Rule { field_path, reason, .. } => {
                    assert_eq!(field_path, "expand.pwsh");
                    assert_eq!(*reason, ValidationReason::ExpandEmpty);
                }
                other => panic!("expected Rule, got {other:?}"),
            }
            match &issues[2] {
                ValidationIssue::Rule { field_path, reason, .. } => {
                    assert_eq!(field_path, "when_command_exists[1]");
                    assert_eq!(*reason, ValidationReason::CmdContainsMetacharacter);
                }
                other => panic!("expected Rule, got {other:?}"),
            }
        }
    } // mod validation_walker
}
