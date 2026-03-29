/// End-to-end CLI tests for runex commands.
///
/// These tests invoke the compiled binary as a subprocess and assert on
/// stdout/stderr/exit-code, using:
///   --config <tempfile>   to supply isolated config without touching ~/.config
///   --path-prepend <dir>  to inject fake command presence without altering PATH
use std::io::Write;
use std::path::Path;
use std::process::Command;
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
        // On Unix, mark as executable so is_file() returns true.
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
    // No path_prepend → __runex_fake_lsd__ not on PATH → pass-through
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
    // Must parse as a JSON array
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("list --json is not valid JSON: {e}\nstdout: {stdout}"));
    assert!(parsed.is_array());
    let arr = parsed.as_array().unwrap();
    assert!(!arr.is_empty());
    assert_eq!(arr[0]["key"], "gcm");
    assert_eq!(arr[0]["expand"], "git commit -m");
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
    assert!(ok);
    // The command:__runex_fake_lsd__ check should be OK (not Warn) because we prepended the fake bin
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

#[test]
fn expand_duplicate_key_self_loop_then_real() {
    // rule 1: self-loop (skipped by expand), rule 2: real expansion
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
    // First rule skipped (self-loop), second rule is the match
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

#[test]
fn export_explicit_missing_config_also_fails() {
    // A second way to pass --config before the subcommand — must also fail.
    let (stdout, _, ok) =
        run(&["--config", "/nonexistent/config.toml", "export", "bash"], None, None);
    assert!(!ok, "stdout: {stdout}");
}

#[test]
fn export_with_valid_config_embeds_known_tokens() {
    let cfg = write_config(
        "version = 1\n[[abbr]]\nkey = \"gcm\"\nexpand = \"git commit -m\"\n",
    );
    let (stdout, _, ok) = run(&["export", "bash"], Some(cfg.path()), None);
    assert!(ok);
    assert!(stdout.contains("gcm"), "stdout should embed known token 'gcm'");
}

// ─── doctor --no-shell-aliases ────────────────────────────────────────────────

#[test]
fn doctor_no_shell_aliases_skips_external_shells() {
    let cfg = write_config("version = 1\n");
    // --no-shell-aliases must not spawn pwsh/bash, so the test completes quickly
    // and there are no shell:pwsh:* or shell:bash:* checks in JSON output
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
