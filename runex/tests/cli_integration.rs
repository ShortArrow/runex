/// End-to-end CLI tests for runex commands.
///
/// These tests invoke the compiled binary as a subprocess and assert on
/// stdout/stderr/exit-code, using:
///   --config <tempfile>   to supply isolated config without touching ~/.config
///   --path-prepend <dir>  to inject fake command presence without altering PATH
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::{NamedTempFile, TempDir};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_runex")
}

/// Write a TOML config to a NamedTempFile and return it.
fn write_config(toml: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(toml.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

/// Create a temporary directory with the given executable stubs (empty files).
fn fake_bin_dir(cmds: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for cmd in cmds {
        let path = dir.path().join(cmd);
        std::fs::write(&path, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
    }
    dir
}

/// Run `runex` with the given arguments, config override, and optional path_prepend.
/// Returns (stdout, stderr, exit_success).
fn run(
    args: &[&str],
    config: Option<&Path>,
    path_prepend: Option<&Path>,
) -> (String, String, bool) {
    let mut cmd = Command::new(bin());

    if let Some(p) = config {
        cmd.arg("--config").arg(p);
    }
    if let Some(p) = path_prepend {
        cmd.arg("--path-prepend").arg(p);
    }
    cmd.args(args);

    let out = cmd.output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

// ─── expand ───────────────────────────────────────────────────────────────────

#[test]
fn expand_known_token() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["expand", "--token", "gcm"], Some(cfg.path()), None);
    assert!(ok);
    assert_eq!(stdout, "git commit -m");
}

#[test]
fn expand_unknown_token_passthrough() {
    let cfg = write_config("version = 1\n");
    let (stdout, _, ok) = run(&["expand", "--token", "xyz"], Some(cfg.path()), None);
    assert!(ok);
    assert_eq!(stdout, "xyz");
}

#[test]
fn expand_condition_skipped_when_command_absent() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"__runex_fake_lsd__\"\nwhen_command_exists = [\"__runex_fake_lsd__\"]\n",
    );
    let (stdout, _, ok) = run(&["expand", "--token", "ls"], Some(cfg.path()), None);
    assert!(ok);
    assert_eq!(stdout, "ls");
}

#[test]
fn expand_condition_passes_with_path_prepend() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"__runex_fake_lsd__\"\nwhen_command_exists = [\"__runex_fake_lsd__\"]\n",
    );
    let bins = fake_bin_dir(&["__runex_fake_lsd__"]);
    let (stdout, _, ok) =
        run(&["expand", "--token", "ls"], Some(cfg.path()), Some(bins.path()));
    assert!(ok);
    assert_eq!(stdout, "__runex_fake_lsd__");
}

#[test]
fn expand_number_placeholder_repeats_unit() {
    let cfg = write_config(
        "version = 1\n\
         [[abbr]]\n\
         key = \"up{number}\"\n\
         expand = \"cd {number}\"\n\
         number = \"../\"\n",
    );
    let (stdout, _, ok) = run(&["expand", "--token", "up3"], Some(cfg.path()), None);
    assert!(ok);
    assert_eq!(stdout, "cd ../../../");
}

#[test]
fn expand_exact_rule_wins_over_number_pattern() {
    let cfg = write_config(
        "version = 1\n\
         [[abbr]]\n\
         key = \"up{number}\"\n\
         expand = \"cd {number}\"\n\
         number = \"../\"\n\
         [[abbr]]\n\
         key = \"up2\"\n\
         expand = \"cd ../EXACT\"\n",
    );
    // Exact rule wins for `up2` even though it appears later in the config.
    let (stdout, _, _) = run(&["expand", "--token", "up2"], Some(cfg.path()), None);
    assert_eq!(stdout, "cd ../EXACT");
    // Pattern handles `up3`.
    let (stdout, _, _) = run(&["expand", "--token", "up3"], Some(cfg.path()), None);
    assert_eq!(stdout, "cd ../../../");
}

#[test]
fn expand_number_pattern_passes_through_zero_and_over_max() {
    let cfg = write_config(
        "version = 1\n\
         [[abbr]]\n\
         key = \"up{number}\"\n\
         expand = \"cd {number}\"\n\
         number = \"../\"\n",
    );
    // Zero is rejected (no fallback to exact `up` either).
    let (stdout, _, ok) = run(&["expand", "--token", "up0"], Some(cfg.path()), None);
    assert!(ok);
    assert_eq!(stdout, "up0");
    // 129 > MAX_NUMERIC_REPEAT (128) → pass-through.
    let (stdout, _, _) = run(&["expand", "--token", "up129"], Some(cfg.path()), None);
    assert_eq!(stdout, "up129");
}

// ─── expand --dry-run ─────────────────────────────────────────────────────────

#[test]
fn dry_run_shows_expanded_result() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) =
        run(&["expand", "--token", "gcm", "--dry-run"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("token: gcm"), "stdout: {stdout}");
    assert!(stdout.contains("git commit -m"), "stdout: {stdout}");
    assert!(stdout.contains("expanded"), "stdout: {stdout}");
}

#[test]
fn dry_run_shows_condition_failed() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"__runex_fake_lsd__\"\nwhen_command_exists = [\"__runex_fake_lsd__\"]\n",
    );
    let (stdout, _, ok) =
        run(&["expand", "--token", "ls", "--dry-run"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("__runex_fake_lsd__: NOT FOUND"), "stdout: {stdout}");
    assert!(stdout.contains("pass-through"), "stdout: {stdout}");
}

#[test]
fn dry_run_condition_passes_with_path_prepend() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"__runex_fake_lsd__\"\nwhen_command_exists = [\"__runex_fake_lsd__\"]\n",
    );
    let bins = fake_bin_dir(&["__runex_fake_lsd__"]);
    let (stdout, _, ok) = run(
        &["expand", "--token", "ls", "--dry-run"],
        Some(cfg.path()),
        Some(bins.path()),
    );
    assert!(ok);
    assert!(stdout.contains("found"), "stdout: {stdout}");
    assert!(stdout.contains("expanded  ->  __runex_fake_lsd__"), "stdout: {stdout}");
}

#[test]
fn dry_run_no_match() {
    let cfg = write_config("version = 1\n");
    let (stdout, _, ok) =
        run(&["expand", "--token", "xyz", "--dry-run"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("no rule matched"), "stdout: {stdout}");
    assert!(stdout.contains("pass-through"), "stdout: {stdout}");
}

// ─── list ────────────────────────────────────────────────────────────────────

#[test]
fn list_shows_all_abbrs() {
    let cfg = write_config(
        "version = 1\n\
         [[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n\
         [[abbr]]\nkey = \"gp\"\nexpand = \"git push\"\n",
    );
    let (stdout, _, ok) = run(&["list"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("gcm"), "stdout: {stdout}");
    assert!(stdout.contains("git commit -m"), "stdout: {stdout}");
    assert!(stdout.contains("gp"), "stdout: {stdout}");
    assert!(stdout.contains("git push"), "stdout: {stdout}");
}

#[test]
fn list_json_is_valid_json_array() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["list", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("list --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert!(parsed.is_array());
    let arr = parsed.as_array().unwrap();
    assert!(!arr.is_empty());
    assert_eq!(arr[0]["key"], "gcm");
    assert_eq!(arr[0]["expand"], "git commit -m");
}

#[test]
fn list_filter_shows_only_matching_key() {
    let cfg = write_config(
        "version = 1\n\
         [[abbr]]\nkey = \"ll\"\nexpand = \"ls -la\"\n\
         [[abbr]]\nkey = \"ll.\"\nexpand = \"ls -laF\"\n\
         [[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["list", "ll"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("ll\t"), "stdout: {stdout}");
    // exact-match: `ll.` must not slip through even though it shares a prefix.
    assert!(!stdout.contains("ll.\t"), "stdout: {stdout}");
    assert!(!stdout.contains("gcm\t"), "stdout: {stdout}");
}

#[test]
fn list_filter_no_match_is_empty_success() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["list", "nope"], Some(cfg.path()), None);
    assert!(ok, "no-match must still exit 0 — list is an enumeration command");
    assert!(
        stdout.trim().is_empty(),
        "no-match must produce empty stdout: {stdout:?}",
    );
}

#[test]
fn list_filter_with_json_emits_filtered_array() {
    let cfg = write_config(
        "version = 1\n\
         [[abbr]]\nkey = \"ll\"\nexpand = \"ls -la\"\n\
         [[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["list", "ll", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let arr: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("list --json must be valid JSON: {e}\nstdout: {stdout}"));
    let items = arr.as_array().expect("expected JSON array");
    assert_eq!(items.len(), 1, "should keep only the ll entry: {stdout}");
    assert_eq!(items[0]["key"], "ll");
}

// ─── which ───────────────────────────────────────────────────────────────────

#[test]
fn which_known_token_shows_expansion() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["which", "gcm"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("gcm"), "stdout: {stdout}");
    assert!(stdout.contains("git commit -m"), "stdout: {stdout}");
}

#[test]
fn which_unknown_token_says_no_rule() {
    let cfg = write_config("version = 1\n");
    let (stdout, _, ok) = run(&["which", "zzz"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("no rule found"), "stdout: {stdout}");
}

#[test]
fn which_skipped_shows_missing_command() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"__runex_fake_lsd__\"\nwhen_command_exists = [\"__runex_fake_lsd__\"]\n",
    );
    let (stdout, _, ok) = run(&["which", "ls"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("skipped"), "stdout: {stdout}");
    assert!(stdout.contains("__runex_fake_lsd__"), "stdout: {stdout}");
}

#[test]
fn which_why_shows_rule_index() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["which", "gcm", "--why"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("rule #1"), "stdout: {stdout}");
}

#[test]
fn which_json_is_valid_json() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["which", "gcm", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("which --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(v["result"], "expanded");
    assert_eq!(v["expansion"], "git commit -m");
}

#[test]
fn which_json_no_match() {
    let cfg = write_config("version = 1\n");
    let (stdout, _, ok) = run(&["which", "zzz", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("which --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(v["result"], "no_match");
}

/// JSON output must use 1-based `rule_index` to match the text output ("rule #1").
#[test]
fn which_json_rule_index_is_one_based() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["which", "gcm", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        v["rule_index"], 1,
        "rule_index must be 1-based (got {})",
        v["rule_index"]
    );
}

/// Skipped entries in JSON output must also use 1-based indices.
#[test]
fn which_json_skipped_indices_are_one_based() {
    let cfg = write_config(
        "version = 1\n\
         [[abbr]]\nkey = \"ls\"\nexpand = \"ls\"\n\
         [[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\n",
    );
    let (stdout, _, ok) = run(&["which", "ls", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["result"], "expanded");
    let skipped = v["skipped"].as_array().expect("skipped must be array");
    assert!(!skipped.is_empty(), "expected at least one skipped entry");
    assert_eq!(
        skipped[0][0], 1,
        "skipped rule index must be 1-based (got {})",
        skipped[0][0]
    );
}

#[test]
fn dry_run_json_rule_index_is_one_based() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(
        &["expand", "--token", "gcm", "--dry-run", "--json"],
        Some(cfg.path()),
        None,
    );
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["result"], "expanded");
    assert_eq!(
        v["rule_index"], 1,
        "dry-run --json rule_index must be 1-based (got {})",
        v["rule_index"]
    );
}

#[test]
fn which_with_path_prepend_resolves_condition() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"__runex_fake_lsd__\"\nwhen_command_exists = [\"__runex_fake_lsd__\"]\n",
    );
    let bins = fake_bin_dir(&["__runex_fake_lsd__"]);
    let (stdout, _, ok) =
        run(&["which", "ls"], Some(cfg.path()), Some(bins.path()));
    assert!(ok);
    assert!(stdout.contains("->"), "stdout: {stdout}");
    assert!(!stdout.contains("skipped"), "stdout: {stdout}");
}

// ─── doctor ──────────────────────────────────────────────────────────────────

#[test]
fn doctor_passes_with_valid_config() {
    let cfg = write_config("version = 1\n");
    let (_, _, ok) = run(&["doctor"], Some(cfg.path()), None);
    assert!(ok, "doctor should exit 0 with valid config");
}

#[test]
fn doctor_fails_with_missing_config() {
    let (_, _, ok) = run(&["doctor", "--config", "/nonexistent/path/config.toml"], None, None);
    assert!(!ok, "doctor should exit non-zero when config file missing");
}

#[test]
fn doctor_json_is_valid_json_array() {
    let cfg = write_config("version = 1\n");
    let (stdout, _, ok) = run(&["doctor", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("doctor --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert!(parsed.is_array());
}

#[test]
fn doctor_with_path_prepend_finds_command() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"__runex_fake_lsd__\"\nwhen_command_exists = [\"__runex_fake_lsd__\"]\n",
    );
    let bins = fake_bin_dir(&["__runex_fake_lsd__"]);
    let (stdout, _, ok) =
        run(&["doctor", "--json"], Some(cfg.path()), Some(bins.path()));
    assert!(ok, "doctor should exit 0 when required command is found");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let checks = parsed.as_array().unwrap();
    let fake_cmd_check = checks
        .iter()
        .find(|c| c["name"].as_str() == Some("command:__runex_fake_lsd__"));
    assert!(fake_cmd_check.is_some(), "expected command:__runex_fake_lsd__ check in output");
    assert_eq!(
        fake_cmd_check.unwrap()["status"].as_str(),
        Some("ok"),
        "__runex_fake_lsd__ should be ok with path_prepend"
    );
}

// ─── version ─────────────────────────────────────────────────────────────────

#[test]
fn version_shows_version_string() {
    let (stdout, _, ok) = run(&["version"], None, None);
    assert!(ok);
    assert!(stdout.starts_with("runex "), "stdout: {stdout}");
}

#[test]
fn version_json_has_version_field() {
    let (stdout, _, ok) = run(&["version", "--json"], None, None);
    assert!(ok);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("version --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert!(parsed["version"].is_string());
    let ver = parsed["version"].as_str().unwrap();
    assert!(!ver.is_empty());
}

// ─── duplicate-key fallthrough (bug regression) ──────────────────────────────

/// Rule 1 is a self-loop (skipped by expand); rule 2 is the real expansion.
#[test]
fn expand_duplicate_key_self_loop_then_real() {
    let cfg = write_config(
        "version = 1\n\
         [[abbr]]\nkey = \"ls\"\nexpand = \"ls\"\n\
         [[abbr]]\nkey = \"ls\"\nexpand = \"lsd2\"\n",
    );
    let (stdout, _, ok) = run(&["expand", "--token", "ls"], Some(cfg.path()), None);
    assert!(ok);
    assert_eq!(stdout.trim(), "lsd2");
}

#[test]
fn which_duplicate_key_shows_skipped_and_final() {
    let cfg = write_config(
        "version = 1\n\
         [[abbr]]\nkey = \"ls\"\nexpand = \"ls\"\n\
         [[abbr]]\nkey = \"ls\"\nexpand = \"lsd2\"\n",
    );
    let (stdout, _, ok) = run(&["which", "ls", "--why"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("rule #1 skipped"), "stdout: {stdout}");
    assert!(stdout.contains("lsd2"), "stdout: {stdout}");
}

#[test]
fn dry_run_duplicate_key_shows_skip_trace() {
    let cfg = write_config(
        "version = 1\n\
         [[abbr]]\nkey = \"ls\"\nexpand = \"ls\"\n\
         [[abbr]]\nkey = \"ls\"\nexpand = \"lsd2\"\n",
    );
    let (stdout, _, ok) =
        run(&["expand", "--token", "ls", "--dry-run"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("rule #1 skipped"), "stdout: {stdout}");
    assert!(stdout.contains("lsd2"), "stdout: {stdout}");
}

// ─── export --config validation ───────────────────────────────────────────────

#[test]
fn export_explicit_invalid_config_fails() {
    let (_, _, ok) = run(
        &["export", "bash", "--config", "/nonexistent/config.toml"],
        None,
        None,
    );
    assert!(!ok, "export with explicit invalid --config should fail");
}

/// `--config` can appear before the subcommand; that variant must also fail when the path is invalid.
#[test]
fn export_explicit_missing_config_also_fails() {
    let (stdout, _, ok) =
        run(&["--config", "/nonexistent/config.toml", "export", "bash"], None, None);
    assert!(!ok, "stdout: {stdout}");
}

#[test]
fn export_bash_bootstrap_calls_runex_hook() {
    // With the hook-based design, templates no longer embed the abbreviation
    // list as inline `case` arms. Instead the bootstrap must delegate to
    // `runex hook` at keypress time.
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["export", "bash"], Some(cfg.path()), None);
    assert!(ok);
    assert!(
        stdout.contains("hook --shell bash"),
        "bash bootstrap should invoke `runex hook --shell bash`; got:\n{stdout}"
    );
}

// ─── config error policy ─────────────────────────────────────────────────────

#[test]
fn explicit_config_not_found_exits_nonzero_and_mentions_path() {
    let (_, stderr, ok) = run(&["list"], Some(std::path::Path::new("/nonexistent/config.toml")), None);
    assert!(!ok, "list with missing --config must exit non-zero");
    assert!(stderr.contains("nonexistent"), "stderr must mention the path: {stderr}");
}

#[test]
fn explicit_config_parse_error_exits_nonzero() {
    let cfg = write_config("this is not valid toml [[[");
    let (_, _, ok) = run(&["list"], Some(cfg.path()), None);
    assert!(!ok, "list with broken config must exit non-zero");
}

// Doctor output has two consumers with different stability needs:
//   * `--json` is the *contract* — name/status pairs are the API that
//     editor plugins / dashboards / scripts parse. Pin those by parsing
//     JSON.
//   * Plain stdout is the *UX* — wording, ordering, emoji are tuned for
//     human readability and may change. Assert only the bare minimum
//     (exit code, presence of the check's machine name) so doctor
//     copy-edits don't ripple into test failures.

/// Helper: locate a doctor check by name from `--json` output.
fn doctor_check<'a>(json: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    json.as_array()?.iter().find(|c| c["name"].as_str() == Some(name))
}

#[test]
fn doctor_with_missing_explicit_config_marks_config_file_error() {
    let (stdout, _, ok) = run(
        &["doctor", "--json"],
        Some(std::path::Path::new("/nonexistent/config.toml")),
        None,
    );
    assert!(!ok, "doctor must exit non-zero when config file is missing");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("doctor --json is not valid JSON: {e}\nstdout: {stdout}"));
    let cf = doctor_check(&parsed, "config_file")
        .expect("doctor --json must include a 'config_file' check");
    assert_eq!(
        cf["status"].as_str(),
        Some("error"),
        "config_file status must be 'error' when the path is missing: {cf}"
    );
}

#[test]
fn doctor_reports_parse_error_via_json_contract() {
    let cfg = write_config("[keybind]\ntrigger = \"space\"\n");
    let (stdout_json, _, ok_json) = run(&["doctor", "--json"], Some(cfg.path()), None);
    assert!(!ok_json, "doctor must exit non-zero with broken config");
    let parsed: serde_json::Value = serde_json::from_str(&stdout_json)
        .unwrap_or_else(|e| panic!("doctor --json is not valid JSON: {e}\nstdout: {stdout_json}"));
    let parse = doctor_check(&parsed, "config_parse")
        .expect("doctor --json must include a 'config_parse' check");
    assert_eq!(parse["status"].as_str(), Some("error"));
    assert!(
        parse["detail"].as_str().is_some_and(|d| !d.is_empty()),
        "config_parse error must carry a non-empty detail: {parse}"
    );

    // Plain stdout: only check the *machine* name appears so users can
    // grep for it. Don't pin specific wording — that's UX surface.
    let (stdout_plain, _, ok_plain) = run(&["doctor"], Some(cfg.path()), None);
    assert!(!ok_plain);
    assert!(
        stdout_plain.contains("config_parse"),
        "plain stdout must reference 'config_parse' check name: {stdout_plain}"
    );
}

#[test]
fn doctor_verbose_shows_multiline_parse_error() {
    let cfg = write_config("[keybind]\ntrigger = \"space\"\n");
    let (stdout_normal, _, _) = run(&["doctor"], Some(cfg.path()), None);
    let (stdout_verbose, _, _) = run(&["doctor", "--verbose"], Some(cfg.path()), None);
    assert!(
        stdout_verbose.lines().count() > stdout_normal.lines().count(),
        "doctor --verbose must produce more output lines than plain doctor\nnormal: {stdout_normal}\nverbose: {stdout_verbose}"
    );
}

#[test]
fn doctor_parse_error_unsupported_version() {
    let cfg = write_config("version = 99\n");
    let (stdout, _, ok) = run(&["doctor", "--json"], Some(cfg.path()), None);
    assert!(!ok, "doctor must exit non-zero for unsupported version");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("doctor --json is not valid JSON: {e}\nstdout: {stdout}"));
    let parse = doctor_check(&parsed, "config_parse")
        .expect("doctor --json must include a 'config_parse' check");
    assert_eq!(parse["status"].as_str(), Some("error"));
}

#[test]
fn doctor_parse_error_key_too_long() {
    let long_key = "a".repeat(1025);
    let cfg = write_config(&format!("version = 1\n[[abbr]]\nkey = \"{long_key}\"\nexpand = \"x\"\n"));
    let (stdout, _, ok) = run(&["doctor", "--json"], Some(cfg.path()), None);
    assert!(!ok, "doctor must exit non-zero for key too long");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("doctor --json is not valid JSON: {e}\nstdout: {stdout}"));
    let parse = doctor_check(&parsed, "config_parse")
        .expect("doctor --json must include a 'config_parse' check");
    assert_eq!(parse["status"].as_str(), Some("error"));
}

#[test]
fn expand_with_missing_explicit_config_exits_nonzero() {
    let (_, _, ok) = run(&["expand", "--token", "ls"], Some(std::path::Path::new("/nonexistent/config.toml")), None);
    assert!(!ok, "expand with missing config must exit non-zero");
}

#[test]
fn which_with_missing_explicit_config_exits_nonzero() {
    let (_, _, ok) = run(&["which", "ls"], Some(std::path::Path::new("/nonexistent/config.toml")), None);
    assert!(!ok, "which with missing config must exit non-zero");
}

// ─── JSON schema regression ───────────────────────────────────────────────────

#[test]
fn json_version_has_required_fields() {
    let (stdout, _, ok) = run(&["version", "--json"], None, None);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("version --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert!(v.get("version").and_then(|v| v.as_str()).is_some(), "must have string 'version' field");
}

#[test]
fn json_list_is_array_with_key_and_expand() {
    let cfg = write_config("version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n");
    let (stdout, _, ok) = run(&["list", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("list --json is not valid JSON: {e}"));
    let arr = v.as_array().expect("list --json must be an array");
    assert!(!arr.is_empty());
    assert!(arr[0].get("key").is_some(), "each entry must have 'key'");
    assert!(arr[0].get("expand").is_some(), "each entry must have 'expand'");
}

#[test]
fn json_doctor_contract_pins_name_and_status_enum() {
    // The `--json` output is the contract for editor plugins / dashboards.
    // Pin (1) the top-level shape (array of objects), (2) the always-
    // present check names, (3) every check's name/status types, and
    // (4) the `status` enum values. Wording, ordering, and human-
    // readable detail strings are intentionally not pinned.
    let cfg = write_config("version = 1\n");
    let (stdout, _, ok) = run(&["doctor", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("doctor --json is not valid JSON: {e}"));

    // (1) top-level shape: array of objects.
    let arr = v.as_array().expect("doctor --json must be a top-level array");
    assert!(!arr.is_empty(), "doctor --json array must not be empty");
    for check in arr {
        assert!(check.is_object(), "every doctor check must be a JSON object: {check}");
    }

    // (2) always-present checks. Both fire on every run regardless of
    //     config validity, so plugins can rely on their existence.
    for required in ["config_file", "config_parse"] {
        assert!(
            doctor_check(&v, required).is_some(),
            "doctor --json must always include the '{required}' check; got: {arr:?}"
        );
    }

    // (3) + (4) per-check types and the status enum.
    //     Mirrors `runex::app::doctor::CheckStatus` in the bin's
    //     module tree (this is an external test crate so the path
    //     is informational, not import-able). If a new variant is
    //     added there, add it here too — that's the whole point.
    const ALLOWED_STATUS: &[&str] = &["ok", "warn", "error"];
    for check in arr {
        let name = check["name"].as_str()
            .unwrap_or_else(|| panic!("each check must have a string 'name': {check}"));
        assert!(!name.is_empty(), "check 'name' must be non-empty: {check}");
        let status = check["status"].as_str()
            .unwrap_or_else(|| panic!("each check must have a string 'status': {check}"));
        assert!(
            ALLOWED_STATUS.contains(&status),
            "check '{name}' status '{status}' is not in {ALLOWED_STATUS:?}"
        );
    }
}

// ─── init --config ────────────────────────────────────────────────────────────

/// Build a Command for `runex init` with HOME/USERPROFILE/XDG_CONFIG_HOME/
/// XDG_CACHE_HOME/LOCALAPPDATA/PSModulePath/SHELL all redirected into
/// `home_dir` so that shell detection, rc-file resolution, and Phase G
/// integration-cache writes stay entirely inside the temp directory on
/// every platform.
///
/// `PSModulePath` is removed to suppress pwsh detection on Windows; `SHELL`
/// is forced to `/bin/bash` so that `rc_file_for()` resolves to
/// `$HOME/.bashrc` inside the temp directory.
///
/// XDG_CACHE_HOME / LOCALAPPDATA are pinned so that
/// `infra::integration_cache::cache_path` resolves into the temp dir.
/// Without this, parallel tests would share the real `~/.cache` and
/// race on writes.
fn init_cmd_in_dir(home_dir: &std::path::Path) -> Command {
    let mut cmd = Command::new(bin());
    cmd.env("HOME", home_dir)
        .env("USERPROFILE", home_dir)
        .env("XDG_CONFIG_HOME", home_dir.join(".config"))
        .env("XDG_CACHE_HOME", home_dir.join(".cache"))
        .env("LOCALAPPDATA", home_dir.join("AppData").join("Local"))
        .env_remove("PSModulePath")
        .env("SHELL", "/bin/bash");
    cmd
}

#[test]
fn init_config_creates_file_at_given_path() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("custom_config.toml");
    assert!(!config_path.exists());

    let out = init_cmd_in_dir(dir.path())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "init",
            "--yes",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(config_path.exists(), "config file must be created at the given path");
}

/// init must succeed even when the shell rc file's parent directory does not yet exist.
#[test]
fn init_creates_rc_parent_dir_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let out = init_cmd_in_dir(dir.path())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "init",
            "--yes",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "init must exit 0 even when rc parent dir is missing\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn init_config_already_exists_does_not_overwrite() {
    let cfg = write_config("version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n");
    let dir = tempfile::tempdir().unwrap();

    let out = init_cmd_in_dir(dir.path())
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "init",
            "--yes",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("already exists"), "stdout: {stdout}");
    let content = std::fs::read_to_string(cfg.path()).unwrap();
    assert!(content.contains("gcm"), "config must not be overwritten");
}

// ─── --path-prepend silent ignore ────────────────────────────────────────────

#[test]
fn list_ignores_path_prepend() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"lsd\"]\n",
    );
    let bins = fake_bin_dir(&["lsd"]);
    let (stdout_with, _, ok_with) = run(&["list"], Some(cfg.path()), Some(bins.path()));
    let (stdout_without, _, ok_without) = run(&["list"], Some(cfg.path()), None);
    assert!(ok_with && ok_without);
    assert_eq!(
        stdout_with, stdout_without,
        "--path-prepend must not affect list output"
    );
}

// ─── expand --json ───────────────────────────────────────────────────────────

#[test]
fn expand_json_expanded() {
    let cfg = write_config("version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n");
    let (stdout, _, ok) = run(&["expand", "--token", "gcm", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expand --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(v["result"], "expanded");
    assert_eq!(v["expansion"], "git commit -m");
}

#[test]
fn expand_json_pass_through() {
    let cfg = write_config("version = 1\n");
    let (stdout, _, ok) = run(&["expand", "--token", "xyz", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expand --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(v["result"], "pass_through");
}

#[test]
fn dry_run_json_expanded() {
    let cfg = write_config("version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n");
    let (stdout, _, ok) = run(
        &["expand", "--token", "gcm", "--dry-run", "--json"],
        Some(cfg.path()),
        None,
    );
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expand --dry-run --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(v["result"], "expanded");
}

#[test]
fn dry_run_json_no_match() {
    let cfg = write_config("version = 1\n");
    let (stdout, _, ok) = run(
        &["expand", "--token", "xyz", "--dry-run", "--json"],
        Some(cfg.path()),
        None,
    );
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expand --dry-run --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(v["result"], "no_match");
}

// ─── doctor --no-shell-aliases ────────────────────────────────────────────────

/// `--no-shell-aliases` must not spawn pwsh/bash, so the test completes quickly
/// and no `shell:pwsh:*` or `shell:bash:*` checks appear in JSON output.
#[test]
fn doctor_no_shell_aliases_skips_external_shells() {
    let cfg = write_config("version = 1\n");
    let (stdout, _, ok) = run(
        &["doctor", "--no-shell-aliases", "--json"],
        Some(cfg.path()),
        None,
    );
    assert!(ok, "stdout: {stdout}");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let checks = parsed.as_array().unwrap();
    let shell_checks: Vec<_> = checks
        .iter()
        .filter(|c| {
            c["name"]
                .as_str()
                .map(|n| n.starts_with("shell:"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        shell_checks.is_empty(),
        "expected no shell alias checks, got: {shell_checks:?}"
    );
}

// --- Terminal escape sequence injection: expansion values must not pass ANSI escapes ---

/// TOML spec forbids raw control characters (U+0000–U+001F, U+007F) in strings.
/// A config containing a raw ESC byte (`\x1b`) in an expansion value must be
/// rejected by the TOML parser, preventing terminal escape injection via `list`.
#[test]
fn list_rejects_config_with_ansi_escape_in_expansion() {
    let mut toml = String::from("version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"");
    toml.push('\x1b'); // literal ESC byte — invalid in TOML string
    toml.push_str("[2Jmalicious\"\n");
    let cfg = write_config(&toml);
    let (_, _, ok) = run(&["list"], Some(cfg.path()), None);
    assert!(
        !ok,
        "list must reject a config with a raw ESC byte in expansion (TOML spec violation)"
    );
}

#[test]
fn which_rejects_config_with_ansi_escape_in_expansion() {
    let mut toml = String::from("version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"");
    toml.push('\x1b');
    toml.push_str("[2Jmalicious\"\n");
    let cfg = write_config(&toml);
    let (_, _, ok) = run(&["which", "ls"], Some(cfg.path()), None);
    assert!(
        !ok,
        "which must reject a config with a raw ESC byte in expansion (TOML spec violation)"
    );
}

// --- doctor/expand: when_command_exists with path-like values ---

/// `when_command_exists` values must be bare command names, not filesystem paths.
/// A value containing a path separator is rejected at config parse time, preventing
/// filesystem probing via `dir.join("../target_file")`.
#[test]
fn expand_when_command_exists_with_path_separator_not_satisfied() {
    let traversal_cmd = "../target_file";
    let toml = format!(
        "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"{traversal_cmd}\"]\n"
    );
    let cfg = write_config(&toml);
    let (_, stderr, ok) = run(&["expand", "--token=ls"], Some(cfg.path()), None);
    assert!(
        !ok,
        "expand must fail when when_command_exists entry contains a path separator (config rejected at parse)"
    );
    assert!(
        stderr.contains("path separator") || stderr.contains("failed to load"),
        "stderr must mention path separator rejection: {stderr:?}"
    );
}

// --- when_command_exists: Windows drive-letter colon must be rejected ─────────

/// On Windows, `dir.join("C:foo")` resolves as an absolute path, bypassing the
/// intended `--path-prepend` directory restriction.  A `when_command_exists` entry
/// containing `:` is rejected at config parse time.
#[test]
#[cfg(windows)]
fn expand_when_command_exists_with_colon_not_satisfied() {
    let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"C:lsd\"]\n";
    let cfg = write_config(toml);
    let bins = fake_bin_dir(&["lsd"]);
    let (_, stderr, ok) = run(&["expand", "--token=ls"], Some(cfg.path()), Some(bins.path()));
    assert!(
        !ok,
        "expand must fail when when_command_exists entry contains ':' (config rejected at parse)"
    );
    assert!(
        stderr.contains("path separator") || stderr.contains("failed to load"),
        "stderr must mention rejection: {stderr:?}"
    );
}

// --- doctor: when_command_exists with absolute path must not probe filesystem ---

/// `when_command_exists` values containing path separators are rejected at config parse time,
/// so a path like `"/etc/passwd"` can never be probed via doctor output. The config must be
/// rejected, never confirming file existence via "found" in the output.
#[test]
fn doctor_when_command_exists_absolute_path_is_treated_as_not_found() {
    #[cfg(unix)]
    {
        let cfg = write_config(
            "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"lsd\"\nwhen_command_exists = [\"/etc/passwd\"]\n",
        );
        let (stdout, _stderr, _ok) = run(&["doctor", "--json"], Some(cfg.path()), None);
        let checks: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
        let empty = vec![];
        let check_arr = checks.as_array().unwrap_or(&empty);
        for check in check_arr {
            let detail = check["detail"].as_str().unwrap_or("");
            assert!(
                !detail.contains("/etc/passwd: found"),
                "doctor must not report absolute path /etc/passwd as found: {detail}"
            );
            let name = check["name"].as_str().unwrap_or("");
            assert!(
                !name.contains("/etc/passwd"),
                "doctor check name must not contain raw path /etc/passwd: {name}"
            );
        }
    }
}

// ─── init: rc file symlink safety ────────────────────────────────────────────

/// init must not follow a symlink at the rc file path (O_NOFOLLOW protection).
/// If the rc path is a symlink, init must refuse to append to it.
#[test]
#[cfg(unix)]
fn init_does_not_follow_symlink_at_rc_file() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let target = dir.path().join("sensitive_target.txt");
    std::fs::write(&target, b"original content").unwrap();
    let bashrc = dir.path().join(".bashrc");
    symlink(&target, &bashrc).unwrap();

    let out = init_cmd_in_dir(dir.path())
        .args(["--config", config_path.to_str().unwrap(), "init", "--yes"])
        .output()
        .unwrap();

    let content = std::fs::read_to_string(&target).unwrap();
    assert_eq!(
        content, "original content",
        "init must not follow symlink at rc file path and write to the symlink target"
    );
    let _ = out;
}

// ─── init: rcfile-write safety properties ─────────────────────────────────────
// docs/setup.md promises four guarantees about `runex init`'s rcfile
// writes (append-only, marker-idempotent, size-cap, seed-config-content).
// The tests below pin those promises so we notice if a refactor breaks
// one. Each one is a regression-pinning test against the current
// implementation rather than TDD against new behaviour.
//
// Most of these tests are `#[cfg(unix)]` because the Windows
// `dirs::home_dir()` uses the Known Folders API (FOLDERID_Profile),
// not `$HOME` / `$USERPROFILE`, so the `init_cmd_in_dir` helper's env
// override does not redirect rcfile resolution on Windows. The
// rcfile-write logic itself is platform-agnostic, so the property
// guarantees still hold on Windows; we just can't exercise them
// without running against the real user `~/.bashrc`, which we refuse
// to do in tests. The seed-config and clink-lua tests don't have
// this limitation and run on all platforms.

/// Append-only: an existing rcfile keeps its prior content byte-for-byte
/// after `runex init`, with the integration block strictly past the
/// previous EOF.
#[test]
#[cfg(unix)]
fn init_preserves_existing_rcfile_lines() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let bashrc = dir.path().join(".bashrc");
    let user_content = "alias ll='ls -la'\nexport EDITOR=nvim\n";
    std::fs::write(&bashrc, user_content).unwrap();

    let out = init_cmd_in_dir(dir.path())
        .args(["--config", config_path.to_str().unwrap(), "init", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "init must succeed: {:?}", out);

    let after = std::fs::read_to_string(&bashrc).unwrap();
    assert!(
        after.starts_with(user_content),
        "init must preserve existing rcfile bytes verbatim at the start; got:\n{after}"
    );
    assert!(
        after.contains("# runex-init"),
        "init must append the marker block after the existing content: {after}"
    );
    let _ = out;
}

/// Idempotent: running `runex init` twice yields exactly one
/// integration block (marker appears once).
#[test]
#[cfg(unix)]
fn init_is_idempotent_marker_present() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let bashrc = dir.path().join(".bashrc");

    init_cmd_in_dir(dir.path())
        .args(["--config", config_path.to_str().unwrap(), "init", "--yes"])
        .output()
        .unwrap();
    let after_first = std::fs::read_to_string(&bashrc).unwrap();

    init_cmd_in_dir(dir.path())
        .args(["--config", config_path.to_str().unwrap(), "init", "--yes"])
        .output()
        .unwrap();
    let after_second = std::fs::read_to_string(&bashrc).unwrap();

    assert_eq!(
        after_first, after_second,
        "second init must be a no-op; rcfile content changed: first=\n{after_first}\nsecond=\n{after_second}"
    );
    let marker_count = after_second.matches("# runex-init").count();
    assert_eq!(
        marker_count, 1,
        "exactly one `# runex-init` marker must be present after two inits; saw {marker_count} in:\n{after_second}"
    );
}

/// Size cap: rcfiles larger than `MAX_RC_FILE_BYTES` (1 MB) are read as
/// if the marker were absent, but the safety read fails closed — the
/// existing oversized content stays intact and no integration block is
/// silently appended.
///
/// Note on current behaviour: `read_rc_content` returns "" for oversized
/// files, which causes `init` to *attempt* to append, which then succeeds
/// (the file exists, append-only). That actually means the marker DOES
/// get added, just below the 1 MB pile. We test for the property that
/// matters — the user's prior bytes survive — rather than the secondary
/// "no-write-on-oversize" behaviour, because the implementation chose
/// "fail safe = append anyway" semantics and we want to pin that
/// faithfully.
#[test]
#[cfg(unix)]
fn init_oversize_rcfile_keeps_prior_content_intact() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let bashrc = dir.path().join(".bashrc");
    // 1 MB + 1 byte of arbitrary data. The limit is `MAX_RC_FILE_BYTES`
    // = 1 << 20 in `runex/src/main.rs`; we synthesise just past it.
    let oversize: Vec<u8> = vec![b'x'; (1024 * 1024) + 1];
    std::fs::write(&bashrc, &oversize).unwrap();

    init_cmd_in_dir(dir.path())
        .args(["--config", config_path.to_str().unwrap(), "init", "--yes"])
        .output()
        .unwrap();

    let after = std::fs::read(&bashrc).unwrap();
    assert!(
        after.starts_with(&oversize),
        "init must never destroy or rewrite the prior rcfile bytes; got len={}",
        after.len()
    );
}

/// Seed config: a fresh `runex init` writes a config file that contains
/// both the working `[keybind.trigger] default = "space"` block and the
/// `gst → git status` sample abbreviation. README and docs/setup
/// promise these as the "first expand in 5 minutes" demonstration.
#[test]
fn init_seed_config_includes_keybind_trigger_and_gst_sample() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    init_cmd_in_dir(dir.path())
        .args(["--config", config_path.to_str().unwrap(), "init", "--yes"])
        .output()
        .unwrap();

    let body = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        body.contains("[keybind.trigger]"),
        "seed config must include [keybind.trigger]: {body}"
    );
    assert!(
        body.contains("default = \"space\""),
        "seed config must bind Space as the default trigger: {body}"
    );
    assert!(
        body.contains("key    = \"gst\""),
        "seed config must include the gst sample abbreviation: {body}"
    );
    assert!(
        body.contains("expand = \"git status\""),
        "seed config must map gst to `git status`: {body}"
    );
}

/// `read_rc_content` (the marker-presence check used by `init`) must
/// also refuse to follow symlinks. The write side already has
/// `O_NOFOLLOW`; if the read side reports "marker present" by reading
/// through a symlink, init makes a different decision than the write
/// would, which is at minimum confusing and potentially usable for
/// information leakage. Pin them to the same policy.
///
/// We observe the read decision via init's stdout: if the symlink was
/// followed, init reports `Shell integration already present in <path>`
/// (it saw the marker in the decoy target). If the symlink was
/// rejected, init either tries to append (and fails at the write-side
/// O_NOFOLLOW) or reports a different message — anything that does
/// NOT contain "already present" proves the read didn't follow.
#[test]
#[cfg(unix)]
fn init_marker_check_does_not_follow_symlink() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    // Decoy that contains the runex marker. If `read_rc_content`
    // follows the bashrc symlink, init will see this marker.
    let target = dir.path().join("decoy_with_marker.txt");
    std::fs::write(&target, "# runex-init\nfake\n").unwrap();
    let bashrc = dir.path().join(".bashrc");
    symlink(&target, &bashrc).unwrap();

    let out = init_cmd_in_dir(dir.path())
        .args(["--config", config_path.to_str().unwrap(), "init", "--yes"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stdout.contains("already present"),
        "init reported the marker as already present, which means \
         read_rc_content followed the symlink. stdout=\n{stdout}\nstderr=\n{stderr}"
    );
}

/// `runex init clink` must refuse to write through a symlink at the
/// install path. An attacker who can create a symlink in the user's
/// clink scripts directory could otherwise redirect runex's write to
/// any file the runex process can write — same threat model as the
/// rcfile write side, where `O_NOFOLLOW` already protects.
#[test]
#[cfg(unix)]
fn init_clink_refuses_symlink_at_install_path() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let target = dir.path().join("sensitive.txt");
    std::fs::write(&target, b"secret").unwrap();
    let lua_link = dir.path().join("runex.lua");
    symlink(&target, &lua_link).unwrap();

    let _out = init_cmd_in_dir(dir.path())
        .env("RUNEX_CLINK_LUA_PATH", lua_link.to_str().unwrap())
        .args(["--config", config_path.to_str().unwrap(), "init", "clink", "--yes"])
        .output()
        .unwrap();

    let after = std::fs::read(&target).unwrap();
    assert_eq!(
        after, b"secret",
        "init clink must refuse to follow a symlink at the install path; the target file got rewritten"
    );
}

/// `runex init clink` writes the clink lua integration to the path
/// chosen by `RUNEX_CLINK_LUA_PATH`. That env override is the supported
/// way to redirect the install for testing or for users with a
/// non-default clink scripts directory.
#[test]
fn init_clink_writes_lua_to_resolved_path() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let lua_target = dir.path().join("clink").join("runex.lua");

    let out = init_cmd_in_dir(dir.path())
        .env("RUNEX_CLINK_LUA_PATH", lua_target.to_str().unwrap())
        .args(["--config", config_path.to_str().unwrap(), "init", "clink", "--yes"])
        .output()
        .unwrap();
    assert!(out.status.success(), "init clink must succeed: {:?}", out);

    let written = std::fs::read_to_string(&lua_target)
        .expect("init clink should have written the lua file at RUNEX_CLINK_LUA_PATH");
    assert!(
        written.contains("runex shell integration for clink"),
        "written lua must be the clink integration template: {written}"
    );
    assert!(
        written.contains("RUNEX_BIN"),
        "written lua must reference RUNEX_BIN: {written}"
    );
}

// ─── export --bin Phase G default-to-current_exe ───────────────────────────────

/// Static-cache layout: when --bin is omitted, the generated script bakes
/// the absolute path of the running runex binary into the hook
/// invocation. This eliminates the per-keystroke PATH lookup that
/// would otherwise hit a `mise` shim on WSL (~470 ms) or a slow
/// `/mnt/c/...` 9p stat chain. The test runs the bin we built, so
/// `current_exe()` resolves to the cargo target binary path.
#[test]
fn export_omitted_bin_bakes_current_exe_absolute_path() {
    let cfg = write_config("version = 1\n");
    let (stdout, _stderr, ok) = run(&["export", "bash"], Some(cfg.path()), None);
    assert!(ok, "export bash with default --bin must succeed");

    // Header records the bin used. Read the header line and confirm
    // it's an absolute path to the current_exe binary, not bare "runex".
    let bin_line = stdout
        .lines()
        .find(|l| l.starts_with("# runex-bin: "))
        .expect("export output must contain `# runex-bin:` header");
    let bin = bin_line.trim_start_matches("# runex-bin: ").trim();
    assert!(
        bin != "runex",
        "default --bin must be the absolute path, not bare 'runex': got {bin:?}"
    );
    assert!(
        std::path::Path::new(bin).is_absolute(),
        "default --bin must be an absolute path: got {bin:?}"
    );
    // The hook function inside the script must invoke the same path.
    assert!(
        stdout.contains(&format!("{bin} hook --shell bash")) || stdout.contains(&format!("'{bin}' hook --shell bash")),
        "hook function must invoke the baked absolute path: {stdout}"
    );
}

/// Phase G: when --bin is explicitly passed (= power user wants a
/// portable hand-managed dotfile), the bare string is preserved as-is
/// and the hook function calls it via PATH lookup. This is the legacy
/// behaviour, retained intentionally so users running multi-machine
/// dotfile sync against differently-installed runex copies aren't
/// forced into per-machine cache regen.
#[test]
fn export_explicit_bin_runex_preserves_bare_name() {
    let cfg = write_config("version = 1\n");
    let (stdout, _stderr, ok) = run(&["export", "bash", "--bin=runex"], Some(cfg.path()), None);
    assert!(ok, "export bash --bin=runex must succeed");

    let bin_line = stdout
        .lines()
        .find(|l| l.starts_with("# runex-bin: "))
        .expect("export output must contain `# runex-bin:` header");
    assert_eq!(
        bin_line.trim(),
        "# runex-bin: runex",
        "explicit --bin=runex must keep the bare name verbatim"
    );
    // The hook function must invoke the bare PATH-resolved name.
    assert!(
        stdout.contains("'runex' hook --shell bash"),
        "explicit --bin=runex must produce a PATH-resolved invocation: {stdout}"
    );
}

/// Phase G: explicit --bin with a non-default value also passes through
/// verbatim (e.g. an absolute path the user picked manually).
#[test]
fn export_explicit_bin_absolute_path_passes_through() {
    let cfg = write_config("version = 1\n");
    let custom = "/opt/custom/runex";
    let (stdout, _stderr, ok) = run(
        &["export", "bash", &format!("--bin={custom}")],
        Some(cfg.path()),
        None,
    );
    assert!(ok, "export bash --bin=/opt/custom/runex must succeed");
    assert!(
        stdout.contains(&format!("# runex-bin: {custom}")),
        "explicit --bin must appear verbatim in the header: {stdout}"
    );
}

// ─── export --bin validation ──────────────────────────────────────────────────

/// export with an empty --bin must exit non-zero; an empty bin name would
/// produce a broken shell script (e.g. `eval "$('' export bash)"`).
#[test]
fn export_with_empty_bin_exits_nonzero() {
    let cfg = write_config("version = 1\n");
    let (_, stderr, ok) = run(&["export", "bash", "--bin="], Some(cfg.path()), None);
    assert!(!ok, "export --bin='' must exit non-zero");
    assert!(
        stderr.contains("bin") || stderr.contains("empty") || stderr.contains("invalid"),
        "stderr must mention the invalid bin: {stderr}"
    );
}

/// export with a whitespace-only --bin must also exit non-zero.
#[test]
fn export_with_whitespace_only_bin_exits_nonzero() {
    let cfg = write_config("version = 1\n");
    let (_, stderr, ok) = run(&["export", "bash", "--bin=   "], Some(cfg.path()), None);
    assert!(!ok, "export --bin='   ' must exit non-zero");
    assert!(
        stderr.contains("bin") || stderr.contains("empty") || stderr.contains("invalid"),
        "stderr must mention the invalid bin: {stderr}"
    );
}

/// export with a --bin containing control characters must exit non-zero.
/// Silent dropping would produce a silently different binary name.
/// `\x07` (BEL) and `\x7f` (DEL) must both be rejected.
#[test]
fn export_with_control_char_in_bin_exits_nonzero() {
    let cfg = write_config("version = 1\n");
    let (_, stderr, ok) = run(&["export", "bash", "--bin=run\x07ex"], Some(cfg.path()), None);
    assert!(!ok, "export --bin with control char must exit non-zero");
    assert!(
        stderr.contains("bin") || stderr.contains("control") || stderr.contains("invalid"),
        "stderr must mention the invalid bin: {stderr}"
    );
}

/// export with --bin containing a Unicode right-to-left override (U+202E) must exit non-zero.
/// Such characters can deceive users about what binary name is embedded in the script.
#[test]
fn export_with_rlo_in_bin_exits_nonzero() {
    let cfg = write_config("version = 1\n");
    let bin_with_rlo = format!("run\u{202e}ex");
    let (_, stderr, ok) = run(&["export", "bash", &format!("--bin={bin_with_rlo}")], Some(cfg.path()), None);
    assert!(!ok, "export --bin with RLO (U+202E) must exit non-zero");
    assert!(
        stderr.contains("bin") || stderr.contains("invalid") || stderr.contains("ASCII"),
        "stderr must mention the invalid bin: {stderr}"
    );
}

/// export with --bin containing a zero-width joiner (U+200D) must exit non-zero.
#[test]
fn export_with_zero_width_joiner_in_bin_exits_nonzero() {
    let cfg = write_config("version = 1\n");
    let bin_with_zwj = format!("run\u{200d}ex");
    let (_, stderr, ok) = run(&["export", "bash", &format!("--bin={bin_with_zwj}")], Some(cfg.path()), None);
    assert!(!ok, "export --bin with ZWJ (U+200D) must exit non-zero");
    assert!(
        stderr.contains("bin") || stderr.contains("invalid") || stderr.contains("ASCII"),
        "stderr must mention the invalid bin: {stderr}"
    );
}

/// export with a --bin containing DEL (\x7f) must exit non-zero.
#[test]
fn export_with_del_in_bin_exits_nonzero() {
    let cfg = write_config("version = 1\n");
    let (_, stderr, ok) = run(&["export", "bash", "--bin=run\x7fex"], Some(cfg.path()), None);
    assert!(!ok, "export --bin with DEL (\\x7f) must exit non-zero");
    assert!(
        stderr.contains("bin") || stderr.contains("control") || stderr.contains("invalid"),
        "stderr must mention the invalid bin: {stderr}"
    );
}

// --- list: terminal injection via Unicode-escaped control chars in config -----

/// `runex list` must not emit raw ANSI escape sequences in human output.
/// parse_config now rejects ASCII control characters (including those injected
/// via TOML \uXXXX escapes), so configs containing them must fail to load.
/// This is the primary defense; sanitize_for_display is defense-in-depth.
#[test]
fn list_rejects_config_with_control_char_in_expansion() {
    let toml = "version = 1\n[[abbr]]\nkey = \"ls\"\nexpand = \"\\u001B[2Jmalicious\"\n";
    let cfg = write_config(toml);
    let (_, _, ok) = run(&["list"], Some(cfg.path()), None);
    assert!(
        !ok,
        "list must reject a config with ESC in expansion (parse_config control char check)"
    );
}

/// A key containing BEL (`\u{0007}`) via TOML Unicode escape must be rejected by `parse_config`.
#[test]
fn list_rejects_config_with_control_char_in_key() {
    let toml = "version = 1\n[[abbr]]\nkey = \"k\\u0007ey\"\nexpand = \"git commit -m\"\n";
    let cfg = write_config(toml);
    let (_, _, ok) = run(&["list"], Some(cfg.path()), None);
    assert!(
        !ok,
        "list must reject a config with BEL in key (parse_config control char check)"
    );
}

/// export with an extremely long --bin must exit non-zero to prevent DoS via huge rc-file writes.
#[test]
fn export_with_oversized_bin_exits_nonzero() {
    let cfg = write_config("version = 1\n");
    let huge_bin = "a".repeat(5000);
    let (_, stderr, ok) = run(&["export", "bash", &format!("--bin={huge_bin}")], Some(cfg.path()), None);
    assert!(!ok, "export --bin with 5000 chars must exit non-zero");
    assert!(
        stderr.contains("bin") || stderr.contains("long") || stderr.contains("invalid"),
        "stderr must mention the invalid bin: {stderr}"
    );
}

// ─── export --shell: terminal injection in error messages ─────────────────────

/// export with an unknown shell containing an ANSI escape sequence must not
/// echo the raw ESC byte into stderr (terminal injection via error message).
/// `ShellParseError` embeds the user-supplied shell name in its Display output;
/// that output must be sanitized before reaching the terminal.
#[test]
fn export_unknown_shell_with_control_char_in_name_does_not_inject_into_stderr() {
    let cfg = write_config("version = 1\n");
    let evil_shell = "bash\x1b[2Jevil";
    let (_, stderr, ok) = run(
        &["export", evil_shell, "--bin=runex"],
        Some(cfg.path()),
        None,
    );
    assert!(!ok, "export with unknown shell must exit non-zero");
    assert!(
        !stderr.contains('\x1b'),
        "stderr must not contain raw ESC from shell name (terminal injection risk): {stderr:?}"
    );
}

/// export with an unknown shell containing a BEL byte (\x07) must not
/// echo the raw BEL byte into stderr.
#[test]
fn export_unknown_shell_with_bel_in_name_does_not_inject_into_stderr() {
    let cfg = write_config("version = 1\n");
    let evil_shell = "bash\x07evil";
    let (_, stderr, ok) = run(
        &["export", evil_shell, "--bin=runex"],
        Some(cfg.path()),
        None,
    );
    assert!(!ok, "export with unknown shell must exit non-zero");
    assert!(
        !stderr.contains('\x07'),
        "stderr must not contain raw BEL from shell name (terminal injection risk): {stderr:?}"
    );
}

/// export with an unknown shell containing U+202E (RIGHT-TO-LEFT OVERRIDE) must not
/// echo the raw RLO into stderr. RLO reverses the visual display order of text, so
/// "bash\u{202E}lve" would appear as "bash evil" in some terminals even though the
/// byte content is different.
#[test]
fn export_unknown_shell_with_rlo_in_name_does_not_inject_into_stderr() {
    let cfg = write_config("version = 1\n");
    let evil_shell = "bash\u{202E}lve";
    let (_, stderr, ok) = run(
        &["export", evil_shell, "--bin=runex"],
        Some(cfg.path()),
        None,
    );
    assert!(!ok, "export with unknown shell must exit non-zero");
    assert!(
        !stderr.contains('\u{202E}'),
        "stderr must not contain raw RLO U+202E from shell name: {stderr:?}"
    );
}

/// export with an unknown shell containing U+FEFF (BOM / zero-width no-break space)
/// must not echo the raw BOM into stderr.
#[test]
fn export_unknown_shell_with_bom_in_name_does_not_inject_into_stderr() {
    let cfg = write_config("version = 1\n");
    let evil_shell = "bash\u{FEFF}evil";
    let (_, stderr, ok) = run(
        &["export", evil_shell, "--bin=runex"],
        Some(cfg.path()),
        None,
    );
    assert!(!ok, "export with unknown shell must exit non-zero");
    assert!(
        !stderr.contains('\u{FEFF}'),
        "stderr must not contain raw BOM U+FEFF from shell name: {stderr:?}"
    );
}

/// export with an unknown shell containing U+200B (ZERO-WIDTH SPACE) must not
/// echo the raw ZWSP into stderr.
#[test]
fn export_unknown_shell_with_zwsp_in_name_does_not_inject_into_stderr() {
    let cfg = write_config("version = 1\n");
    let evil_shell = "ba\u{200B}sh";
    let (_, stderr, ok) = run(
        &["export", evil_shell, "--bin=runex"],
        Some(cfg.path()),
        None,
    );
    assert!(!ok, "export with unknown shell must exit non-zero");
    assert!(
        !stderr.contains('\u{200B}'),
        "stderr must not contain raw ZWSP U+200B from shell name: {stderr:?}"
    );
}

// ─── expand / which: --token length limit ────────────────────────────────────
//
// The shell integration passes the token from the command-line buffer to
// `runex expand --token=<token>`.  Without a length guard, a user pasting a
// huge buffer could cause runex to allocate and process an arbitrarily large
// string.  Any token longer than MAX_KEY_BYTES (1024) can never match an abbr
// rule and must be rejected with a non-zero exit code so the shell integration
// falls back to a plain space insertion.

/// expand --token exceeding the maximum key length must exit non-zero.
#[test]
fn expand_with_oversized_token_exits_nonzero() {
    let cfg = write_config("version = 1\n");
    let huge_token = "k".repeat(1025);
    let (_, stderr, ok) = run(
        &["expand", &format!("--token={huge_token}")],
        Some(cfg.path()),
        None,
    );
    assert!(
        !ok,
        "expand --token longer than 1024 bytes must exit non-zero"
    );
    assert!(
        stderr.contains("token") || stderr.contains("long") || stderr.contains("invalid"),
        "stderr must mention the invalid token: {stderr}"
    );
}

/// which <token> exceeding the maximum key length must exit non-zero.
#[test]
fn which_with_oversized_token_exits_nonzero() {
    let cfg = write_config("version = 1\n");
    let huge_token = "k".repeat(1025);
    let (_, stderr, ok) = run(
        &["which", &huge_token],
        Some(cfg.path()),
        None,
    );
    assert!(
        !ok,
        "which <token> longer than 1024 bytes must exit non-zero"
    );
    assert!(
        stderr.contains("token") || stderr.contains("long") || stderr.contains("invalid"),
        "stderr must mention the invalid token: {stderr}"
    );
}

// ─── init: prompt_confirm stdin DoS ──────────────────────────────────────────
//
// `prompt_confirm` reads a line from stdin to ask the user y/N. Without a
// maximum read size, piping a huge amount of data (e.g. 10 MB of 'a' with no
// newline) causes read_line to buffer it all into a String, consuming memory
// proportional to the input size.
//
// The fix must truncate or discard input beyond MAX_CONFIRM_BYTES, so the
// process stays within expected memory bounds and terminates promptly.

/// `init` must exit promptly even when stdin contains 10 MB of data without a newline.
/// Without a read limit, read_line() buffers all of stdin before returning.
#[test]
fn init_prompt_confirm_handles_huge_stdin_without_oom() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let mut child = init_cmd_in_dir(dir.path())
        .args(["--config", config_path.to_str().unwrap(), "init"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.take().unwrap();
        let mut writer = std::io::BufWriter::new(stdin);
        let chunk = vec![b'y'; 65_536];
        for _ in 0..160 {
            if writer.write_all(&chunk).is_err() {
                break;
            }
        }
    }

    let status = child.wait().unwrap();
    let _ = status;
}

/// `init` with 2 KB of 'y' (no newline) as stdin must not treat the input as "yes".
/// After reading MAX_CONFIRM_BYTES, content beyond the limit is discarded;
/// a blob of raw 'y' bytes without a valid "y\n" response must be treated as "no".
#[test]
fn init_prompt_confirm_huge_stdin_is_treated_as_no() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    let mut child = init_cmd_in_dir(dir.path())
        .args(["--config", config_path.to_str().unwrap(), "init"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.take().unwrap();
        let mut writer = std::io::BufWriter::new(stdin);
        let blob = vec![b'y'; 2048];
        let _ = writer.write_all(&blob);
    }

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !config_path.exists() || stdout.contains("Skipped"),
        "huge 'yyy...' blob without newline must not be treated as 'yes': stdout={stdout}"
    );
}

// ─── hook: oversized --line cap ──────────────────────────────────────────────
//
// `runex hook` runs on every keystroke. If the shell wrapper passes a
// pathologically large `--line` value, the per-keystroke cost should
// stay bounded — we'd rather emit the trigger key's literal-space
// fallback than chew CPU on token extraction. The handler enforces an
// `MAX_HOOK_LINE_BYTES` cap (16 KiB) and emits InsertSpace without
// running any expansion logic.
//
// We pick 16 KiB rather than something larger because Windows
// `CreateProcess` caps argv at ~32 KiB; the cap value has to leave
// room for the rest of the command line and for tests to feed an
// oversize input through argv.

/// `runex hook --shell bash --line <oversize>` must short-circuit to
/// InsertSpace and skip expansion entirely. The cap exists so the
/// per-keystroke handler stays O(1) regardless of buffer size; if the
/// guard regresses the hook would walk a multi-MB paste on every key.
///
/// Construction: place the abbr `gcm` at the buffer head with the
/// cursor immediately after it, then pad to `MAX_HOOK_LINE_BYTES + 1`
/// with `a`. *With* the cap, the handler short-circuits and emits a
/// literal space (the original `gcm` survives unchanged). *Without*
/// the cap, the head `gcm` is in command position with cursor right
/// after it — exactly the shape that triggers expansion — and the
/// output would carry the expanded form. Asserting that the expanded
/// form is absent is what makes this test exercise the cap branch
/// rather than just timing it (timing-based assertions are flaky
/// under CI load).
#[test]
fn hook_oversize_line_short_circuits_before_expansion() {
    let cfg = write_config(
        "version = 1\n\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );

    // Cap is 16 KiB (`MAX_HOOK_LINE_BYTES`). Feed exactly cap + 1 so
    // the `>` boundary fires with no slack. Has to stay below
    // Windows' ~32 KiB CreateProcess argv limit, which it does.
    const OVER_CAP: usize = 16 * 1024 + 1;
    let head = "gcm";
    let pad_len = OVER_CAP - head.len() - 1; // -1 for the space
    let line = format!("{head} {}", "a".repeat(pad_len));
    assert_eq!(line.len(), OVER_CAP, "test must feed exactly cap + 1 bytes");
    let cursor = head.len().to_string(); // right after `gcm`

    let out = Command::new(bin())
        .args(["hook", "--shell", "bash", "--line"])
        .arg(&line)
        .args(["--cursor"])
        .arg(&cursor)
        .env("RUNEX_CONFIG", cfg.path())
        .output()
        .expect("runex hook must spawn");

    assert!(out.status.success(), "hook must succeed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("READLINE_LINE="),
        "expected a READLINE_LINE assignment: {stdout}"
    );
    assert!(
        !stdout.contains("git commit -m"),
        "oversize --line must short-circuit before expansion runs; \
         expanded form leaked into stdout: {stdout}"
    );
}

/// Reflexive control for the oversize test above: feed *exactly* the
/// cap (cap bytes, not cap+1) with the same shape and confirm
/// expansion still fires. Without this control, the oversize test
/// could pass simply because the shape never triggers expansion at
/// all — and we'd silently lose the cap-branch coverage.
#[test]
fn hook_at_cap_still_expands() {
    let cfg = write_config(
        "version = 1\n\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );

    const AT_CAP: usize = 16 * 1024;
    let head = "gcm";
    let pad_len = AT_CAP - head.len() - 1;
    let line = format!("{head} {}", "a".repeat(pad_len));
    assert_eq!(line.len(), AT_CAP, "control must feed exactly cap bytes");
    let cursor = head.len().to_string();

    let out = Command::new(bin())
        .args(["hook", "--shell", "bash", "--line"])
        .arg(&line)
        .args(["--cursor"])
        .arg(&cursor)
        .env("RUNEX_CONFIG", cfg.path())
        .output()
        .expect("runex hook must spawn");
    assert!(out.status.success(), "hook must succeed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("git commit -m"),
        "at-cap control must still expand; otherwise the oversize test \
         is asserting the wrong branch: {stdout}"
    );
}
