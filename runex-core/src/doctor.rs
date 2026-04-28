use std::path::Path;

use crate::model::{Config, TriggerKey};
use crate::sanitize::{sanitize_for_display, sanitize_multiline_for_display};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
    /// Full detail shown only with --verbose. None when same as detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_verbose: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagResult {
    pub checks: Vec<Check>,
}

impl DiagResult {
    pub fn is_healthy(&self) -> bool {
        self.checks.iter().all(|c| c.status != CheckStatus::Error)
    }
}

/// Informational facts about the host environment that `runex doctor`
/// surfaces alongside the config-validation checks.
///
/// The struct exists (rather than passing bare values) so future
/// platform diagnostics — XDG paths, registry overrides, shell autodetect
/// hints — can be added without churning every call site.
#[derive(Debug, Clone, Default)]
pub struct DoctorEnvInfo {
    /// Summary of the augmented command-resolution PATH used on Windows.
    /// `None` on non-Windows or when the caller can't compute it. When
    /// present, `diagnose` emits an informational `effective_search_path`
    /// check so a degraded process PATH (clink-style) is visible to the
    /// user before they hit a `command:foo not found` warning.
    pub effective_search_path: Option<EffectiveSearchPathSummary>,

    /// The output `runex export clink` would produce *now*. When
    /// `Some`, `diagnose` compares it against the on-disk `runex.lua`
    /// and warns if the two have drifted — the canonical sign that the
    /// user upgraded runex but never re-ran `runex init clink`. `None`
    /// skips the check (e.g. on platforms without clink, or when the
    /// caller can't render the export).
    pub clink_export_for_drift_check: Option<String>,

    /// Per-shell rcfile marker checks. The caller decides which shells
    /// the user actually has installed; entries set to `true` produce
    /// a `integration:<shell>` row in the doctor output.
    pub check_rcfile_markers: RcfileMarkerSelection,
}

/// Which shells should have their rcfile checked for the runex init
/// marker. The struct exists (rather than a bare `Vec<Shell>`) so
/// future per-shell options (skip-if-missing, custom path overrides)
/// can be added without churning callers.
#[derive(Debug, Clone, Default)]
pub struct RcfileMarkerSelection {
    pub bash: bool,
    pub zsh: bool,
    pub pwsh: bool,
    pub nu: bool,
}

impl RcfileMarkerSelection {
    /// Enable checks for every shell that has an rcfile concept
    /// (i.e. all of them except clink).
    pub fn all() -> Self {
        Self { bash: true, zsh: true, pwsh: true, nu: true }
    }
}

/// Per-source breakdown of the merged PATH `runex hook` actually uses
/// when resolving `when_command_exists` entries.
#[derive(Debug, Clone)]
pub struct EffectiveSearchPathSummary {
    pub from_process: usize,
    pub from_user_registry: usize,
    pub from_system_registry: usize,
}

impl EffectiveSearchPathSummary {
    pub fn total(&self) -> usize {
        self.from_process + self.from_user_registry + self.from_system_registry
    }
}

/// Build the informational `effective_search_path` check.
///
/// Uses `Warn` status only when the process PATH itself is empty
/// (extremely unusual — almost certainly a misconfigured environment),
/// otherwise stays `Ok` and reports the breakdown so users can spot
/// "process=2, +user=42, +system=15" patterns that suggest the parent
/// process was launched with a degraded PATH.
fn check_effective_search_path(s: &EffectiveSearchPathSummary) -> Check {
    let total = s.total();
    let detail = format!(
        "{} entries (process={}, +user={}, +system={})",
        total, s.from_process, s.from_user_registry, s.from_system_registry
    );
    Check {
        name: "effective_search_path".into(),
        status: if s.from_process == 0 {
            // No process PATH at all is a real anomaly — flag it.
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        },
        detail,
        detail_verbose: None,
    }
}

fn check_config_file(config_path: &Path) -> Check {
    let exists = config_path.exists();
    Check {
        name: "config_file".into(),
        status: if exists { CheckStatus::Ok } else { CheckStatus::Error },
        detail: if exists {
            format!("found: {}", sanitize_for_display(&config_path.display().to_string()))
        } else {
            format!("not found: {}", sanitize_for_display(&config_path.display().to_string()))
        },
        detail_verbose: None,
    }
}

fn check_config_parse(config: Option<&Config>, parse_error: Option<&str>) -> Check {
    let (detail, detail_verbose) = if config.is_some() {
        ("config loaded successfully".into(), None)
    } else if let Some(e) = parse_error {
        let first_line = e.lines().next().unwrap_or(e);
        let short = format!("failed to load config: {}", sanitize_for_display(first_line));
        let full = format!("failed to load config: {}", sanitize_multiline_for_display(e));
        let verbose = if full != short { Some(full) } else { None };
        (short, verbose)
    } else {
        ("failed to load config".into(), None)
    };
    Check { name: "config_parse".into(), status: if config.is_some() { CheckStatus::Ok } else { CheckStatus::Error }, detail, detail_verbose }
}

fn check_abbr_quality(config: &Config) -> Vec<Check> {
    let mut checks = Vec::new();
    for (i, abbr) in config.abbr.iter().enumerate() {
        if abbr.key.is_empty() {
            checks.push(Check {
                name: format!("abbr[{i}].empty_key"),
                status: CheckStatus::Warn,
                detail: format!("rule #{n} has an empty key — it will never match", n = i + 1),
                detail_verbose: None,
            });
        }
        // Self-loop: check all expand variants for key == expand.
        let self_loop = abbr.expand.all_values().iter().any(|&v| v == abbr.key);
        if self_loop {
            checks.push(Check {
                name: format!("abbr[{i}].self_loop"),
                status: CheckStatus::Warn,
                detail: format!(
                    "rule #{n} key == expand ('{key}') — this rule is always skipped",
                    n = i + 1,
                    key = sanitize_for_display(&abbr.key)
                ),
                detail_verbose: None,
            });
        }
    }
    checks
}

fn check_when_command_exists<F>(config: &Config, command_exists: &F) -> Vec<Check>
where
    F: Fn(&str) -> bool,
{
    let mut checks = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for abbr in &config.abbr {
        if let Some(cmds) = &abbr.when_command_exists {
            for cmd_list in cmds.all_values() {
                for cmd in cmd_list {
                    // Deduplicate checks for the same command name.
                    if !seen.insert(cmd.clone()) {
                        continue;
                    }
                    let exists = command_exists(cmd);
                    checks.push(Check {
                        name: format!("command:{}", sanitize_for_display(cmd)),
                        status: if exists { CheckStatus::Ok } else { CheckStatus::Warn },
                        detail: if exists {
                            format!(
                                "'{}' found (required by '{}')",
                                sanitize_for_display(cmd),
                                sanitize_for_display(&abbr.key)
                            )
                        } else {
                            format!(
                                "'{}' not found (required by '{}')",
                                sanitize_for_display(cmd),
                                sanitize_for_display(&abbr.key)
                            )
                        },
                        detail_verbose: None,
                    });
                }
            }
        }
    }
    checks
}

fn check_keybind(config: &Config) -> Vec<Check> {
    let mut checks = Vec::new();
    let si = &config.keybind.self_insert;
    let bash_si = si.bash.or(si.default);
    let zsh_si = si.zsh.or(si.default);
    if bash_si == Some(TriggerKey::ShiftSpace) || zsh_si == Some(TriggerKey::ShiftSpace) {
        checks.push(Check {
            name: "keybind.self_insert".into(),
            status: CheckStatus::Warn,
            detail:
                "self_insert = \"shift-space\" has no effect in bash/zsh (Shift+Space is terminal-dependent); use \"alt-space\" for cross-shell support".into(),
            detail_verbose: None,
        });
    }
    checks
}

/// Levenshtein distance between two strings (for "did you mean?" suggestions).
fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Find the closest match from `candidates` for `name` (Levenshtein distance ≤ 2).
fn suggest_similar(name: &str, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .filter_map(|&c| {
            let d = levenshtein(name, c);
            if d <= 2 && d > 0 { Some((c, d)) } else { None }
        })
        .min_by_key(|&(_, d)| d)
        .map(|(c, _)| c.to_string())
}

/// Known top-level TOML keys in config.
const KNOWN_TOP_LEVEL_KEYS: &[&str] = &["version", "keybind", "precache", "abbr"];

/// Known keys inside an `[[abbr]]` table.
const KNOWN_ABBR_KEYS: &[&str] = &["key", "expand", "when_command_exists"];

/// Known keys inside `[keybind]`.
const KNOWN_KEYBIND_KEYS: &[&str] = &["trigger", "self_insert"];

/// Known keys inside a keybind subtable (e.g. `[keybind.trigger]`).
const KNOWN_KEYBIND_SUB_KEYS: &[&str] = &["default", "bash", "zsh", "pwsh", "nu"];

/// Known keys inside `[precache]`.
const KNOWN_PRECACHE_KEYS: &[&str] = &["path_only"];

/// Check for unknown fields in the raw TOML source (strict mode).
///
/// Parses the config as a raw TOML table and compares keys against whitelists.
/// Returns Warn checks for each unknown field, with "did you mean?" suggestions.
/// Check for rules rejected by per-field validation.
///
/// Unlike `check_config_parse`, which surfaces only the first `ConfigError`
/// from `parse_config`, this walks every rule and reports *all* validation
/// failures with field-path diagnostics (e.g. `abbr[3].expand.pwsh`).
///
/// Config loading still stops at the first error — these warnings are
/// observability only, not a lenient-load mode.
pub fn check_rejected_rules(config_source: &str) -> Vec<Check> {
    // If deserialization fails (syntax / unsupported version), check_config_parse
    // already reports it. Emit nothing here.
    let Ok(config) = crate::config::parse_config_lenient(config_source) else {
        return vec![];
    };
    let issues = crate::config::collect_validation_issues(&config);
    if issues.is_empty() {
        return vec![];
    }

    let mut checks = Vec::with_capacity(issues.len() + 1);

    // Summary check first, so the reader sees the semantics before the details.
    checks.push(Check {
        name: "config_rejected_rules".into(),
        status: CheckStatus::Warn,
        detail: format!(
            "{} invalid abbr field(s) found; config loading still stops at the first one",
            issues.len()
        ),
        detail_verbose: None,
    });

    for issue in issues {
        let check = match &issue {
            crate::config::ValidationIssue::Config { .. } => Check {
                name: "config_validation".into(),
                status: CheckStatus::Warn,
                detail: format!("config rejected: {}", issue.reason_text()),
                detail_verbose: None,
            },
            crate::config::ValidationIssue::Rule { rule_index, field_path, .. } => {
                let safe_path = sanitize_for_display(field_path);
                Check {
                    name: format!("config_validation.abbr[{rule_index}].{safe_path}"),
                    status: CheckStatus::Warn,
                    detail: format!(
                        "rule #{rule_index} field '{safe_path}' rejected: {}",
                        issue.reason_text(),
                    ),
                    detail_verbose: None,
                }
            }
        };
        checks.push(check);
    }
    checks
}

/// Strict-mode check: warn when the deprecated `[precache]` section is
/// present in the config. The section was used by the legacy shell template
/// to emit a startup precache helper; the hook-based bootstraps consult the
/// config at keypress time so the section now has no run-time effect.
///
/// Returns an empty vec when the TOML is unparseable; that case is reported
/// by `check_config_parse` already.
pub fn check_precache_deprecation(config_source: &str) -> Vec<Check> {
    let table: toml::Table = match config_source.parse() {
        Ok(t) => t,
        Err(_) => return vec![],
    };
    if !table.contains_key("precache") {
        return vec![];
    }
    vec![Check {
        name: "precache_deprecation".into(),
        status: CheckStatus::Warn,
        detail: "[precache] is deprecated and has no effect since the shell integration moved to runtime hook calls. Remove the section to silence this warning.".into(),
        detail_verbose: None,
    }]
}

pub fn check_unknown_fields(config_source: &str) -> Vec<Check> {
    let table: toml::Table = match config_source.parse() {
        Ok(t) => t,
        Err(_) => return vec![], // parse errors are caught by check_config_parse
    };

    let mut checks = Vec::new();

    // Top-level keys
    for key in table.keys() {
        if !KNOWN_TOP_LEVEL_KEYS.contains(&key.as_str()) {
            let suggestion = suggest_similar(key, KNOWN_TOP_LEVEL_KEYS);
            let detail = match suggestion {
                Some(s) => format!("unknown top-level field '{}' (did you mean '{}'?)", sanitize_for_display(key), s),
                None => format!("unknown top-level field '{}'", sanitize_for_display(key)),
            };
            checks.push(Check {
                name: format!("strict.unknown_field.{}", sanitize_for_display(key)),
                status: CheckStatus::Warn,
                detail,
                detail_verbose: None,
            });
        }
    }

    // [keybind] subtable keys
    if let Some(toml::Value::Table(kb)) = table.get("keybind") {
        for key in kb.keys() {
            if !KNOWN_KEYBIND_KEYS.contains(&key.as_str()) {
                let suggestion = suggest_similar(key, KNOWN_KEYBIND_KEYS);
                let detail = match suggestion {
                    Some(s) => format!("unknown keybind field '{}' (did you mean '{}'?)", sanitize_for_display(key), s),
                    None => format!("unknown keybind field '{}'", sanitize_for_display(key)),
                };
                checks.push(Check {
                    name: format!("strict.unknown_field.keybind.{}", sanitize_for_display(key)),
                    status: CheckStatus::Warn,
                    detail,
                    detail_verbose: None,
                });
            } else if let Some(toml::Value::Table(sub)) = kb.get(key) {
                // Check keybind subtable keys (e.g. [keybind.trigger])
                for sub_key in sub.keys() {
                    if !KNOWN_KEYBIND_SUB_KEYS.contains(&sub_key.as_str()) {
                        let suggestion = suggest_similar(sub_key, KNOWN_KEYBIND_SUB_KEYS);
                        let detail = match suggestion {
                            Some(s) => format!("unknown keybind.{} field '{}' (did you mean '{}'?)", key, sanitize_for_display(sub_key), s),
                            None => format!("unknown keybind.{} field '{}'", key, sanitize_for_display(sub_key)),
                        };
                        checks.push(Check {
                            name: format!("strict.unknown_field.keybind.{}.{}", key, sanitize_for_display(sub_key)),
                            status: CheckStatus::Warn,
                            detail,
                            detail_verbose: None,
                        });
                    }
                }
            }
        }
    }

    // [precache] keys
    if let Some(toml::Value::Table(pc)) = table.get("precache") {
        for key in pc.keys() {
            if !KNOWN_PRECACHE_KEYS.contains(&key.as_str()) {
                let suggestion = suggest_similar(key, KNOWN_PRECACHE_KEYS);
                let detail = match suggestion {
                    Some(s) => format!("unknown precache field '{}' (did you mean '{}'?)", sanitize_for_display(key), s),
                    None => format!("unknown precache field '{}'", sanitize_for_display(key)),
                };
                checks.push(Check {
                    name: format!("strict.unknown_field.precache.{}", sanitize_for_display(key)),
                    status: CheckStatus::Warn,
                    detail,
                    detail_verbose: None,
                });
            }
        }
    }

    // [[abbr]] entries
    if let Some(toml::Value::Array(abbrs)) = table.get("abbr") {
        for (i, entry) in abbrs.iter().enumerate() {
            if let toml::Value::Table(abbr_table) = entry {
                for key in abbr_table.keys() {
                    if !KNOWN_ABBR_KEYS.contains(&key.as_str()) {
                        let suggestion = suggest_similar(key, KNOWN_ABBR_KEYS);
                        let detail = match suggestion {
                            Some(s) => format!(
                                "unknown field '{}' in abbr[{}] (did you mean '{}'?)",
                                sanitize_for_display(key), i + 1, s
                            ),
                            None => format!("unknown field '{}' in abbr[{}]", sanitize_for_display(key), i + 1),
                        };
                        checks.push(Check {
                            name: format!("strict.unknown_field.abbr[{}].{}", i, sanitize_for_display(key)),
                            status: CheckStatus::Warn,
                            detail,
                            detail_verbose: None,
                        });
                    }
                }
            }
        }
    }

    checks
}

/// Check for unreachable duplicate rules (strict mode).
///
/// A rule is unreachable if an earlier rule with the same key has no
/// `when_command_exists` condition — it will always match first, making
/// all later rules with that key dead code.
pub fn check_unreachable_duplicates(config: &Config) -> Vec<Check> {
    let mut checks = Vec::new();
    // Track keys where an unconditional rule has been seen.
    let mut unconditional_keys: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();

    for (i, abbr) in config.abbr.iter().enumerate() {
        if let Some(&first_rule) = unconditional_keys.get(abbr.key.as_str()) {
            // A previous unconditional rule already matches this key.
            checks.push(Check {
                name: format!("strict.unreachable.abbr[{}]", i),
                status: CheckStatus::Warn,
                detail: format!(
                    "rule #{} ('{}') is unreachable — rule #{} has the same key with no condition and always matches first",
                    i + 1,
                    sanitize_for_display(&abbr.key),
                    first_rule + 1,
                ),
                detail_verbose: None,
            });
        } else if abbr.when_command_exists.is_none() {
            // This is an unconditional rule — record it.
            unconditional_keys.insert(&abbr.key, i);
        }
    }
    checks
}

/// Run environment diagnostics.
///
/// `config` is `None` when config loading failed (parse error, etc.).
/// `parse_error` carries the error message when `config` is `None` due to a parse failure.
/// `command_exists` is injected for testability.
pub fn diagnose<F>(
    config_path: &Path,
    config: Option<&Config>,
    parse_error: Option<&str>,
    env_info: &DoctorEnvInfo,
    command_exists: F,
) -> DiagResult
where
    F: Fn(&str) -> bool,
{
    let mut checks = Vec::new();
    checks.push(check_config_file(config_path));
    checks.push(check_config_parse(config, parse_error));
    if let Some(summary) = env_info.effective_search_path.as_ref() {
        // Emit before the per-command checks so users see the search PATH
        // context above any "command:foo not found" warnings that may
        // follow.
        checks.push(check_effective_search_path(summary));
    }
    checks.extend(integration_marker_checks(&env_info.check_rcfile_markers));
    if let Some(export) = env_info.clink_export_for_drift_check.as_deref() {
        let r = crate::integration_check::check_clink_lua_freshness(
            export,
            &crate::integration_check::default_clink_lua_paths(),
        );
        checks.push(integration_check_to_check(r));
    }
    if let Some(cfg) = config {
        checks.extend(check_keybind(cfg));
        checks.extend(check_abbr_quality(cfg));
        checks.extend(check_when_command_exists(cfg, &command_exists));
    }
    DiagResult { checks }
}

/// Convert an [`integration_check::IntegrationCheck`] into the doctor
/// `Check` shape. `Outdated` becomes `Warn`, `Missing` becomes `Warn`
/// (we don't escalate to Error: a stale or missing rcfile shouldn't
/// fail `doctor` outright — the user's shell still works).
fn integration_check_to_check(r: crate::integration_check::IntegrationCheck) -> Check {
    use crate::integration_check::IntegrationCheck;
    let (status, name, detail) = match r {
        IntegrationCheck::Ok { name, detail } => (CheckStatus::Ok, name, detail),
        IntegrationCheck::Outdated { name, detail, .. } => (CheckStatus::Warn, name, detail),
        IntegrationCheck::Missing { name, detail } => (CheckStatus::Warn, name, detail),
        IntegrationCheck::Skipped { name, detail } => (CheckStatus::Ok, name, detail),
    };
    Check { name, status, detail, detail_verbose: None }
}

/// Run the rcfile-marker check for each shell selected by the caller.
fn integration_marker_checks(sel: &RcfileMarkerSelection) -> Vec<Check> {
    use crate::integration_check::check_rcfile_marker;
    use crate::shell::Shell;
    let mut out = Vec::new();
    if sel.bash {
        out.push(integration_check_to_check(check_rcfile_marker(Shell::Bash, None)));
    }
    if sel.zsh {
        out.push(integration_check_to_check(check_rcfile_marker(Shell::Zsh, None)));
    }
    if sel.pwsh {
        out.push(integration_check_to_check(check_rcfile_marker(Shell::Pwsh, None)));
    }
    if sel.nu {
        out.push(integration_check_to_check(check_rcfile_marker(Shell::Nu, None)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Abbr, Config};
    use std::io::Write;

    fn test_config(abbrs: Vec<Abbr>) -> Config {
        Config {
            version: 1,
            keybind: crate::model::KeybindConfig::default(),
            precache: crate::model::PrecacheConfig::default(),
            abbr: abbrs,
        }
    }

    fn abbr_when(key: &str, exp: &str, cmds: Vec<&str>) -> Abbr {
        Abbr {
            key: key.into(),
            expand: crate::model::PerShellString::All(exp.into()),
            when_command_exists: Some(crate::model::PerShellCmds::All(
                cmds.into_iter().map(String::from).collect(),
            )),
        }
    }

    fn abbr(key: &str, exp: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: crate::model::PerShellString::All(exp.into()),
            when_command_exists: None,
        }
    }

    mod diagnostics {
        use super::*;

    #[test]
    fn all_healthy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "version = 1").unwrap();

        let cfg = test_config(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| true);

        assert!(result.is_healthy());
        assert_eq!(result.checks[0].status, CheckStatus::Ok); // file exists
        assert_eq!(result.checks[1].status, CheckStatus::Ok); // config parsed
        assert_eq!(result.checks[2].status, CheckStatus::Ok); // command found
    }

    #[test]
    fn config_file_missing() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let result = diagnose(&path, None, None, &DoctorEnvInfo::default(), |_| true);

        assert!(!result.is_healthy());
        assert_eq!(result.checks[0].status, CheckStatus::Error);
        assert_eq!(result.checks[1].status, CheckStatus::Error);
    }

    #[test]
    fn config_parse_error_detail_shown() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let result = diagnose(&path, None, Some("TOML parse error at line 4"), &DoctorEnvInfo::default(), |_| true);

        let parse_check = result.checks.iter().find(|c| c.name == "config_parse").unwrap();
        assert_eq!(parse_check.status, CheckStatus::Error);
        assert!(parse_check.detail.contains("TOML parse error at line 4"),
            "detail must include the parse error message: {:?}", parse_check.detail);
    }

    #[test]
    fn config_parse_multiline_error_splits_detail_and_verbose() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let multiline = "TOML parse error at line 4, column 11\n  |\n4 | trigger = \"space\"\n  |           ^^^^^^^\ninvalid type";
        let result = diagnose(&path, None, Some(multiline), &DoctorEnvInfo::default(), |_| true);

        let parse_check = result.checks.iter().find(|c| c.name == "config_parse").unwrap();
        assert_eq!(parse_check.status, CheckStatus::Error);

        let detail_lines: Vec<&str> = parse_check.detail.lines().collect();
        assert_eq!(detail_lines.len(), 1,
            "detail must be a single line, got: {:?}", parse_check.detail);
        assert!(parse_check.detail.contains("TOML parse error at line 4, column 11"),
            "detail must contain the first line: {:?}", parse_check.detail);

        let verbose = parse_check.detail_verbose.as_deref()
            .expect("detail_verbose must be Some for multiline errors");
        assert!(verbose.contains("invalid type"),
            "detail_verbose must contain later lines: {:?}", verbose);
    }

    #[test]
    fn config_parse_single_line_error_has_no_verbose() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let result = diagnose(&path, None, Some("unsupported version: 99"), &DoctorEnvInfo::default(), |_| true);

        let parse_check = result.checks.iter().find(|c| c.name == "config_parse").unwrap();
        assert!(parse_check.detail_verbose.is_none(),
            "detail_verbose must be None when error is single-line: {:?}", parse_check.detail_verbose);
    }

    #[test]
    fn command_not_found_is_warn() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "version = 1").unwrap();

        let cfg = test_config(vec![abbr_when("ls", "lsd", vec!["lsd"])]);
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| false);

        assert!(result.is_healthy());
        assert_eq!(result.checks[2].status, CheckStatus::Warn);
        assert!(result.checks[2].detail.contains("not found"));
    }

    #[test]
    fn doctor_warns_empty_key() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![abbr("", "git commit -m")]);
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| true);
        assert!(
            result.checks.iter().any(|c| c.name.contains("empty_key") && c.status == CheckStatus::Warn),
            "must warn on empty key: {:?}", result.checks
        );
    }

    #[test]
    fn doctor_warns_self_loop() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![abbr("ls", "ls")]);
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| true);
        assert!(
            result.checks.iter().any(|c| c.name.contains("self_loop") && c.status == CheckStatus::Warn),
            "must warn on self-loop: {:?}", result.checks
        );
    }

    #[test]
    fn diag_result_is_healthy_with_error() {
        let result = DiagResult {
            checks: vec![Check {
                name: "test".into(),
                status: CheckStatus::Error,
                detail: "bad".into(),
                detail_verbose: None,
            }],
        };
        assert!(!result.is_healthy());
    }

    #[test]
    fn doctor_warns_shift_space_self_insert() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    bash: Some(crate::model::TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| true);
        assert!(
            result.checks.iter().any(|c| c.name == "keybind.self_insert" && c.status == CheckStatus::Warn),
            "must warn when self_insert.bash = shift-space: {:?}", result.checks
        );
    }

    #[test]
    fn doctor_ok_alt_space_self_insert() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    pwsh: Some(crate::model::TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| true);
        assert!(
            !result.checks.iter().any(|c| c.name == "keybind.self_insert" && c.status == CheckStatus::Warn),
            "must not warn when only self_insert.pwsh = shift-space: {:?}", result.checks
        );
    }

    #[test]
    fn doctor_warns_when_default_self_insert_is_shift_space() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    default: Some(crate::model::TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| true);
        assert!(
            result.checks.iter().any(|c| c.name == "keybind.self_insert" && c.status == CheckStatus::Warn),
            "must warn when default self_insert = shift-space (propagates to bash/zsh): {:?}", result.checks
        );
    }

    #[test]
    fn doctor_ok_when_only_pwsh_self_insert_is_shift_space() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = Config {
            version: 1,
            keybind: crate::model::KeybindConfig {
                self_insert: crate::model::PerShellKey {
                    pwsh: Some(crate::model::TriggerKey::ShiftSpace),
                    ..Default::default()
                },
                ..crate::model::KeybindConfig::default()
            },
            precache: crate::model::PrecacheConfig::default(),
            abbr: vec![],
        };
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| true);
        assert!(
            !result.checks.iter().any(|c| c.name == "keybind.self_insert" && c.status == CheckStatus::Warn),
            "must not warn when only pwsh self_insert = shift-space: {:?}", result.checks
        );
    }

    } // mod diagnostics

    /// Detail strings embed user-controlled values (keys, cmd names, config paths).
    /// If these contain ANSI escape sequences or other control characters, they will
    /// be printed raw to the terminal — enabling screen clearing, cursor movement,
    /// or other terminal injection attacks. All detail and name fields must be sanitized.
    mod sanitization {
        use super::*;

    /// A key containing a BEL (\x07) control character (valid TOML via \uXXXX escape)
    /// must not appear raw in the detail string, which is printed to the terminal.
    #[test]
    fn doctor_self_loop_detail_strips_control_chars_from_key() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![abbr("key\x07evil", "key\x07evil")]);
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| true);
        let self_loop = result.checks.iter().find(|c| c.name.contains("self_loop"));
        let check = self_loop.expect("must produce a self_loop check for a self-loop key");
        assert!(
            !check.detail.contains('\x07'),
            "detail must not contain raw control char BEL: {:?}", check.detail
        );
    }

    /// A cmd in `when_command_exists` containing a control char must not appear raw in detail.
    #[test]
    fn doctor_command_check_detail_strips_control_chars_from_cmd() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![crate::model::Abbr {
            key: "ls".into(),
            expand: crate::model::PerShellString::All("lsd".into()),
            when_command_exists: Some(crate::model::PerShellCmds::All(vec!["cmd\x07inject".into()])),
        }]);
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| false);
        let cmd_check = result.checks.iter().find(|c| c.name.contains("command:"));
        let check = cmd_check.expect("must produce a command check");
        assert!(
            !check.detail.contains('\x07'),
            "detail must not contain raw control char from cmd: {:?}", check.detail
        );
    }

    /// `--config` path containing ANSI escape sequences must not appear raw in
    /// the `config_file` check detail. Attack: a path with ESC sequences could clear the screen.
    #[test]
    fn doctor_config_file_detail_strips_control_chars_from_path() {
        let path = std::path::PathBuf::from("/home/user/\x1b[2Jevil.toml");
        let result = diagnose(&path, None, None, &DoctorEnvInfo::default(), |_| true);
        let config_check = result.checks.iter().find(|c| c.name == "config_file");
        let check = config_check.expect("must produce a config_file check");
        assert!(
            !check.detail.contains('\x1b'),
            "config_file detail must not contain raw ESC from path: {:?}", check.detail
        );
    }

    /// The `name` field `"command:{cmd}"` is printed to the terminal.
    /// A cmd containing ANSI escape sequences (e.g. ESC+`[2J` = clear screen) must
    /// not appear raw in `check.name` — sanitized the same way as `detail`.
    #[test]
    fn doctor_command_check_name_strips_control_chars() {
        let path = std::path::PathBuf::from("/nonexistent/config.toml");
        let cfg = test_config(vec![crate::model::Abbr {
            key: "ls".into(),
            expand: crate::model::PerShellString::All("lsd".into()),
            when_command_exists: Some(crate::model::PerShellCmds::All(vec!["cmd\x1b[2Jevil".into()])),
        }]);
        let result = diagnose(&path, Some(&cfg), None, &DoctorEnvInfo::default(), |_| false);
        let cmd_check = result.checks.iter().find(|c| c.name.starts_with("command:"));
        let check = cmd_check.expect("must produce a command check");
        assert!(
            !check.name.contains('\x1b'),
            "check.name must not contain raw ESC (ANSI injection risk): {:?}", check.name
        );
    }

    } // mod sanitization

    mod strict {
        use super::*;

    #[test]
    fn check_unknown_top_level_field() {
        let toml = r#"
version = 1
abr = "typo"
"#;
        let checks = check_unknown_fields(toml);
        assert!(
            checks.iter().any(|c| c.detail.contains("abr") && c.detail.contains("did you mean 'abbr'")),
            "must detect 'abr' typo: {:?}", checks
        );
    }

    #[test]
    fn check_unknown_abbr_field() {
        let toml = r#"
version = 1
[[abbr]]
key = "gcm"
expad = "git commit -m"
"#;
        let checks = check_unknown_fields(toml);
        assert!(
            checks.iter().any(|c| c.detail.contains("expad") && c.detail.contains("did you mean 'expand'")),
            "must detect 'expad' typo: {:?}", checks
        );
    }

    #[test]
    fn check_no_warnings_for_valid_config() {
        let toml = r#"
version = 1
[keybind.trigger]
default = "space"
[[abbr]]
key = "gcm"
expand = "git commit -m"
when_command_exists = ["git"]
"#;
        let checks = check_unknown_fields(toml);
        assert!(checks.is_empty(), "valid config must produce no warnings: {:?}", checks);
    }

    #[test]
    fn precache_deprecation_warns_when_section_is_present() {
        let toml = r#"
version = 1
[precache]
path_only = true
"#;
        let checks = check_precache_deprecation(toml);
        assert_eq!(checks.len(), 1, "should warn once when [precache] is present: {:?}", checks);
        assert_eq!(checks[0].status, CheckStatus::Warn);
        assert!(checks[0].detail.contains("deprecated"), "detail must say deprecated: {}", checks[0].detail);
        assert_eq!(checks[0].name, "precache_deprecation");
    }

    #[test]
    fn precache_deprecation_silent_when_section_absent() {
        let toml = r#"
version = 1
[[abbr]]
key = "gcm"
expand = "git commit -m"
"#;
        let checks = check_precache_deprecation(toml);
        assert!(checks.is_empty(), "no warning when [precache] is absent: {:?}", checks);
    }

    #[test]
    fn precache_deprecation_silent_when_toml_invalid() {
        // If the TOML doesn't even parse, check_config_parse handles it.
        let checks = check_precache_deprecation("this is not [valid toml");
        assert!(checks.is_empty());
    }

    #[test]
    fn check_unknown_keybind_field() {
        let toml = r#"
version = 1
[keybind]
trigerr = "space"
"#;
        let checks = check_unknown_fields(toml);
        assert!(
            checks.iter().any(|c| c.detail.contains("trigerr") && c.detail.contains("did you mean 'trigger'")),
            "must detect 'trigerr' typo: {:?}", checks
        );
    }

    #[test]
    fn suggest_similar_field_name() {
        assert_eq!(suggest_similar("abr", KNOWN_TOP_LEVEL_KEYS), Some("abbr".to_string()));
        assert_eq!(suggest_similar("expad", KNOWN_ABBR_KEYS), Some("expand".to_string()));
        assert_eq!(suggest_similar("xyz_completely_different", KNOWN_TOP_LEVEL_KEYS), None);
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("abr", "abbr"), 1);
        assert_eq!(levenshtein("expad", "expand"), 1);
    }

    #[test]
    fn check_duplicate_key_without_condition() {
        let cfg = test_config(vec![
            abbr("gcm", "git commit -m"),
            abbr("gcm", "git checkout main"),
        ]);
        let checks = check_unreachable_duplicates(&cfg);
        assert_eq!(checks.len(), 1);
        assert!(checks[0].detail.contains("gcm"), "must mention the key: {:?}", checks[0].detail);
        assert!(checks[0].detail.contains("unreachable"), "must say unreachable: {:?}", checks[0].detail);
    }

    #[test]
    fn check_duplicate_key_with_condition_is_ok() {
        let cfg = test_config(vec![
            abbr_when("ls", "lsd", vec!["lsd"]),
            abbr("ls", "ls --color=auto"),
        ]);
        let checks = check_unreachable_duplicates(&cfg);
        assert!(checks.is_empty(), "fallback chain should not warn: {:?}", checks);
    }

    #[test]
    fn check_duplicate_key_condition_then_no_condition_is_ok() {
        let cfg = test_config(vec![
            abbr_when("ls", "lsd", vec!["lsd"]),
            abbr_when("ls", "eza", vec!["eza"]),
            abbr("ls", "ls --color=auto"),
        ]);
        let checks = check_unreachable_duplicates(&cfg);
        assert!(checks.is_empty(), "all-conditional + one fallback should not warn: {:?}", checks);
    }

    #[test]
    fn check_no_condition_blocks_later_rules() {
        let cfg = test_config(vec![
            abbr("gcm", "git commit -m"),       // unconditional — always matches
            abbr_when("gcm", "git cm", vec!["git"]),  // unreachable
        ]);
        let checks = check_unreachable_duplicates(&cfg);
        assert_eq!(checks.len(), 1);
        assert!(checks[0].detail.contains("#2"), "must mention the rule number: {:?}", checks[0].detail);
    }

    } // mod strict

    mod rejected_rules {
        use super::*;

        #[test]
        fn check_rejected_rules_empty_for_valid_config() {
            let toml = r#"
version = 1
[[abbr]]
key = "gcm"
expand = "git commit -m"
"#;
            assert!(check_rejected_rules(toml).is_empty());
        }

        #[test]
        fn check_rejected_rules_emits_summary_check_first() {
            let toml = r#"
version = 1
[[abbr]]
key = ""
expand = "x"
[[abbr]]
key = "ls"
expand = ""
"#;
            let checks = check_rejected_rules(toml);
            assert!(!checks.is_empty());
            assert_eq!(checks[0].name, "config_rejected_rules");
            assert!(checks[0].detail.contains("2 invalid"), "summary count: {:?}", checks[0].detail);
            // Remaining are per-field warns, sorted by rule order.
            assert!(checks[1].name.starts_with("config_validation.abbr[1]."));
            assert!(checks[2].name.starts_with("config_validation.abbr[2]."));
        }

        #[test]
        fn check_rejected_rules_warns_for_each_bad_rule() {
            let toml = r#"
version = 1
[[abbr]]
key = ""
expand = "something"
[[abbr]]
key = "lsa"
expand = ""
[[abbr]]
key = "valid"
expand = "echo ok"
when_command_exists = ["good", "bad&inject"]
"#;
            let checks = check_rejected_rules(toml);
            // 1 summary + 3 per-field = 4
            assert_eq!(checks.len(), 4, "expected 1 summary + 3 warns: {checks:?}");
            assert_eq!(checks[1].name, "config_validation.abbr[1].key");
            assert_eq!(checks[2].name, "config_validation.abbr[2].expand");
            assert_eq!(checks[3].name, "config_validation.abbr[3].when_command_exists[2]");
        }

        #[test]
        fn check_rejected_rules_does_not_leak_raw_values() {
            // Key contains a BEL control character. The check must not echo it.
            let toml = "
version = 1
[[abbr]]
key = \"gc\\u0007m\"
expand = \"x\"
";
            let checks = check_rejected_rules(toml);
            assert!(!checks.is_empty());
            for check in &checks {
                assert!(
                    !check.detail.contains('\x07'),
                    "raw BEL must not appear in detail: {:?}",
                    check.detail
                );
                assert!(
                    !check.name.contains('\x07'),
                    "raw BEL must not appear in name: {:?}",
                    check.name
                );
            }
        }

        #[test]
        fn check_rejected_rules_skips_when_deserialization_fails() {
            // Invalid TOML syntax — check_config_parse handles this.
            let toml = "this is = not [ valid toml";
            assert!(check_rejected_rules(toml).is_empty());
        }

        #[test]
        fn check_rejected_rules_skips_when_unsupported_version() {
            let toml = r#"version = 99
[[abbr]]
key = ""
expand = "x"
"#;
            assert!(check_rejected_rules(toml).is_empty());
        }
    } // mod rejected_rules
}
