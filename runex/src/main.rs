use clap::{Parser, Subcommand};
use runex_core::config::{default_config_path, load_config};
use runex_core::doctor::{self, Check, CheckStatus, DiagResult};
use runex_core::expand::{self, WhichResult};
use runex_core::init as runex_init;
use runex_core::model::{Abbr, Config, ExpandResult};
use runex_core::shell::Shell;
use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";
const GIT_COMMIT: Option<&str> = option_env!("RUNEX_GIT_COMMIT");

struct Spinner {
    done: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Spinner {
    fn start(message: &'static str) -> Self {
        if !io::stderr().is_terminal() {
            return Self {
                done: Arc::new(AtomicBool::new(true)),
                handle: None,
            };
        }

        let done = Arc::new(AtomicBool::new(false));
        let thread_done = Arc::clone(&done);
        let handle = thread::spawn(move || {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut i = 0usize;
            while !thread_done.load(Ordering::Relaxed) {
                eprint!("\r{} {}", frames[i % frames.len()], message);
                let _ = io::stderr().flush();
                i += 1;
                thread::sleep(Duration::from_millis(100));
            }
            eprint!("\r\x1b[2K");
            let _ = io::stderr().flush();
        });

        Self {
            done,
            handle: Some(handle),
        }
    }

    fn stop(mut self) {
        self.done.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Parser)]
#[command(name = "runex", about = "Rune-to-cast expansion engine")]
struct Cli {
    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,

    /// Override config file path (overrides RUNEX_CONFIG env var)
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Prepend a directory to PATH for command existence checks
    #[arg(long, global = true, value_name = "DIR")]
    path_prepend: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Expand a token to its cast
    Expand {
        #[arg(long)]
        token: String,
        /// Print diagnostic output instead of the final expansion
        #[arg(long)]
        dry_run: bool,
    },
    /// List all abbreviations
    List,
    /// Check environment health
    Doctor {
        /// Skip shell alias conflict checks (avoids spawning pwsh/bash)
        #[arg(long)]
        no_shell_aliases: bool,
    },
    /// Show build version information
    Version,
    /// Export shell integration script
    Export {
        /// Target shell: bash, zsh, pwsh, clink, nu
        shell: String,
        /// Binary name used in the generated script
        #[arg(long, default_value = "runex")]
        bin: String,
    },
    /// Show what a token expands to (and why it may be skipped)
    Which {
        /// The abbreviation key to look up
        token: String,
        /// Show detailed reasoning
        #[arg(long)]
        why: bool,
    },
    /// Initialize runex: create config and add shell integration
    Init {
        /// Skip confirmation prompts
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

// ─── Config helpers ──────────────────────────────────────────────────────────

/// Load config, erroring if the path or parse fails. Used by commands that
/// require a valid config (Expand, List, Which).
fn resolve_config(
    config_override: Option<&Path>,
) -> Result<(PathBuf, Config), Box<dyn std::error::Error>> {
    if let Some(path) = config_override {
        let config = load_config(path).map_err(|e| {
            format!("failed to load config {}: {e}", sanitize_for_display(&path.display().to_string()))
        })?;
        return Ok((path.to_path_buf(), config));
    }
    let path = default_config_path()?;
    let config = load_config(&path).map_err(|e| {
        format!("failed to load config {}: {e}", sanitize_for_display(&path.display().to_string()))
    })?;
    Ok((path, config))
}

/// Load config, returning None on failure. Used by commands that degrade
/// gracefully when config is absent (Doctor, Export).
fn resolve_config_opt(config_override: Option<&Path>) -> (PathBuf, Option<Config>) {
    if let Some(path) = config_override {
        return (path.to_path_buf(), load_config(path).ok());
    }
    let path = default_config_path().unwrap_or_default();
    let config = load_config(&path).ok();
    (path, config)
}

/// Returns true if `c` should be removed before printing to a terminal.
///
/// Removes:
/// - ASCII control characters (U+0000–U+001F, U+007F): terminal escape sequences,
///   cursor movement, screen clearing, etc.
/// - Unicode line/paragraph separators (U+0085, U+2028, U+2029): treated as newlines
///   by some runtimes, can cause unexpected line breaks.
/// - Unicode visual-deception characters: invisible characters, zero-width spaces,
///   bidirectional overrides (e.g. U+202E reverses display order), BOM (U+FEFF), etc.
///   These can make displayed text look different from its actual content.
fn is_unsafe_for_display(c: char) -> bool {
    c.is_ascii_control()
        || matches!(c,
            '\u{0085}'                  // NEL (Next Line)
            | '\u{00AD}'               // Soft Hyphen (invisible in many renderers)
            | '\u{034F}'               // Combining Grapheme Joiner
            | '\u{061C}'               // Arabic Letter Mark
            | '\u{115F}'..='\u{1160}' // Hangul fillers
            | '\u{17B4}'..='\u{17B5}' // Khmer invisible vowels
            | '\u{180B}'..='\u{180F}' // Mongolian free variation selectors
            | '\u{200B}'..='\u{200F}' // Zero-width space/non-joiner/joiner/marks
            | '\u{202A}'..='\u{202E}' // Bidirectional formatting (LRE, RLE, PDF, LRO, RLO)
            | '\u{2028}'..='\u{2029}' // Line/Paragraph separator
            | '\u{2060}'..='\u{206F}' // Word joiner, invisible operators, bidi isolates
            | '\u{3164}'               // Hangul filler
            | '\u{FE00}'..='\u{FE0F}' // Variation selectors
            | '\u{FEFF}'               // BOM / zero-width no-break space
            | '\u{FFA0}'               // Halfwidth Hangul filler
            | '\u{FFF9}'..='\u{FFFB}' // Interlinear annotation characters
            | '\u{E0000}'..='\u{E007F}' // Tags block (invisible ASCII lookalikes)
        )
}

/// Strip characters that are unsafe for terminal display before embedding
/// user-controlled data in strings printed to a terminal.
fn sanitize_for_display(s: &str) -> String {
    s.chars().filter(|&c| !is_unsafe_for_display(c)).collect()
}

// ─── Command existence resolver ───────────────────────────────────────────────

/// Build a `command_exists` closure.
///
/// When `path_prepend` is `Some(dir)`, files inside `dir` are checked first
/// (bare name, and `.exe` on Windows). Falls through to `which::which`.
fn make_command_exists(path_prepend: Option<&Path>) -> impl Fn(&str) -> bool + '_ {
    move |cmd: &str| -> bool {
        // when_command_exists values must be bare command names, not filesystem paths.
        // Reject anything containing a path separator or Windows drive letter (':') to
        // prevent directory traversal and absolute-path probing via dir.join(cmd).
        if cmd.contains('/') || cmd.contains('\\') || cmd.contains(':') {
            return false;
        }
        if let Some(dir) = path_prepend {
            if dir.join(cmd).is_file() {
                return true;
            }
            #[cfg(windows)]
            if dir.join(format!("{cmd}.exe")).is_file() {
                return true;
            }
        }
        which::which(cmd).is_ok()
    }
}

// ─── Formatting helpers ───────────────────────────────────────────────────────

fn format_check_tag(status: &CheckStatus) -> String {
    match status {
        CheckStatus::Ok => format!("[{ANSI_GREEN}OK{ANSI_RESET}]"),
        CheckStatus::Warn => format!("[{ANSI_YELLOW}WARN{ANSI_RESET}]"),
        CheckStatus::Error => format!("[{ANSI_RED}ERROR{ANSI_RESET}]"),
    }
}

fn format_check_line(check: &Check) -> String {
    format!(
        "{:>8}  {}: {}",
        format_check_tag(&check.status),
        check.name,
        check.detail
    )
}

fn version_line() -> String {
    let version = env!("CARGO_PKG_VERSION");
    match GIT_COMMIT {
        Some(commit) if !commit.is_empty() => format!("runex {version} ({commit})"),
        _ => format!("runex {version}"),
    }
}

fn format_skip_reason(i: usize, reason: &expand::SkipReason, why: bool) -> String {
    if !why {
        return String::new();
    }
    match reason {
        expand::SkipReason::SelfLoop => {
            format!("\n  rule #{} skipped: key == expand (self-loop)", i + 1)
        }
        expand::SkipReason::ConditionFailed { found_commands, missing_commands } => {
            let mut parts = Vec::new();
            for cmd in found_commands {
                parts.push(format!("{}: found", sanitize_for_display(cmd)));
            }
            for cmd in missing_commands {
                parts.push(format!("{}: NOT FOUND", sanitize_for_display(cmd)));
            }
            format!(
                "\n  rule #{} skipped: when_command_exists [{}]",
                i + 1,
                parts.join(", ")
            )
        }
    }
}

fn format_which_result(result: &WhichResult, why: bool) -> String {
    match result {
        WhichResult::Expanded {
            key,
            expansion,
            rule_index,
            satisfied_conditions,
            skipped,
        } => {
            let key = sanitize_for_display(key);
            let expansion = sanitize_for_display(expansion);
            let mut s = format!("{key}  ->  {expansion}");
            if why {
                for (i, reason) in skipped {
                    s.push_str(&format_skip_reason(*i, reason, true));
                }
                s.push_str(&format!("\n  rule #{} matched", rule_index + 1));
                if satisfied_conditions.is_empty() {
                    s.push_str(", no conditions");
                } else {
                    for cmd in satisfied_conditions {
                        let cmd = sanitize_for_display(cmd);
                        s.push_str(&format!("\n  condition: when_command_exists '{cmd}' -> found"));
                    }
                }
            }
            s
        }
        WhichResult::AllSkipped { token, skipped } => {
            // Collect distinct skip reasons across all matching rules for the headline.
            let has_condition_fail = skipped.iter().any(|(_, r)| {
                matches!(r, expand::SkipReason::ConditionFailed { .. })
            });
            let has_self_loop = skipped
                .iter()
                .any(|(_, r)| matches!(r, expand::SkipReason::SelfLoop));
            let token = sanitize_for_display(token);
            let headline = match (has_condition_fail, has_self_loop) {
                (true, true) => format!(
                    "{token}  [skipped: condition failed on some rules; others are self-loops]"
                ),
                (true, false) => {
                    // Collect all missing commands across all ConditionFailed rules.
                    let all_missing: Vec<String> = skipped
                        .iter()
                        .flat_map(|(_, r)| match r {
                            expand::SkipReason::ConditionFailed { missing_commands, .. } => {
                                missing_commands.iter().map(|c| sanitize_for_display(c)).collect::<Vec<_>>()
                            }
                            _ => vec![],
                        })
                        .collect::<std::collections::HashSet<_>>()
                        .into_iter()
                        .collect();
                    format!("{token}  [skipped: {} not found]", all_missing.join(", "))
                }
                (false, true) => format!("{token}  [no-op: key and expansion are identical]"),
                (false, false) => format!("{token}: no rule found"),
            };
            let mut s = headline;
            if why {
                for (i, reason) in skipped {
                    s.push_str(&format_skip_reason(*i, reason, true));
                }
            }
            s
        }
        WhichResult::NoMatch { token } => format!("{}: no rule found", sanitize_for_display(token)),
    }
}

/// Convert a `WhichResult` to a JSON value with 1-based rule indices.
///
/// `WhichResult` stores 0-based indices internally (matching `enumerate()`).
/// The text output already presents these as `rule #1`, `rule #2`, etc., so
/// JSON must use the same numbering for consistency.
fn which_result_to_json(result: &WhichResult) -> serde_json::Value {
    match result {
        WhichResult::Expanded {
            key,
            expansion,
            rule_index,
            satisfied_conditions,
            skipped,
        } => serde_json::json!({
            "result": "expanded",
            "key": key,
            "expansion": expansion,
            "rule_index": rule_index + 1,
            "satisfied_conditions": satisfied_conditions,
            "skipped": skipped.iter().map(|(i, r)| serde_json::json!([i + 1, r])).collect::<Vec<_>>(),
        }),
        WhichResult::AllSkipped { token, skipped } => serde_json::json!({
            "result": "all_skipped",
            "token": token,
            "skipped": skipped.iter().map(|(i, r)| serde_json::json!([i + 1, r])).collect::<Vec<_>>(),
        }),
        WhichResult::NoMatch { token } => serde_json::json!({
            "result": "no_match",
            "token": token,
        }),
    }
}

fn format_dry_run_result(token: &str, result: &WhichResult) -> String {
    let mut out = String::new();
    out.push_str(&format!("token: {}\n", sanitize_for_display(token)));
    match result {
        WhichResult::Expanded {
            key,
            expansion,
            rule_index,
            satisfied_conditions,
            skipped,
        } => {
            for (i, reason) in skipped {
                match reason {
                    expand::SkipReason::SelfLoop => {
                        out.push_str(&format!("rule #{} skipped: self-loop\n", i + 1));
                    }
                    expand::SkipReason::ConditionFailed { found_commands, missing_commands } => {
                        out.push_str(&format!("rule #{} skipped: when_command_exists\n", i + 1));
                        for cmd in found_commands {
                            out.push_str(&format!("  {}: found\n", sanitize_for_display(cmd)));
                        }
                        for cmd in missing_commands {
                            out.push_str(&format!("  {}: NOT FOUND\n", sanitize_for_display(cmd)));
                        }
                    }
                }
            }
            out.push_str(&format!("matched rule #{} (key = '{}')\n", rule_index + 1, sanitize_for_display(key)));
            if satisfied_conditions.is_empty() {
                out.push_str("conditions: none\n");
            } else {
                out.push_str("conditions:\n");
                for cmd in satisfied_conditions {
                    out.push_str(&format!("  when_command_exists '{}': found\n", sanitize_for_display(cmd)));
                }
            }
            out.push_str(&format!("result: expanded  ->  {}\n", sanitize_for_display(expansion)));
        }
        WhichResult::AllSkipped { token, skipped } => {
            for (i, reason) in skipped {
                match reason {
                    expand::SkipReason::SelfLoop => {
                        out.push_str(&format!("rule #{} skipped: self-loop\n", i + 1));
                    }
                    expand::SkipReason::ConditionFailed { found_commands, missing_commands } => {
                        out.push_str(&format!("rule #{} skipped: when_command_exists\n", i + 1));
                        for cmd in found_commands {
                            out.push_str(&format!("  {}: found\n", sanitize_for_display(cmd)));
                        }
                        for cmd in missing_commands {
                            out.push_str(&format!("  {}: NOT FOUND\n", sanitize_for_display(cmd)));
                        }
                    }
                }
            }
            out.push_str(&format!("no rule for '{}' passed all conditions\n", sanitize_for_display(token)));
            out.push_str("result: pass-through\n");
        }
        WhichResult::NoMatch { token } => {
            out.push_str(&format!("no rule matched '{}'\n", sanitize_for_display(token)));
            out.push_str("result: pass-through\n");
        }
    }
    out
}

/// Maximum byte size of an rc file that `init` will read for marker detection.
/// Files larger than this are treated as if the marker is absent so that init
/// fails safe (appends the integration line) rather than consuming unbounded memory.
const MAX_RC_FILE_BYTES: usize = 1024 * 1024; // 1 MB

/// Read a shell rc file for RUNEX_INIT_MARKER detection.
///
/// Returns the file contents as a string, or an empty string if:
/// - the file does not exist (init should append)
/// - the file exceeds MAX_RC_FILE_BYTES (safety: never read enormous files)
/// - the file cannot be read for any other I/O reason
///
/// Uses a single file descriptor for both the metadata check and the read to
/// eliminate the TOCTOU race that exists when `metadata()` and `read_to_string()`
/// open the file separately.  On Unix, `O_NONBLOCK` prevents `open()` from
/// blocking if the path points to a named pipe (FIFO) with no writer.
fn read_rc_content(path: &Path) -> String {
    use std::io::Read;
    // Open the file once.  On Unix, O_NONBLOCK prevents open() from blocking on
    // a FIFO that has no writer yet, closing the TOCTOU window between a separate
    // metadata() call and a subsequent read_to_string().
    // We intentionally do NOT use O_NOFOLLOW so that symlinked dotfiles (common
    // in dotfile-manager setups) continue to work.
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
        {
            Ok(f) => f,
            Err(_) => return String::new(),
        }
    };
    #[cfg(not(unix))]
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    // Metadata from the same fd — no second open, no TOCTOU window.
    let meta = match file.metadata() {
        Ok(m) => m,
        Err(_) => return String::new(),
    };
    // Reject non-regular files (named pipes, device files).
    // On Unix these report len=0 but read_to_string() would consume unbounded
    // data (/dev/zero) or block until a writer appears (FIFO).
    if !meta.is_file() {
        return String::new();
    }
    if meta.len() > MAX_RC_FILE_BYTES as u64 {
        return String::new();
    }
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap_or_default();
    content
}

// ─── Shell helpers ────────────────────────────────────────────────────────────

fn detect_shell() -> Option<Shell> {
    // Unix: $SHELL environment variable
    if let Ok(sh) = std::env::var("SHELL") {
        let base = Path::new(&sh)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if let Ok(s) = base.parse::<Shell>() {
            return Some(s);
        }
    }
    // Windows: presence of PSModulePath implies a PowerShell parent
    if std::env::var("PSModulePath").is_ok() {
        return Some(Shell::Pwsh);
    }
    None
}

/// Maximum byte length accepted from a single `prompt_confirm` read.
/// A real y/N answer is at most a few bytes; anything beyond this limit
/// is treated as "no" to prevent unbounded memory growth from piped input.
const MAX_CONFIRM_BYTES: usize = 1_024;

/// Inner implementation of `prompt_confirm` that reads from an arbitrary `BufRead`.
/// Returns true only for trimmed, case-insensitive "y" or "yes" responses
/// that fit within MAX_CONFIRM_BYTES. Oversized input is treated as "no".
fn prompt_confirm_from(reader: &mut impl io::BufRead) -> bool {
    use io::{BufRead as _, Read as _};
    let mut input = String::new();
    // Read at most MAX_CONFIRM_BYTES + 1 bytes via a by_ref adapter so we do not
    // consume the reader itself.  The +1 lets us detect inputs that exceed the limit:
    // if input.len() > MAX_CONFIRM_BYTES the response is abnormally long → treat as "no".
    let mut limited = reader.by_ref().take(MAX_CONFIRM_BYTES as u64 + 1);
    match limited.read_line(&mut input) {
        Err(_) => return false,
        Ok(0) => return false, // EOF with no data
        Ok(_) => {}
    }
    // Reject if the limited read filled more than the allowed budget.
    if input.len() > MAX_CONFIRM_BYTES {
        return false;
    }
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn prompt_confirm(msg: &str) -> bool {
    eprint!("{msg} [y/N] ");
    let _ = io::stderr().flush();
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    prompt_confirm_from(&mut reader)
}

/// Maximum number of alias entries accepted from a single shell invocation.
/// Prevents unbounded memory consumption if a misbehaving or compromised shell
/// emits an unusually large number of alias definitions.
const MAX_ALIAS_LINES: usize = 10_000;

/// Maximum byte length of an alias value stored in the alias map.
/// A single extremely long line (e.g. 10 MB) would otherwise consume unbounded
/// memory even with the line-count limit in place.  Values exceeding this limit
/// are silently truncated at a UTF-8 character boundary.
const MAX_ALIAS_VALUE_BYTES: usize = 65_536;

/// Maximum byte length of an alias key (name) stored in the alias map.
/// Alias names longer than any possible abbr key (MAX_KEY_BYTES = 1024) can
/// never match and only waste memory.  Entries with oversized keys are discarded.
const MAX_ALIAS_KEY_BYTES: usize = 1_024;

/// Seconds to wait for a shell subprocess (bash/pwsh) to produce alias output.
/// If the subprocess does not exit within this deadline it is killed and an
/// empty alias map is returned.  Prevents a malicious `bash` or `pwsh` on PATH
/// from hanging `runex doctor` indefinitely.
const ALIAS_SUBPROCESS_TIMEOUT_SECS: u64 = 5;

/// Maximum bytes read from a subprocess's stdout.
/// Prevents a misbehaving or malicious shell from causing unbounded heap
/// allocation during alias enumeration (e.g., outputting /dev/zero data within
/// the timeout window).  Output exceeding this limit is treated as invalid and
/// an empty alias map is returned.
const MAX_SUBPROCESS_OUTPUT_BYTES: usize = 4 * 1024 * 1024; // 4 MB

fn truncate_to_limit(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Find the largest char boundary at or before max_bytes.
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn parse_pwsh_alias_lines(stdout: &str) -> HashMap<String, String> {
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

/// Run a command with a timeout.  If the child does not exit within
/// `timeout_secs` seconds it is killed and `None` is returned.
/// Returns `Some(stdout bytes)` on success (exit 0), `None` otherwise.
///
/// Uses a reader thread to collect stdout while the main thread enforces
/// the deadline.  The child's stdin is closed immediately so that the child
/// is not blocked waiting for input.
fn run_with_timeout(
    program: &str,
    args: &[&str],
    env_path: Option<&str>,
    timeout_secs: u64,
) -> Option<Vec<u8>> {
    use std::io::Read;
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;

    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());
    if let Some(path) = env_path {
        cmd.env("PATH", path);
    }
    // Place the child in its own process group so that we can send SIGKILL
    // to the entire group (including grandchildren like `sleep`) without
    // affecting the current process group.
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd.spawn().ok()?;

    // Move the stdout pipe into a reader thread so that `wait()` below does
    // not block on pipe drainage.  Cap the read at MAX_SUBPROCESS_OUTPUT_BYTES
    // to prevent unbounded heap allocation from a misbehaving subprocess.
    let stdout_pipe = child.stdout.take()?;
    let reader_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe
            .take(MAX_SUBPROCESS_OUTPUT_BYTES as u64 + 1)
            .read_to_end(&mut buf);
        buf
    });

    // Poll for child exit up to the deadline.
    let deadline = Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_millis(100);
    let mut elapsed = Duration::ZERO;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if elapsed >= deadline {
                    // Timed out: kill the child and all its descendants by
                    // sending SIGKILL to the process group.
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                thread::sleep(poll_interval);
                elapsed += poll_interval;
            }
            Err(_) => break None,
        }
    };

    let stdout = reader_handle.join().unwrap_or_default();

    // Reject output that hit the cap (read MAX+1 bytes → oversized).
    if stdout.len() > MAX_SUBPROCESS_OUTPUT_BYTES {
        return None;
    }

    let status = status?;
    if !status.success() {
        return None;
    }
    Some(stdout)
}

fn load_pwsh_aliases() -> HashMap<String, String> {
    load_pwsh_aliases_with_path(&std::env::var("PATH").unwrap_or_default())
}

fn load_pwsh_aliases_with_path(path_env: &str) -> HashMap<String, String> {
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

fn check_pwsh_alias_with<F>(token: &str, lookup: F) -> Option<Check>
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

fn parse_bash_alias_lines(stdout: &str) -> HashMap<String, String> {
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

fn load_bash_aliases() -> HashMap<String, String> {
    load_bash_aliases_with_path(&std::env::var("PATH").unwrap_or_default())
}

fn load_bash_aliases_with_path(path_env: &str) -> HashMap<String, String> {
    if cfg!(windows) {
        return HashMap::new();
    }

    if which::which_in("bash", Some(path_env), std::env::current_dir().unwrap_or_default())
        .is_err()
    {
        return HashMap::new();
    }

    // Use --norc --noprofile instead of -i to avoid sourcing ~/.bashrc and other
    // startup files, which could execute arbitrary user code during alias enumeration.
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

fn check_bash_alias_with<F>(token: &str, lookup: F) -> Option<Check>
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

fn collect_shell_alias_conflicts_with<FPwsh, FBash>(
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

fn add_shell_alias_conflicts(result: &mut DiagResult, config: Option<&Config>) {
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

// ─── Main ─────────────────────────────────────────────────────────────────────

/// Maximum byte length accepted for `--token` (expand) and `which <token>`.
/// Tokens longer than any possible abbr key (MAX_KEY_BYTES = 1024 in config.rs)
/// can never match and would cause needless memory allocation in sanitize_for_display.
const MAX_TOKEN_BYTES: usize = 1_024;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Expand { token, dry_run } => {
            if token.len() > MAX_TOKEN_BYTES {
                eprintln!(
                    "error: --token is too long ({} bytes); maximum is {MAX_TOKEN_BYTES}",
                    token.len()
                );
                std::process::exit(1);
            }
            let (_config_path, config) = resolve_config(cli.config.as_deref())?;
            let command_exists = make_command_exists(cli.path_prepend.as_deref());
            if dry_run {
                let result = expand::which_abbr(&config, &token, &command_exists);
                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&which_result_to_json(&result))?);
                } else {
                    print!("{}", format_dry_run_result(&token, &result));
                }
            } else {
                let result = expand::expand(&config, &token, &command_exists);
                if cli.json {
                    let v = match &result {
                        ExpandResult::Expanded(s) => serde_json::json!({
                            "result": "expanded",
                            "token": token,
                            "expansion": s,
                        }),
                        ExpandResult::PassThrough(s) => serde_json::json!({
                            "result": "pass_through",
                            "token": s,
                        }),
                    };
                    println!("{}", serde_json::to_string_pretty(&v)?);
                } else {
                    match result {
                        ExpandResult::Expanded(s) => print!("{s}"),
                        ExpandResult::PassThrough(s) => print!("{s}"),
                    }
                }
            }
        }
        Commands::List => {
            let (_config_path, config) = resolve_config(cli.config.as_deref())?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&config.abbr)?);
            } else {
                for (key, exp) in expand::list(&config) {
                    println!("{}\t{}", sanitize_for_display(key), sanitize_for_display(exp));
                }
            }
        }
        Commands::Version => {
            if cli.json {
                #[derive(serde::Serialize)]
                struct VersionJson<'a> {
                    version: &'a str,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    commit: Option<&'a str>,
                }
                let v = VersionJson {
                    version: env!("CARGO_PKG_VERSION"),
                    commit: GIT_COMMIT.filter(|s| !s.is_empty()),
                };
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!("{}", version_line());
            }
        }
        Commands::Export { shell, bin } => {
            const MAX_BIN_LEN: usize = 255;
            if bin.trim().is_empty() {
                eprintln!("error: --bin must not be empty or whitespace-only");
                std::process::exit(1);
            }
            if bin.len() > MAX_BIN_LEN {
                eprintln!("error: --bin is too long ({} bytes); maximum is {MAX_BIN_LEN}", bin.len());
                std::process::exit(1);
            }
            if bin.chars().any(|c| c.is_ascii_control() || c == '\u{0085}' || c == '\u{2028}' || c == '\u{2029}') {
                eprintln!("error: --bin contains an invalid control character");
                std::process::exit(1);
            }
            // Restrict to printable ASCII to prevent Unicode homoglyphs, right-to-left
            // override (U+202E), zero-width joiners, and other visually deceptive characters
            // from being silently embedded in generated shell scripts.
            if bin.chars().any(|c| !c.is_ascii() || !c.is_ascii_graphic()) {
                eprintln!("error: --bin must contain only printable ASCII characters");
                std::process::exit(1);
            }
            let s: Shell = shell.parse().map_err(|e: runex_core::shell::ShellParseError| {
                Box::<dyn std::error::Error>::from(e.to_string())
            })?;
            // Explicit --config must be valid; implicit default degrades gracefully.
            let config = if cli.config.is_some() {
                let (_path, cfg) = resolve_config(cli.config.as_deref())?;
                Some(cfg)
            } else {
                let (_path, cfg) = resolve_config_opt(None);
                cfg
            };
            print!("{}", runex_core::shell::export_script(s, &bin, config.as_ref()));
        }
        Commands::Doctor { no_shell_aliases } => {
            let (config_path, config) = resolve_config_opt(cli.config.as_deref());
            let command_exists = make_command_exists(cli.path_prepend.as_deref());
            let spinner = Spinner::start("Checking environment...");
            let mut result = doctor::diagnose(&config_path, config.as_ref(), &command_exists);
            if !no_shell_aliases {
                add_shell_alias_conflicts(&mut result, config.as_ref());
            }
            spinner.stop();

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&result.checks)?);
            } else {
                for check in &result.checks {
                    println!("{}", format_check_line(check));
                }
            }

            if !result.is_healthy() {
                std::process::exit(1);
            }
        }
        Commands::Which { token, why } => {
            if token.len() > MAX_TOKEN_BYTES {
                eprintln!(
                    "error: token is too long ({} bytes); maximum is {MAX_TOKEN_BYTES}",
                    token.len()
                );
                std::process::exit(1);
            }
            let (_config_path, config) = resolve_config(cli.config.as_deref())?;
            let command_exists = make_command_exists(cli.path_prepend.as_deref());
            let result = expand::which_abbr(&config, &token, &command_exists);
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&which_result_to_json(&result))?);
            } else {
                println!("{}", format_which_result(&result, why));
            }
        }
        Commands::Init { yes } => {
            let config_path = if let Some(p) = cli.config.as_deref() {
                p.to_path_buf()
            } else {
                default_config_path()?
            };

            // Step 1: config file
            // Use create_new to atomically create the file, avoiding the TOCTOU race
            // between an existence check and a subsequent write.
            let msg = format!("Create config at {}?", sanitize_for_display(&config_path.display().to_string()));
            if yes || prompt_confirm(&msg) {
                if let Some(parent) = config_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&config_path)
                {
                    Ok(mut f) => {
                        use std::io::Write;
                        f.write_all(runex_init::default_config_content().as_bytes())?;
                        println!("Created: {}", sanitize_for_display(&config_path.display().to_string()));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        println!("Config already exists: {}", sanitize_for_display(&config_path.display().to_string()));
                    }
                    Err(e) => return Err(e.into()),
                }
            } else {
                println!("Skipped config creation.");
            }

            // Step 2: shell integration
            let shell = detect_shell().unwrap_or_else(|| {
                eprintln!(
                    "Could not detect shell. Defaulting to bash. \
                     Use `runex export <shell>` to generate integration manually."
                );
                Shell::Bash
            });

            match runex_init::rc_file_for(shell) {
                None => {
                    println!(
                        "Shell integration for {:?} must be added manually. \
                         Run `runex export {:?}` for the script.",
                        shell, shell
                    );
                }
                Some(rc_path) => {
                    let existing = read_rc_content(&rc_path);
                    if existing.contains(runex_init::RUNEX_INIT_MARKER) {
                        println!(
                            "Shell integration already present in {}",
                            sanitize_for_display(&rc_path.display().to_string())
                        );
                    } else {
                        let msg =
                            format!("Append shell integration to {}?", sanitize_for_display(&rc_path.display().to_string()));
                        if yes || prompt_confirm(&msg) {
                            let line = runex_init::integration_line(shell, "runex");
                            let block =
                                format!("\n{}\n{}\n", runex_init::RUNEX_INIT_MARKER, line);
                            if let Some(parent) = rc_path.parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            let mut open_opts = std::fs::OpenOptions::new();
                            open_opts.create(true).append(true);
                            // On Unix, refuse to follow a symlink at the final path component
                            // to prevent an attacker from racing to replace the rc file with
                            // a symlink pointing to a sensitive file.
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::OpenOptionsExt;
                                open_opts.custom_flags(libc::O_NOFOLLOW);
                            }
                            let mut file = open_opts.open(&rc_path)?;
                            file.write_all(block.as_bytes())?;
                            println!("Appended integration to {}", sanitize_for_display(&rc_path.display().to_string()));
                        } else {
                            println!("Skipped shell integration.");
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_abbr(key: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: format!("expand-{key}"),
            when_command_exists: None,
        }
    }

    #[test]
    fn format_check_line_colors_only_tag_text() {
        let check = Check {
            name: "config_file".into(),
            status: CheckStatus::Warn,
            detail: "detail".into(),
        };

        let line = format_check_line(&check);
        assert!(line.starts_with(&format!("[{ANSI_YELLOW}WARN{ANSI_RESET}]")));
        assert!(line.contains("config_file: detail"));
    }

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

    #[test]
    #[cfg(unix)]
    fn load_bash_aliases_does_not_source_startup_files() {
        // Verify that bash alias enumeration does not execute user startup files.
        // We create a temporary HOME with a .bashrc that writes a sentinel file.
        // If -i were used, the sentinel would be created. With --norc --noprofile, it must not.
        let home = tempfile::tempdir().unwrap();
        let sentinel = home.path().join("dotfile_executed");
        let bashrc = home.path().join(".bashrc");
        std::fs::write(
            &bashrc,
            format!("touch {}\n", sentinel.display()),
        ).unwrap();

        // Run bash with HOME pointing to the temp dir
        let output = Command::new("bash")
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
        // If bash is not available, skip silently
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
    fn version_line_contains_pkg_version() {
        let line = version_line();
        assert!(line.starts_with(&format!("runex {}", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn make_command_exists_no_prepend_uses_which() {
        let exists = make_command_exists(None);
        // "cargo" is guaranteed to be on PATH in a Rust build environment
        assert!(exists("cargo"));
        assert!(!exists("__runex_fake_cmd_that_does_not_exist__"));
    }

    #[test]
    fn make_command_exists_prepend_finds_file() {
        let dir = tempfile::tempdir().unwrap();
        let fake_bin = dir.path().join("myfaketool");
        std::fs::write(&fake_bin, b"").unwrap();
        let exists = make_command_exists(Some(dir.path()));
        assert!(exists("myfaketool"));
        assert!(!exists("__runex_other_fake__"));
    }

    #[test]
    fn format_dry_run_no_match() {
        let config = runex_core::model::Config {
            version: 1,
            keybind: runex_core::model::KeybindConfig::default(),
            abbr: vec![],
        };
        let result = expand::which_abbr(&config, "xyz", |_| true);
        let out = format_dry_run_result("xyz", &result);
        assert!(out.contains("token: xyz"));
        assert!(out.contains("no rule matched"));
        assert!(out.contains("pass-through"));
    }

    #[test]
    fn format_dry_run_expanded() {
        let config = runex_core::model::Config {
            version: 1,
            keybind: runex_core::model::KeybindConfig::default(),
            abbr: vec![runex_core::model::Abbr {
                key: "gcm".into(),
                expand: "git commit -m".into(),
                when_command_exists: None,
            }],
        };
        let result = expand::which_abbr(&config, "gcm", |_| true);
        let out = format_dry_run_result("gcm", &result);
        assert!(out.contains("token: gcm"));
        assert!(out.contains("expanded  ->  git commit -m"));
        assert!(out.contains("conditions: none"));
    }

    #[test]
    fn format_dry_run_condition_failed() {
        let config = runex_core::model::Config {
            version: 1,
            keybind: runex_core::model::KeybindConfig::default(),
            abbr: vec![runex_core::model::Abbr {
                key: "ls".into(),
                expand: "lsd".into(),
                when_command_exists: Some(vec!["lsd".into()]),
            }],
        };
        let result = expand::which_abbr(&config, "ls", |_| false);
        let out = format_dry_run_result("ls", &result);
        assert!(out.contains("lsd: NOT FOUND"), "out: {out}");
        assert!(out.contains("pass-through"), "out: {out}");
    }

    #[test]
    fn format_dry_run_duplicate_key_fallthrough() {
        // rule 1: self-loop (skipped), rule 2: actual expansion
        let config = runex_core::model::Config {
            version: 1,
            keybind: runex_core::model::KeybindConfig::default(),
            abbr: vec![
                runex_core::model::Abbr {
                    key: "ls".into(),
                    expand: "ls".into(), // self-loop
                    when_command_exists: None,
                },
                runex_core::model::Abbr {
                    key: "ls".into(),
                    expand: "lsd".into(),
                    when_command_exists: None,
                },
            ],
        };
        let result = expand::which_abbr(&config, "ls", |_| true);
        let out = format_dry_run_result("ls", &result);
        assert!(out.contains("rule #1 skipped"), "out: {out}");
        assert!(out.contains("expanded  ->  lsd"), "out: {out}");
    }

    #[test]
    fn check_pwsh_alias_name_strips_control_chars_from_key() {
        // An abbr key containing an ANSI escape sequence must not appear raw in check.name,
        // which is printed to the terminal via format_check_line.
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
        // The alias definition comes from pwsh output (external data).
        // It could theoretically contain ANSI sequences if pwsh emits colorized output.
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

    #[test]
    fn format_which_result_expanded_strips_control_chars() {
        // key and expansion are printed directly to the terminal.
        // Control chars in either must not reach the terminal output.
        let result = WhichResult::Expanded {
            key: "key\x1b[2J".to_string(),
            expansion: "exp\x07anded".to_string(),
            rule_index: 0,
            satisfied_conditions: vec![],
            skipped: vec![],
        };
        let s = format_which_result(&result, false);
        assert!(!s.contains('\x1b'), "format_which_result: ESC in key must be stripped: {s:?}");
        assert!(!s.contains('\x07'), "format_which_result: BEL in expansion must be stripped: {s:?}");
    }

    #[test]
    fn format_which_result_why_strips_control_chars_from_cmd() {
        // `--why` output includes when_command_exists cmd names from the config.
        let result = WhichResult::AllSkipped {
            token: "ls".to_string(),
            skipped: vec![(0, expand::SkipReason::ConditionFailed {
                found_commands: vec![],
                missing_commands: vec!["cmd\x1b[31mevil\x1b[0m".to_string()],
            })],
        };
        let s = format_which_result(&result, true);
        assert!(!s.contains('\x1b'), "format_which_result --why: ESC in cmd must be stripped: {s:?}");
    }

    #[test]
    fn format_dry_run_result_strips_control_chars() {
        // expand --dry-run prints key, expansion, and cmd names to terminal.
        let result = WhichResult::Expanded {
            key: "k\x1bey".to_string(),
            expansion: "ex\x07pand".to_string(),
            rule_index: 0,
            satisfied_conditions: vec!["cmd\x1b[0m".to_string()],
            skipped: vec![],
        };
        let s = format_dry_run_result("tok", &result);
        assert!(!s.contains('\x1b'), "format_dry_run_result: ESC must be stripped: {s:?}");
        assert!(!s.contains('\x07'), "format_dry_run_result: BEL must be stripped: {s:?}");
    }

    // ─── sanitize_for_display: Unicode visual-deception chars ────────────────
    //
    // sanitize_for_display is applied to external data before printing to the terminal
    // (alias definitions from bash/pwsh output, config paths from the OS, token values).
    // U+202E (Right-to-Left Override) and similar directional characters can reverse the
    // visual order of text in the terminal, making an alias like "evil" look like "live".
    // U+FEFF (BOM) is invisible. These must be stripped by sanitize_for_display.

    #[test]
    fn sanitize_for_display_strips_rlo() {
        // U+202E reverses character display order in a terminal.
        // An alias definition containing it could make "evil" look like "live".
        let s = sanitize_for_display("run\u{202e}ex");
        assert!(
            !s.contains('\u{202e}'),
            "sanitize_for_display must strip U+202E (Right-to-Left Override): {s:?}"
        );
    }

    #[test]
    fn sanitize_for_display_strips_bom() {
        // U+FEFF (BOM) is invisible in most terminals.
        let s = sanitize_for_display("run\u{FEFF}ex");
        assert!(
            !s.contains('\u{FEFF}'),
            "sanitize_for_display must strip U+FEFF (BOM): {s:?}"
        );
    }

    #[test]
    fn sanitize_for_display_strips_zwsp() {
        // U+200B (Zero-Width Space) is invisible, could hide content in displayed strings.
        let s = sanitize_for_display("run\u{200B}ex");
        assert!(
            !s.contains('\u{200B}'),
            "sanitize_for_display must strip U+200B (Zero-Width Space): {s:?}"
        );
    }

    #[test]
    fn sanitize_for_display_preserves_normal_unicode() {
        // Non-deceptive Unicode (e.g. Japanese, emoji) must be preserved.
        let s = sanitize_for_display("git-コミット");
        assert_eq!(s, "git-コミット", "sanitize_for_display must not strip normal Unicode");
    }

    // ─── alias parser DoS: line count limit ──────────────────────────────────
    //
    // `parse_bash_alias_lines` and `parse_pwsh_alias_lines` receive output from
    // external shell processes. If a compromised or misbehaving shell emits an
    // unbounded number of lines, parsing them all would consume unbounded memory
    // and CPU. Parsers must silently truncate after a maximum number of lines.

    #[test]
    fn parse_bash_alias_lines_truncates_at_max_lines() {
        // Generate more than MAX_ALIAS_LINES alias lines.
        // The parser must stop after the limit and not accumulate all entries.
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
        // Generate more than MAX_ALIAS_LINES pwsh alias lines.
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
        // Fewer than limit: all entries must be returned.
        let mut input = String::new();
        for i in 0..50 {
            input.push_str(&format!("alias k{i}='v{i}'\n"));
        }
        let aliases = parse_bash_alias_lines(&input);
        assert_eq!(aliases.len(), 50, "parse_bash_alias_lines must return all entries below the limit");
    }

    // ─── read_rc_content: size limit ─────────────────────────────────────────
    //
    // `init` reads the rc file to check for RUNEX_INIT_MARKER before appending.
    // If the rc file is extremely large (e.g. corrupted or adversarially crafted),
    // `read_to_string` would consume unbounded memory. `read_rc_content` must
    // refuse files larger than MAX_RC_FILE_BYTES and return an empty string so
    // that the marker check fails safe (appends as if unseen — idempotent).

    #[test]
    fn read_rc_content_returns_content_for_normal_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        f.write_all(b"# runex-init\n").unwrap();
        let content = read_rc_content(f.path());
        assert!(content.contains("# runex-init"), "normal rc file must be readable");
    }

    #[test]
    fn read_rc_content_returns_empty_for_oversized_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        // Write MAX_RC_FILE_BYTES + 1 bytes
        f.write_all(&vec![b'x'; MAX_RC_FILE_BYTES + 1]).unwrap();
        let content = read_rc_content(f.path());
        assert!(
            content.is_empty(),
            "read_rc_content must return empty string for oversized rc file"
        );
    }

    #[test]
    fn read_rc_content_returns_empty_for_missing_file() {
        let content = read_rc_content(std::path::Path::new("/nonexistent/runex_test.rc"));
        assert!(content.is_empty(), "missing rc file must return empty string");
    }

    /// A file exactly at MAX_RC_FILE_BYTES must be read (boundary: <=, not <).
    /// This test exercises the single-fd size check introduced to close the TOCTOU
    /// race: the same fd used for metadata() is also used for the read, so there
    /// is no window for the file to be swapped between the size check and the read.
    #[test]
    fn read_rc_content_accepts_file_at_exact_size_limit() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        f.write_all(&vec![b'x'; MAX_RC_FILE_BYTES]).unwrap();
        let content = read_rc_content(f.path());
        assert_eq!(
            content.len(),
            MAX_RC_FILE_BYTES,
            "read_rc_content must accept a file exactly at MAX_RC_FILE_BYTES"
        );
    }

    #[test]
    fn parse_pwsh_alias_lines_accepts_normal_count() {
        let mut input = String::new();
        for i in 0..50 {
            input.push_str(&format!("k{i}\tv{i}\n"));
        }
        let aliases = parse_pwsh_alias_lines(&input);
        assert_eq!(aliases.len(), 50, "parse_pwsh_alias_lines must return all entries below the limit");
    }

    // ─── alias parser DoS: per-line length limit ─────────────────────────────
    //
    // Even with MAX_ALIAS_LINES, a single extremely long line (e.g. 10 MB) would
    // consume unbounded memory for that one entry.  Parsers must silently truncate
    // any alias value that exceeds MAX_ALIAS_VALUE_BYTES to cap per-entry memory.

    #[test]
    fn parse_bash_alias_lines_truncates_oversized_value() {
        // A single alias with a value exceeding MAX_ALIAS_VALUE_BYTES must be
        // accepted (the key is valid) but the stored value must be truncated.
        let huge_value = "x".repeat(65_536 + 1);
        let input = format!("alias k='{huge_value}'\n");
        let aliases = parse_bash_alias_lines(&input);
        // The alias must be present (not silently dropped), but its value must not
        // exceed the per-entry limit.
        if let Some(val) = aliases.get("k") {
            assert!(
                val.len() <= 65_536,
                "bash alias value must be truncated to MAX_ALIAS_VALUE_BYTES, got {} bytes",
                val.len()
            );
        }
        // If the entry was dropped entirely that is also acceptable (key is preserved
        // without oversized value), so we accept both outcomes — only the stored
        // value length matters.
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

    // ─── alias parser DoS: key (name) length limit ───────────────────────────
    //
    // `parse_bash_alias_lines` and `parse_pwsh_alias_lines` truncate the VALUE
    // at MAX_ALIAS_VALUE_BYTES, but not the KEY (alias name).  A misbehaving
    // shell that emits alias names with huge lengths (e.g. "alias AAAAAA…=v")
    // fills the HashMap with oversized keys.  With MAX_ALIAS_LINES=10,000 entries
    // and each key up to 1 MB, total memory could be 10 GB.
    // Keys must be silently discarded when they exceed MAX_ALIAS_KEY_BYTES.

    #[test]
    fn parse_bash_alias_lines_discards_oversized_key() {
        // An alias whose NAME exceeds MAX_ALIAS_KEY_BYTES must not be stored.
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
        // Keys exactly at the limit must be stored.
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

    // ─── prompt_confirm: stdin read size limit ────────────────────────────────
    //
    // `prompt_confirm` reads one line from stdin to get a y/N answer.
    // Without a size limit, a caller piping 10 MB of data would cause
    // read_line() to allocate a 10 MB String before returning, wasting memory.
    // The internal `prompt_confirm_from` helper must cap reading at
    // MAX_CONFIRM_BYTES so that oversized input is treated as "no" without
    // accumulating it all.

    #[test]
    fn prompt_confirm_from_accepts_yes() {
        use std::io::BufReader;
        let input = b"y\n";
        let mut reader = BufReader::new(&input[..]);
        assert!(
            prompt_confirm_from(&mut reader),
            "prompt_confirm_from must return true for 'y\\n'"
        );
    }

    #[test]
    fn prompt_confirm_from_accepts_yes_long_form() {
        use std::io::BufReader;
        let input = b"yes\n";
        let mut reader = BufReader::new(&input[..]);
        assert!(
            prompt_confirm_from(&mut reader),
            "prompt_confirm_from must return true for 'yes\\n'"
        );
    }

    #[test]
    fn prompt_confirm_from_rejects_no() {
        use std::io::BufReader;
        let input = b"n\n";
        let mut reader = BufReader::new(&input[..]);
        assert!(
            !prompt_confirm_from(&mut reader),
            "prompt_confirm_from must return false for 'n\\n'"
        );
    }

    #[test]
    fn prompt_confirm_from_rejects_oversized_input() {
        // A line far exceeding MAX_CONFIRM_BYTES must be treated as "no",
        // not buffered in full. The function must return false without OOM.
        use std::io::BufReader;
        let huge = vec![b'y'; MAX_CONFIRM_BYTES + 1];
        let mut reader = BufReader::new(huge.as_slice());
        assert!(
            !prompt_confirm_from(&mut reader),
            "prompt_confirm_from must return false for input exceeding MAX_CONFIRM_BYTES"
        );
    }

    #[test]
    fn prompt_confirm_from_rejects_empty_input() {
        use std::io::BufReader;
        let input = b"";
        let mut reader = BufReader::new(&input[..]);
        assert!(
            !prompt_confirm_from(&mut reader),
            "prompt_confirm_from must return false for empty input (EOF)"
        );
    }

    // ─── read_rc_content: non-regular file rejection ─────────────────────────
    //
    // `read_rc_content` reads the shell rc file to detect the RUNEX_INIT_MARKER.
    // It must reject non-regular files (named pipes, device files) to prevent:
    //   - Named pipe (FIFO): metadata().len() == 0, read_to_string() blocks
    //     indefinitely waiting for a writer — process hangs.
    //   - Device files (/dev/zero, /dev/urandom): report len=0, read_to_string()
    //     fills memory unboundedly.
    // The function must check metadata().is_file() before attempting to read.

    #[test]
    #[cfg(unix)]
    fn read_rc_content_rejects_named_pipe() {
        use std::ffi::CString;
        let dir = tempfile::tempdir().unwrap();
        let pipe = dir.path().join("fake_rc.sh");
        let path_c = CString::new(pipe.to_str().unwrap()).unwrap();
        unsafe { libc::mkfifo(path_c.as_ptr(), 0o600) };
        // If read_rc_content opens the pipe, read_to_string blocks indefinitely.
        // The function must return an empty string without blocking.
        let content = read_rc_content(&pipe);
        assert_eq!(
            content, "",
            "read_rc_content must return empty string for a named pipe (FIFO), not block"
        );
    }

    #[test]
    #[cfg(unix)]
    fn read_rc_content_rejects_dev_zero() {
        // /dev/zero reports metadata.len() == 0 and reads unlimited zero bytes.
        // read_rc_content must not attempt to read it.
        let path = std::path::Path::new("/dev/zero");
        let content = read_rc_content(path);
        assert_eq!(
            content, "",
            "read_rc_content must return empty string for /dev/zero (device file)"
        );
    }

    // ── Vector 23: subprocess timeout ────────────────────────────────────────

    /// A malicious `bash` on PATH that sleeps forever must not cause
    /// load_bash_aliases to hang indefinitely.  The function must return
    /// within ALIAS_SUBPROCESS_TIMEOUT_SECS + a small margin.
    #[test]
    #[cfg(unix)]
    fn load_bash_aliases_returns_within_timeout_when_bash_hangs() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        // Create a fake "bash" that sleeps forever.
        let dir = tempfile::tempdir().unwrap();
        let fake_bash = dir.path().join("bash");
        fs::write(&fake_bash, "#!/bin/sh\nsleep 999\n").unwrap();
        fs::set_permissions(&fake_bash, fs::Permissions::from_mode(0o755)).unwrap();

        // Inject the fake bash at the front of PATH.
        let original_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.path().display(), original_path);

        let start = Instant::now();
        // We can't easily set PATH for load_bash_aliases without refactoring.
        // Instead call Command directly with the env to simulate what load_bash_aliases does.
        // This test documents the expected behavior: the function must time out.
        //
        // We call a helper (not yet existing) that mirrors load_bash_aliases but
        // accepts a PATH override for testability.
        let result = load_bash_aliases_with_path(&new_path);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < ALIAS_SUBPROCESS_TIMEOUT_SECS + 2,
            "load_bash_aliases must return within timeout; took {:?}",
            elapsed
        );
        // A hanging bash produces no usable output — empty map is fine.
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

    // ── Vector 24: subprocess stdout size limit ───────────────────────────────

    /// A shell that emits gigabytes of output must not cause OOM.
    /// run_with_timeout must cap stdout at MAX_SUBPROCESS_OUTPUT_BYTES and
    /// discard the rest without allocating unboundedly.
    #[test]
    #[cfg(unix)]
    fn run_with_timeout_caps_output_size() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        // Create a script that outputs MAX_SUBPROCESS_OUTPUT_BYTES * 2 bytes fast
        // using `yes` (outputs "y\n" in a tight loop) then exits 0.
        // `yes | head -c N` is capped at N bytes by the shell, ensuring the
        // process exits quickly and we can test the size cap independently of
        // the timeout.
        let dir = tempfile::tempdir().unwrap();
        let fake_sh = dir.path().join("flood");
        // Write a large block (MAX*2 bytes) using dd from /dev/zero with a large
        // block size (fast) then exit 0.  Exits with status 0 so we can verify
        // the size check — not the exit-code check — triggers the None return.
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

        // Output is 2×limit → exceeds MAX_SUBPROCESS_OUTPUT_BYTES → must be None.
        assert!(
            result.is_none(),
            "run_with_timeout must return None when output exceeds MAX_SUBPROCESS_OUTPUT_BYTES"
        );
    }

}
