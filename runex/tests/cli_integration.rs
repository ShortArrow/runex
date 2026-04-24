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

/// `doctor` reports `config_file` as Error when the file doesn't exist and exits non-zero.
#[test]
fn doctor_with_missing_explicit_config_exits_nonzero() {
    let (_, _, ok) = run(&["doctor"], Some(std::path::Path::new("/nonexistent/config.toml")), None);
    assert!(!ok, "doctor must exit non-zero when config file is missing");
}

#[test]
fn doctor_shows_parse_error_in_output() {
    let cfg = write_config("[keybind]\ntrigger = \"space\"\n");
    let (stdout, _, ok) = run(&["doctor"], Some(cfg.path()), None);
    assert!(!ok, "doctor must exit non-zero with broken config");
    assert!(stdout.contains("config_parse"), "stdout must contain config_parse check: {stdout}");
    assert!(stdout.contains("failed to load config"), "stdout must contain error message: {stdout}");
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
    let (stdout, _, ok) = run(&["doctor"], Some(cfg.path()), None);
    assert!(!ok, "doctor must exit non-zero for unsupported version");
    assert!(stdout.contains("config_parse"), "stdout must contain config_parse check: {stdout}");
    assert!(stdout.contains("failed to load config"), "stdout must mention the error: {stdout}");
}

#[test]
fn doctor_parse_error_key_too_long() {
    let long_key = "a".repeat(1025);
    let cfg = write_config(&format!("version = 1\n[[abbr]]\nkey = \"{long_key}\"\nexpand = \"x\"\n"));
    let (stdout, _, ok) = run(&["doctor"], Some(cfg.path()), None);
    assert!(!ok, "doctor must exit non-zero for key too long");
    assert!(stdout.contains("config_parse"), "stdout must contain config_parse check: {stdout}");
    assert!(stdout.contains("failed to load config"), "stdout must mention the error: {stdout}");
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
fn json_doctor_is_array_with_name_and_status() {
    let cfg = write_config("version = 1\n");
    let (stdout, _, ok) = run(&["doctor", "--json"], Some(cfg.path()), None);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("doctor --json is not valid JSON: {e}"));
    let arr = v.as_array().expect("doctor --json must be an array");
    assert!(!arr.is_empty());
    assert!(arr[0].get("name").is_some(), "each check must have 'name'");
    assert!(arr[0].get("status").is_some(), "each check must have 'status'");
}

// ─── init --config ────────────────────────────────────────────────────────────

/// Build a Command for `runex init` with HOME/USERPROFILE/PSModulePath/SHELL all
/// redirected into `home_dir` so that shell detection and rc-file resolution
/// stay entirely inside the temp directory on every platform.
///
/// `PSModulePath` is removed to suppress pwsh detection on Windows; `SHELL` is
/// forced to `/bin/bash` so that `rc_file_for()` resolves to `$HOME/.bashrc`
/// inside the temp directory.
fn init_cmd_in_dir(home_dir: &std::path::Path) -> Command {
    let mut cmd = Command::new(bin());
    cmd.env("HOME", home_dir)
        .env("USERPROFILE", home_dir)
        .env("XDG_CONFIG_HOME", home_dir.join(".config"))
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
