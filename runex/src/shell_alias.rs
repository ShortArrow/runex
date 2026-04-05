use std::collections::HashMap;
use std::io;
use std::thread;
use std::time::Duration;

use runex_core::doctor::{Check, CheckStatus, DiagResult};
use runex_core::model::{Abbr, Config};
use runex_core::sanitize::sanitize_for_display;

/// Interval between child-process liveness polls during timeout.
pub(crate) const POLL_INTERVAL_MILLIS: u64 = 100;

/// Maximum number of alias entries accepted from a single shell invocation.
/// Prevents unbounded memory consumption if a misbehaving or compromised shell
/// emits an unusually large number of alias definitions.
pub(crate) const MAX_ALIAS_LINES: usize = 10_000;

/// Maximum byte length of an alias value stored in the alias map.
/// A single extremely long line (e.g. 10 MB) would otherwise consume unbounded
/// memory even with the line-count limit in place.  Values exceeding this limit
/// are silently truncated at a UTF-8 character boundary.
pub(crate) const MAX_ALIAS_VALUE_BYTES: usize = 65_536;

/// Maximum byte length of an alias key (name) stored in the alias map.
/// Alias names longer than any possible abbr key (MAX_KEY_BYTES = 1024) can
/// never match and only waste memory.  Entries with oversized keys are discarded.
pub(crate) const MAX_ALIAS_KEY_BYTES: usize = 1_024;

/// Seconds to wait for a shell subprocess (bash/pwsh) to produce alias output.
/// If the subprocess does not exit within this deadline it is killed and an
/// empty alias map is returned.  Prevents a malicious `bash` or `pwsh` on PATH
/// from hanging `runex doctor` indefinitely.
pub(crate) const ALIAS_SUBPROCESS_TIMEOUT_SECS: u64 = 5;

/// Maximum bytes read from a subprocess's stdout.
/// Prevents a misbehaving or malicious shell from causing unbounded heap
/// allocation during alias enumeration (e.g., outputting /dev/zero data within
/// the timeout window).  Output exceeding this limit is treated as invalid and
/// an empty alias map is returned.
pub(crate) const MAX_SUBPROCESS_OUTPUT_BYTES: usize = 4 * 1024 * 1024; // 4 MB

/// Truncate `s` to at most `max_bytes`, always on a UTF-8 char boundary.
///
/// Walks backwards from `max_bytes` until a valid boundary is found, so the
/// result is never longer than `max_bytes` and is always valid UTF-8.
pub(crate) fn truncate_to_limit(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub(crate) fn parse_pwsh_alias_lines(stdout: &str) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    for line in stdout.lines().take(MAX_ALIAS_LINES) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((name, definition)) = trimmed.split_once('\t') {
            let key = name.trim();
            if key.len() > MAX_ALIAS_KEY_BYTES {
                continue;
            }
            let value = truncate_to_limit(definition.trim(), MAX_ALIAS_VALUE_BYTES);
            aliases.insert(key.to_string(), value.to_string());
        }
    }
    aliases
}

/// Spawn a thread that drains `reader` into a shared buffer capped at `max_bytes + 1`.
///
/// Reading one byte beyond the limit acts as an overflow sentinel: if the returned
/// buffer's length exceeds `max_bytes`, the caller should treat the output as
/// truncated and discard it.
///
/// Returns an `Arc<Mutex<Option<Vec<u8>>>>` rather than a `JoinHandle` so the caller
/// can retrieve the result without blocking when the pipe's write end outlives the child
/// process (e.g. grandchildren that inherited the fd on macOS).
pub(crate) fn spawn_stdout_reader(
    reader: impl io::Read + Send + 'static,
    max_bytes: usize,
) -> std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>> {
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    let result: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let result_clone = Arc::clone(&result);
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.take(max_bytes as u64 + 1).read_to_end(&mut buf);
        if let Ok(mut guard) = result_clone.lock() {
            *guard = Some(buf);
        }
    });
    result
}

/// Poll `slot` until the reader thread writes its result or `deadline` is reached.
///
/// The child's pipe write end closes when the child exits, so the reader thread
/// finishes almost immediately after a successful (non-timed-out) child exit.
/// The 500 ms deadline is a safety net for scheduler jitter.
fn take_reader_result(
    slot: &std::sync::Mutex<Option<Vec<u8>>>,
    deadline: std::time::Instant,
) -> Vec<u8> {
    loop {
        if let Ok(mut guard) = slot.try_lock() {
            if let Some(buf) = guard.take() {
                return buf;
            }
        }
        if std::time::Instant::now() >= deadline {
            return Vec::new();
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Poll `child` until it exits or `timeout` elapses.
///
/// On timeout, sends SIGKILL to the entire process group so that grandchildren
/// (e.g. a `sleep` spawned by the child) cannot keep the stdout pipe open.
/// Returns `None` if the child was killed before it exited naturally.
pub(crate) fn poll_child_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    let poll_interval = Duration::from_millis(POLL_INTERVAL_MILLIS);
    let mut elapsed = Duration::ZERO;
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) => {
                if elapsed >= timeout {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    let _ = child.kill();
                    #[cfg(unix)]
                    reap_child_nonblocking(child.id());
                    #[cfg(not(unix))]
                    let _ = child.wait();
                    return None;
                }
                thread::sleep(poll_interval);
                elapsed += poll_interval;
            }
            Err(_) => return None,
        }
    }
}

/// Reap the child process using WNOHANG so we never block waiting for it.
///
/// After SIGKILL the child may not have exited yet; a blocking waitpid would
/// stall the caller on macOS when the child's stdout pipe is still open.
#[cfg(unix)]
fn reap_child_nonblocking(pid: u32) {
    unsafe {
        libc::waitpid(pid as libc::pid_t, std::ptr::null_mut(), libc::WNOHANG);
    }
}

/// Run a command with a timeout.
///
/// Returns `Some(stdout bytes)` when the child exits successfully within
/// `timeout_secs` seconds and its output fits within [`MAX_SUBPROCESS_OUTPUT_BYTES`].
/// Returns `None` if the child times out, exits with a non-zero status, or produces
/// oversized output.
pub(crate) fn run_with_timeout(
    program: &str,
    args: &[&str],
    env_path: Option<&str>,
    timeout_secs: u64,
) -> Option<Vec<u8>> {
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;

    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());
    if let Some(path) = env_path {
        cmd.env("PATH", path);
    }
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd.spawn().ok()?;
    let stdout_pipe = child.stdout.take()?;
    let reader_result = spawn_stdout_reader(stdout_pipe, MAX_SUBPROCESS_OUTPUT_BYTES);
    let status = poll_child_with_timeout(&mut child, Duration::from_secs(timeout_secs))?;
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    let stdout = take_reader_result(&reader_result, deadline);

    if stdout.len() > MAX_SUBPROCESS_OUTPUT_BYTES {
        return None;
    }
    if !status.success() {
        return None;
    }
    Some(stdout)
}

pub(crate) fn load_pwsh_aliases() -> HashMap<String, String> {
    load_pwsh_aliases_with_path(&std::env::var("PATH").unwrap_or_default())
}

pub(crate) fn load_pwsh_aliases_with_path(path_env: &str) -> HashMap<String, String> {
    if which::which_in("pwsh", Some(path_env), std::env::current_dir().unwrap_or_default())
        .is_err()
    {
        return HashMap::new();
    }

    let stdout = run_with_timeout(
        "pwsh",
        &[
            "-NoLogo",
            "-NoProfile",
            "-Command",
            "Get-Alias | ForEach-Object { \"{0}`t{1}\" -f $_.Name, $_.Definition }",
        ],
        Some(path_env),
        ALIAS_SUBPROCESS_TIMEOUT_SECS,
    );
    match stdout {
        Some(bytes) => parse_pwsh_alias_lines(&String::from_utf8_lossy(&bytes)),
        None => HashMap::new(),
    }
}

pub(crate) fn check_pwsh_alias_with<F>(token: &str, lookup: F) -> Option<Check>
where
    F: Fn(&str) -> Option<String>,
{
    let definition = lookup(token)?;
    Some(Check {
        name: format!("shell:pwsh:key:{}", sanitize_for_display(token)),
        status: CheckStatus::Warn,
        detail: format!(
            "conflicts with existing alias '{}' -> {}",
            sanitize_for_display(token),
            sanitize_for_display(&definition)
        ),
    })
}

pub(crate) fn parse_bash_alias_lines(stdout: &str) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    for line in stdout.lines().take(MAX_ALIAS_LINES) {
        let trimmed = line.trim();
        if !trimmed.starts_with("alias ") {
            continue;
        }
        let rest = &trimmed["alias ".len()..];
        if let Some((name, value)) = rest.split_once('=') {
            let key = name.trim();
            if key.len() > MAX_ALIAS_KEY_BYTES {
                continue;
            }
            let value = truncate_to_limit(value.trim(), MAX_ALIAS_VALUE_BYTES);
            aliases.insert(key.to_string(), value.to_string());
        }
    }
    aliases
}

pub(crate) fn load_bash_aliases() -> HashMap<String, String> {
    load_bash_aliases_with_path(&std::env::var("PATH").unwrap_or_default())
}

/// Load bash aliases by running `bash --norc --noprofile -c alias`.
///
/// Uses `--norc --noprofile` instead of `-i` to avoid sourcing `~/.bashrc` and other
/// startup files, which would execute arbitrary user code during alias enumeration.
/// Returns an empty map on Windows, when bash is not found, or on timeout.
pub(crate) fn load_bash_aliases_with_path(path_env: &str) -> HashMap<String, String> {
    if cfg!(windows) {
        return HashMap::new();
    }

    if which::which_in("bash", Some(path_env), std::env::current_dir().unwrap_or_default())
        .is_err()
    {
        return HashMap::new();
    }

    let stdout = run_with_timeout(
        "bash",
        &["--norc", "--noprofile", "-c", "alias"],
        Some(path_env),
        ALIAS_SUBPROCESS_TIMEOUT_SECS,
    );
    match stdout {
        Some(bytes) => parse_bash_alias_lines(&String::from_utf8_lossy(&bytes)),
        None => HashMap::new(),
    }
}

pub(crate) fn check_bash_alias_with<F>(token: &str, lookup: F) -> Option<Check>
where
    F: Fn(&str) -> Option<String>,
{
    let detail = lookup(token)?;
    Some(Check {
        name: format!("shell:bash:key:{}", sanitize_for_display(token)),
        status: CheckStatus::Warn,
        detail: format!("conflicts with existing alias {}", sanitize_for_display(&detail)),
    })
}

pub(crate) fn collect_shell_alias_conflicts_with<FPwsh, FBash>(
    abbrs: &[Abbr],
    pwsh_lookup: FPwsh,
    bash_lookup: FBash,
) -> Vec<Check>
where
    FPwsh: Fn(&str) -> Option<String> + Copy,
    FBash: Fn(&str) -> Option<String> + Copy,
{
    let mut checks = Vec::new();
    for abbr in abbrs {
        if let Some(check) = check_pwsh_alias_with(&abbr.key, pwsh_lookup) {
            checks.push(check);
        }
        if let Some(check) = check_bash_alias_with(&abbr.key, bash_lookup) {
            checks.push(check);
        }
    }
    checks
}

pub(crate) fn add_shell_alias_conflicts(result: &mut DiagResult, config: Option<&Config>) {
    let Some(config) = config else {
        return;
    };
    let pwsh_aliases = load_pwsh_aliases();
    let bash_aliases = load_bash_aliases();

    result
        .checks
        .extend(collect_shell_alias_conflicts_with(
            &config.abbr,
            |token| pwsh_aliases.get(token).cloned(),
            |token| bash_aliases.get(token).cloned(),
        ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use runex_core::model::Abbr;

    fn test_abbr(key: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: format!("expand-{key}"),
            when_command_exists: None,
        }
    }

    mod alias_parsing {
        use super::*;

    #[test]
    fn collect_shell_alias_conflicts_reports_pwsh_and_bash() {
        let checks = collect_shell_alias_conflicts_with(
            &[test_abbr("gcm"), test_abbr("nv")],
            |token| (token == "gcm").then_some("Get-Command".to_string()),
            |token| (token == "nv").then_some("alias nv='nvim'".to_string()),
        );

        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].name, "shell:pwsh:key:gcm");
        assert!(checks[0].detail.contains("Get-Command"));
        assert_eq!(checks[1].name, "shell:bash:key:nv");
        assert!(checks[1].detail.contains("alias nv='nvim'"));
    }

    #[test]
    fn collect_shell_alias_conflicts_skips_missing_aliases() {
        let checks = collect_shell_alias_conflicts_with(&[test_abbr("gcm")], |_| None, |_| None);
        assert!(checks.is_empty());
    }

    #[test]
    fn parse_pwsh_alias_lines_extracts_aliases() {
        let aliases = parse_pwsh_alias_lines("gcm\tGet-Command\nls\tGet-ChildItem\n");
        assert_eq!(aliases.get("gcm").map(String::as_str), Some("Get-Command"));
        assert_eq!(aliases.get("ls").map(String::as_str), Some("Get-ChildItem"));
    }

    /// Strategy: create a temp HOME with a .bashrc that writes a sentinel file.
    /// With `--norc --noprofile` the sentinel must not be created; `-i` would create it.
    #[test]
    #[cfg(unix)]
    fn load_bash_aliases_does_not_source_startup_files() {
        let home = tempfile::tempdir().unwrap();
        let sentinel = home.path().join("dotfile_executed");
        let bashrc = home.path().join(".bashrc");
        std::fs::write(
            &bashrc,
            format!("touch {}\n", sentinel.display()),
        ).unwrap();

        let output = std::process::Command::new("bash")
            .env("HOME", home.path())
            .args(["--norc", "--noprofile", "-c", "alias"])
            .output();

        if let Ok(out) = output {
            if out.status.success() {
                assert!(
                    !sentinel.exists(),
                    "bash alias detection must not execute ~/.bashrc (startup files sourced)"
                );
            }
        }
    }

    #[test]
    fn parse_bash_alias_lines_extracts_aliases() {
        let aliases = parse_bash_alias_lines("alias ls='ls --color=auto'\nalias nv='nvim'\n");
        assert_eq!(
            aliases.get("ls").map(String::as_str),
            Some("'ls --color=auto'")
        );
        assert_eq!(aliases.get("nv").map(String::as_str), Some("'nvim'"));
    }

    #[test]
    fn check_pwsh_alias_name_strips_control_chars_from_key() {
        let checks = collect_shell_alias_conflicts_with(
            &[test_abbr("key\x1b[2Jevil")],
            |_token| Some("Get-Command".to_string()),
            |_token| None,
        );
        assert_eq!(checks.len(), 1);
        assert!(
            !checks[0].name.contains('\x1b'),
            "shell:pwsh check name must not contain raw ESC: {:?}", checks[0].name
        );
    }

    #[test]
    fn check_bash_alias_name_strips_control_chars_from_key() {
        let checks = collect_shell_alias_conflicts_with(
            &[test_abbr("key\x1b[2Jevil")],
            |_token| None,
            |_token| Some("alias key='evil'".to_string()),
        );
        assert_eq!(checks.len(), 1);
        assert!(
            !checks[0].name.contains('\x1b'),
            "shell:bash check name must not contain raw ESC: {:?}", checks[0].name
        );
    }

    #[test]
    fn check_pwsh_alias_detail_strips_control_chars_from_definition() {
        let checks = collect_shell_alias_conflicts_with(
            &[test_abbr("gcm")],
            |_token| Some("Get-Command\x1b[31mRED\x1b[0m".to_string()),
            |_token| None,
        );
        assert_eq!(checks.len(), 1);
        assert!(
            !checks[0].detail.contains('\x1b'),
            "shell:pwsh check detail must not contain raw ESC from definition: {:?}", checks[0].detail
        );
    }

    } // mod alias_parsing

    /// `parse_bash_alias_lines` and `parse_pwsh_alias_lines` receive output from
    /// external shell processes. If a compromised or misbehaving shell emits an
    /// unbounded number of lines, parsing them all would consume unbounded memory
    /// and CPU. Parsers must silently truncate after a maximum number of lines.
    mod alias_dos_line_count {
        use super::*;

    #[test]
    fn parse_bash_alias_lines_truncates_at_max_lines() {
        let mut input = String::new();
        for i in 0..10_100 {
            input.push_str(&format!("alias k{i}='v{i}'\n"));
        }
        let aliases = parse_bash_alias_lines(&input);
        assert!(
            aliases.len() <= 10_000,
            "parse_bash_alias_lines must not return more than 10,000 entries, got {}",
            aliases.len()
        );
    }

    #[test]
    fn parse_pwsh_alias_lines_truncates_at_max_lines() {
        let mut input = String::new();
        for i in 0..10_100 {
            input.push_str(&format!("k{i}\tv{i}\n"));
        }
        let aliases = parse_pwsh_alias_lines(&input);
        assert!(
            aliases.len() <= 10_000,
            "parse_pwsh_alias_lines must not return more than 10,000 entries, got {}",
            aliases.len()
        );
    }

    #[test]
    fn parse_bash_alias_lines_accepts_normal_count() {
        let mut input = String::new();
        for i in 0..50 {
            input.push_str(&format!("alias k{i}='v{i}'\n"));
        }
        let aliases = parse_bash_alias_lines(&input);
        assert_eq!(aliases.len(), 50, "parse_bash_alias_lines must return all entries below the limit");
    }

    } // mod alias_dos_line_count

    /// Even with MAX_ALIAS_LINES, a single extremely long line (e.g. 10 MB) would
    /// consume unbounded memory for that one entry. Parsers must silently truncate
    /// any alias value that exceeds MAX_ALIAS_VALUE_BYTES to cap per-entry memory.
    mod alias_dos_value_length {
        use super::*;

    #[test]
    fn parse_bash_alias_lines_truncates_oversized_value() {
        let huge_value = "x".repeat(65_536 + 1);
        let input = format!("alias k='{huge_value}'\n");
        let aliases = parse_bash_alias_lines(&input);
        if let Some(val) = aliases.get("k") {
            assert!(
                val.len() <= 65_536,
                "bash alias value must be truncated to MAX_ALIAS_VALUE_BYTES, got {} bytes",
                val.len()
            );
        }
    }

    #[test]
    fn parse_pwsh_alias_lines_truncates_oversized_value() {
        let huge_value = "x".repeat(65_536 + 1);
        let input = format!("k\t{huge_value}\n");
        let aliases = parse_pwsh_alias_lines(&input);
        if let Some(val) = aliases.get("k") {
            assert!(
                val.len() <= 65_536,
                "pwsh alias value must be truncated to MAX_ALIAS_VALUE_BYTES, got {} bytes",
                val.len()
            );
        }
    }

    } // mod alias_dos_value_length

    /// `parse_bash_alias_lines` and `parse_pwsh_alias_lines` truncate the VALUE
    /// at MAX_ALIAS_VALUE_BYTES, but not the KEY (alias name). A misbehaving
    /// shell that emits alias names with huge lengths (e.g. `alias AAAAAA…=v`)
    /// fills the HashMap with oversized keys. With MAX_ALIAS_LINES=10,000 entries
    /// and each key up to 1 MB, total memory could be 10 GB.
    /// Keys must be silently discarded when they exceed MAX_ALIAS_KEY_BYTES.
    mod alias_dos_key_length {
        use super::*;

    #[test]
    fn parse_bash_alias_lines_discards_oversized_key() {
        let huge_key = "k".repeat(1_025);
        let input = format!("alias {huge_key}='value'\n");
        let aliases = parse_bash_alias_lines(&input);
        assert!(
            aliases.is_empty(),
            "parse_bash_alias_lines must discard alias with key longer than MAX_ALIAS_KEY_BYTES, got {} entries",
            aliases.len()
        );
    }

    #[test]
    fn parse_pwsh_alias_lines_discards_oversized_key() {
        let huge_key = "k".repeat(1_025);
        let input = format!("{huge_key}\tvalue\n");
        let aliases = parse_pwsh_alias_lines(&input);
        assert!(
            aliases.is_empty(),
            "parse_pwsh_alias_lines must discard alias with key longer than MAX_ALIAS_KEY_BYTES, got {} entries",
            aliases.len()
        );
    }

    #[test]
    fn parse_bash_alias_lines_accepts_max_length_key() {
        let max_key = "k".repeat(1_024);
        let input = format!("alias {max_key}='value'\n");
        let aliases = parse_bash_alias_lines(&input);
        assert_eq!(aliases.len(), 1, "key at exactly MAX_ALIAS_KEY_BYTES must be stored");
    }

    #[test]
    fn parse_pwsh_alias_lines_accepts_max_length_key() {
        let max_key = "k".repeat(1_024);
        let input = format!("{max_key}\tvalue\n");
        let aliases = parse_pwsh_alias_lines(&input);
        assert_eq!(aliases.len(), 1, "key at exactly MAX_ALIAS_KEY_BYTES must be stored");
    }

    } // mod alias_dos_key_length

    /// Subprocess-level DoS limits: timeout and stdout size cap.
    ///
    /// A malicious or misbehaving shell binary on PATH must not cause alias
    /// loading to hang or exhaust memory. These tests cover both the per-process
    /// timeout and the maximum output size guard.
    mod subprocess {
        use super::*;

    /// A malicious `bash` on PATH that sleeps forever must not cause
    /// `load_bash_aliases` to hang indefinitely. The function must return
    /// within ALIAS_SUBPROCESS_TIMEOUT_SECS + a small margin.
    #[test]
    #[cfg(unix)]
    fn load_bash_aliases_returns_within_timeout_when_bash_hangs() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        let dir = tempfile::tempdir().unwrap();
        let fake_bash = dir.path().join("bash");
        fs::write(&fake_bash, "#!/bin/sh\nsleep 999\n").unwrap();
        fs::set_permissions(&fake_bash, fs::Permissions::from_mode(0o755)).unwrap();

        let original_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), original_path);

        let start = Instant::now();
        let result = load_bash_aliases_with_path(&new_path);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < ALIAS_SUBPROCESS_TIMEOUT_SECS + 2,
            "load_bash_aliases must return within timeout; took {:?}",
            elapsed
        );
        let _ = result;
    }

    /// A malicious `pwsh` on PATH that sleeps forever must not cause
    /// load_pwsh_aliases to hang indefinitely.
    #[test]
    #[cfg(unix)]
    fn load_pwsh_aliases_returns_within_timeout_when_pwsh_hangs() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        let dir = tempfile::tempdir().unwrap();
        let fake_pwsh = dir.path().join("pwsh");
        fs::write(&fake_pwsh, "#!/bin/sh\nsleep 999\n").unwrap();
        fs::set_permissions(&fake_pwsh, fs::Permissions::from_mode(0o755)).unwrap();

        let original_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), original_path);

        let start = Instant::now();
        let result = load_pwsh_aliases_with_path(&new_path);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < ALIAS_SUBPROCESS_TIMEOUT_SECS + 2,
            "load_pwsh_aliases must return within timeout; took {:?}",
            elapsed
        );
        let _ = result;
    }

    /// A shell that emits gigabytes of output must not cause OOM.
    ///
    /// Strategy: create a script that writes `MAX_SUBPROCESS_OUTPUT_BYTES * 2` bytes in a
    /// single `dd` call then exits 0.  Using `exit 0` (not timeout) ensures we test the
    /// size cap rather than the timeout path.  `dd` with a large block size is fast enough
    /// that the process exits well within the timeout window.
    #[test]
    #[cfg(unix)]
    fn run_with_timeout_caps_output_size() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let fake_sh = dir.path().join("flood");
        let bs = MAX_SUBPROCESS_OUTPUT_BYTES * 2;
        let script = format!("#!/bin/sh\ndd if=/dev/zero bs={bs} count=1 2>/dev/null; exit 0\n");
        fs::write(&fake_sh, &script).unwrap();
        fs::set_permissions(&fake_sh, fs::Permissions::from_mode(0o755)).unwrap();

        let result = run_with_timeout(
            fake_sh.to_str().unwrap(),
            &[],
            None,
            ALIAS_SUBPROCESS_TIMEOUT_SECS,
        );

        assert!(
            result.is_none(),
            "run_with_timeout must return None when output exceeds MAX_SUBPROCESS_OUTPUT_BYTES"
        );
    }

    } // mod subprocess

}
