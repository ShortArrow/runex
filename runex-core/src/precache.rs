use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::Config;

/// Environment variable name for the command existence cache.
pub const CACHE_ENV_VAR: &str = "RUNEX_CMD_CACHE_V1";

/// Current cache format version.
const CACHE_VERSION: u32 = 1;

/// Maximum byte length of the raw JSON env var value. Prevents memory/CPU DoS
/// from a maliciously large `RUNEX_CMD_CACHE_V1`. 256 KiB is generous for any
/// realistic config (10 000 rules × ~25 bytes/entry ≈ 250 KB).
const MAX_CACHE_BYTES: usize = 256 * 1024;

/// Maximum number of entries in the `commands` map. Mirrors `MAX_ABBR_RULES`
/// in config validation — a cache should never have more entries than there
/// are abbreviation rules.
const MAX_CACHE_COMMANDS: usize = 10_000;

/// Expected length of a fingerprint hex string (16 hex chars from u64).
const FINGERPRINT_LEN: usize = 16;

/// Serialized command existence cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdCache {
    pub v: u32,
    pub fingerprint: String,
    pub commands: HashMap<String, bool>,
}

/// Compute a fingerprint from PATH, config mtime, and shell name.
///
/// Uses a fast non-cryptographic hash — this is for staleness detection,
/// not tamper resistance. An attacker who can modify the env var can also
/// modify PATH itself.
pub fn compute_fingerprint(path_env: &str, config_mtime: u64, shell: &str) -> String {
    let mut hasher = DefaultHasher::new();
    path_env.hash(&mut hasher);
    config_mtime.hash(&mut hasher);
    shell.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Collect all unique command names referenced by `when_command_exists`
/// across all abbreviation rules in the config.
pub fn collect_unique_commands(config: &Config) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for abbr in &config.abbr {
        if let Some(cmds) = &abbr.when_command_exists {
            for cmd_list in cmds.all_values() {
                for cmd in cmd_list {
                    if seen.insert(cmd.clone()) {
                        result.push(cmd.clone());
                    }
                }
            }
        }
    }
    result
}

/// Build a cache by checking each command with the provided closure.
pub fn build_cache<F>(
    config: &Config,
    fingerprint: &str,
    command_exists: F,
) -> CmdCache
where
    F: Fn(&str) -> bool,
{
    let cmds = collect_unique_commands(config);
    let mut commands = HashMap::new();
    for cmd in cmds {
        commands.insert(cmd.clone(), command_exists(&cmd));
    }
    CmdCache {
        v: CACHE_VERSION,
        fingerprint: fingerprint.to_string(),
        commands,
    }
}

/// Serialize a cache to JSON.
pub fn cache_to_json(cache: &CmdCache) -> String {
    serde_json::to_string(cache).unwrap_or_default()
}

/// Parse a cache from JSON, returning None on any failure.
///
/// Rejects inputs that are too large, have too many command entries, use an
/// unexpected version, or have a malformed fingerprint. This is a
/// defense-in-depth measure — the cache is untrusted input from an
/// environment variable.
pub fn parse_cache(json: &str) -> Option<CmdCache> {
    if json.len() > MAX_CACHE_BYTES {
        return None;
    }
    let cache: CmdCache = serde_json::from_str(json).ok()?;
    if cache.v != CACHE_VERSION {
        return None;
    }
    if cache.fingerprint.len() != FINGERPRINT_LEN
        || !cache.fingerprint.chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    if cache.commands.len() > MAX_CACHE_COMMANDS {
        return None;
    }
    Some(cache)
}

/// Load and validate a cache from the environment variable.
///
/// Returns None if:
/// - env var is absent or empty
/// - JSON is malformed
/// - version != 1
/// - fingerprint does not match expected
pub fn load_cache(expected_fingerprint: &str) -> Option<CmdCache> {
    let json = std::env::var(CACHE_ENV_VAR).ok()?;
    let cache = parse_cache(&json)?;
    if cache.fingerprint != expected_fingerprint {
        return None;
    }
    Some(cache)
}

/// Get the mtime of a config file as seconds since epoch.
/// Returns 0 if the file doesn't exist or metadata can't be read.
pub fn config_mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Generate a shell export statement for the cache.
pub fn export_statement(shell: &str, cache_json: &str) -> String {
    match shell {
        "bash" | "zsh" => {
            let escaped = cache_json.replace('\'', "'\\''");
            format!("export {}='{}'", CACHE_ENV_VAR, escaped)
        }
        "pwsh" => {
            let escaped = cache_json.replace('\'', "''");
            format!("$env:{}='{}'", CACHE_ENV_VAR, escaped)
        }
        "nu" => {
            let escaped = cache_json.replace('\'', "''");
            format!("$env.{} = '{}'", CACHE_ENV_VAR, escaped)
        }
        "clink" => {
            // cmd.exe set command — no quotes around value for os.execute
            let escaped = cache_json.replace('"', "\\\"");
            format!("set {}={}", CACHE_ENV_VAR, escaped)
        }
        _ => {
            let escaped = cache_json.replace('\'', "'\\''");
            format!("export {}='{}'", CACHE_ENV_VAR, escaped)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Abbr, Config, KeybindConfig, PerShellCmds, PerShellString};

    fn test_config(abbrs: Vec<Abbr>) -> Config {
        Config {
            version: 1,
            keybind: KeybindConfig::default(),
            abbr: abbrs,
        }
    }

    fn abbr_when(key: &str, exp: &str, cmds: Vec<&str>) -> Abbr {
        Abbr {
            key: key.into(),
            expand: PerShellString::All(exp.into()),
            when_command_exists: Some(PerShellCmds::All(
                cmds.into_iter().map(String::from).collect(),
            )),
        }
    }

    #[test]
    fn cache_roundtrip() {
        let config = test_config(vec![
            abbr_when("ls", "lsd", vec!["lsd"]),
            abbr_when("7z", "7zip", vec!["7z"]),
        ]);
        let fp = compute_fingerprint("/usr/bin:/bin", 1234567890, "bash");
        let cache = build_cache(&config, &fp, |cmd| cmd == "lsd");

        let json = cache_to_json(&cache);
        let parsed = parse_cache(&json).expect("should parse");

        assert_eq!(parsed.v, 1);
        assert_eq!(parsed.fingerprint, fp);
        assert_eq!(parsed.commands.get("lsd"), Some(&true));
        assert_eq!(parsed.commands.get("7z"), Some(&false));
    }

    #[test]
    fn fingerprint_changes_on_path_change() {
        let fp1 = compute_fingerprint("/usr/bin:/bin", 100, "bash");
        let fp2 = compute_fingerprint("/usr/local/bin:/usr/bin:/bin", 100, "bash");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn fingerprint_changes_on_mtime_change() {
        let fp1 = compute_fingerprint("/usr/bin", 100, "bash");
        let fp2 = compute_fingerprint("/usr/bin", 200, "bash");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn fingerprint_changes_on_shell_change() {
        let fp1 = compute_fingerprint("/usr/bin", 100, "bash");
        let fp2 = compute_fingerprint("/usr/bin", 100, "pwsh");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(parse_cache("not json").is_none());
        assert!(parse_cache("").is_none());
        assert!(parse_cache("{}").is_none());
    }

    #[test]
    fn parse_wrong_version_returns_none() {
        let json = r#"{"v":99,"fingerprint":"0123456789abcdef","commands":{}}"#;
        assert!(parse_cache(json).is_none());
    }

    #[test]
    fn parse_rejects_oversized_json() {
        // Just over MAX_CACHE_BYTES
        let huge = format!(
            r#"{{"v":1,"fingerprint":"0123456789abcdef","commands":{{"{}":true}}}}"#,
            "a".repeat(MAX_CACHE_BYTES)
        );
        assert!(parse_cache(&huge).is_none());
    }

    #[test]
    fn parse_rejects_bad_fingerprint_format() {
        // Too short
        let json = r#"{"v":1,"fingerprint":"abc","commands":{}}"#;
        assert!(parse_cache(json).is_none());

        // Right length but non-hex
        let json = r#"{"v":1,"fingerprint":"zzzzzzzzzzzzzzzz","commands":{}}"#;
        assert!(parse_cache(json).is_none());
    }

    #[test]
    fn parse_rejects_too_many_commands() {
        let mut cmds = String::from("{");
        for i in 0..=MAX_CACHE_COMMANDS {
            if i > 0 { cmds.push(','); }
            cmds.push_str(&format!(r#""cmd{i}":true"#));
        }
        cmds.push('}');
        let json = format!(r#"{{"v":1,"fingerprint":"0123456789abcdef","commands":{cmds}}}"#);
        assert!(parse_cache(&json).is_none());
    }

    #[test]
    fn collect_unique_commands_deduplicates() {
        let config = test_config(vec![
            abbr_when("ls", "lsd", vec!["lsd"]),
            abbr_when("ll", "lsd -l", vec!["lsd"]), // same command
            abbr_when("7z", "7zip", vec!["7z"]),
        ]);
        let cmds = collect_unique_commands(&config);
        assert_eq!(cmds, vec!["lsd".to_string(), "7z".to_string()]);
    }

    #[test]
    fn collect_unique_commands_empty_config() {
        let config = test_config(vec![]);
        assert!(collect_unique_commands(&config).is_empty());
    }

    #[test]
    fn export_statement_bash() {
        let stmt = export_statement("bash", r#"{"v":1}"#);
        assert!(stmt.starts_with("export RUNEX_CMD_CACHE_V1="));
        assert!(stmt.contains(r#"{"v":1}"#));
    }

    #[test]
    fn export_statement_pwsh() {
        let stmt = export_statement("pwsh", r#"{"v":1}"#);
        assert!(stmt.starts_with("$env:RUNEX_CMD_CACHE_V1="));
    }
}
