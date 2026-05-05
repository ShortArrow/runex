mod format;
mod shell_alias;
#[cfg(windows)]
mod win_path;

use clap::{Parser, Subcommand};
use format::{
    format_check_line, format_dry_run_result, format_which_result, version_line,
    which_result_to_json,
};
use runex_core::config::{default_config_path, load_config};
use runex_core::doctor;
use runex_core::expand;
use runex_core::init as runex_init;
use runex_core::model::{Config, ExpandResult};
use runex_core::sanitize::sanitize_for_display;
use runex_core::shell::Shell;
use shell_alias::add_shell_alias_conflicts;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

pub(crate) const ANSI_RESET: &str = "\x1b[0m";
pub(crate) const ANSI_GREEN: &str = "\x1b[32m";
pub(crate) const ANSI_RED: &str = "\x1b[31m";
pub(crate) const ANSI_YELLOW: &str = "\x1b[33m";
pub(crate) const GIT_COMMIT: Option<&str> = option_env!("RUNEX_GIT_COMMIT");

/// Column width for the check status tag in `doctor` output.
pub(crate) const CHECK_TAG_WIDTH: usize = 8;

/// Maximum byte length of the `--bin` argument passed to `export`.
const MAX_BIN_LEN: usize = 255;

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
        /// Current shell (bash, zsh, pwsh, clink, nu); auto-detected if omitted
        #[arg(long, value_name = "SHELL")]
        shell: Option<String>,
    },
    /// List all abbreviations
    List {
        /// Current shell (bash, zsh, pwsh, clink, nu); auto-detected if omitted
        #[arg(long, value_name = "SHELL")]
        shell: Option<String>,
    },
    /// Check environment health
    Doctor {
        /// Skip shell alias conflict checks (avoids spawning pwsh/bash)
        #[arg(long)]
        no_shell_aliases: bool,
        /// Show full error details (e.g. multi-line parse errors)
        #[arg(long)]
        verbose: bool,
        /// Warn about unknown fields in config (catches typos like [[abr]])
        #[arg(long)]
        strict: bool,
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
        /// Current shell (bash, zsh, pwsh, clink, nu); auto-detected if omitted
        #[arg(long, value_name = "SHELL")]
        shell: Option<String>,
    },
    /// Pre-compute command existence cache for shell startup.
    ///
    /// Hidden since 0.2.0: shell templates no longer call this subcommand;
    /// the hook subcommand evaluates `when_command_exists` per keypress
    /// instead. The command is retained for one release for backward
    /// compatibility and may be removed in a future version.
    #[command(hide = true)]
    Precache {
        /// Target shell: bash, zsh, pwsh, clink, nu
        #[arg(long, value_name = "SHELL")]
        shell: String,
        /// Print the list of commands to check (for external resolution)
        #[arg(long)]
        list_commands: bool,
        /// Use externally resolved command existence results instead of which
        #[arg(long, value_name = "RESOLVED")]
        resolved: Option<String>,
    },
    /// Show per-phase timing breakdown of the expand flow
    Timings {
        /// Abbreviation key to time (if omitted, times all keys)
        key: Option<String>,
        /// Current shell (bash, zsh, pwsh, clink, nu); auto-detected if omitted
        #[arg(long, value_name = "SHELL")]
        shell: Option<String>,
    },
    /// Add an abbreviation rule to the config file
    Add {
        /// Abbreviation key (e.g. "gcm")
        key: String,
        /// Expansion text (e.g. "git commit -m")
        expand: String,
        /// Only expand when these commands exist on PATH
        #[arg(long, value_name = "CMD", num_args = 1..)]
        when: Option<Vec<String>>,
    },
    /// Remove an abbreviation rule from the config file
    Remove {
        /// Abbreviation key to remove
        key: String,
    },
    /// Initialize runex: create config and add shell integration
    Init {
        /// Target shell (bash, zsh, pwsh, clink, nu). When omitted,
        /// runex auto-detects from $SHELL / $PSModulePath.
        shell: Option<String>,
        /// Skip confirmation prompts
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Per-keystroke hook — called by the shell integration wrapper on every
    /// trigger-key press. Returns shell-specific eval text describing the new
    /// buffer/cursor state. Exit code 2 means "nothing to do; shell may
    /// insert the literal trigger key".
    #[command(hide = true)]
    Hook {
        /// Target shell: bash, zsh, pwsh, clink, nu
        #[arg(long, value_name = "SHELL")]
        shell: String,
        /// Current buffer contents
        #[arg(long)]
        line: String,
        /// Current cursor position as a byte offset into `line`
        #[arg(long)]
        cursor: usize,
        /// True if a paste is in progress (pwsh). When set, the hook skips
        /// expansion and emits a plain space-insert action.
        #[arg(long)]
        paste_pending: bool,
    },
}


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
///
/// Returns the config path, the parsed config (or None), and the error message if parsing failed.
fn resolve_config_opt(config_override: Option<&Path>) -> (PathBuf, Option<Config>, Option<String>) {
    if let Some(path) = config_override {
        let result = load_config(path);
        let err = result.as_ref().err().map(|e| e.to_string());
        return (path.to_path_buf(), result.ok(), err);
    }
    let path = default_config_path().unwrap_or_default();
    let result = load_config(&path);
    let err = result.as_ref().err().map(|e| e.to_string());
    (path, result.ok(), err)
}


/// Compute the precache fingerprint for the current environment.
fn compute_precache_fingerprint(config_path: &Path, shell: &str) -> String {
    let path_env = std::env::var("PATH").unwrap_or_default();
    let mtime = runex_core::precache::config_mtime(config_path);
    runex_core::precache::compute_fingerprint(&path_env, mtime, shell)
}

/// Build a `command_exists` closure with precache hint layer.
///
/// When `path_prepend` is `Some(dir)`, files inside `dir` are checked first
/// (bare name, and `.exe` on Windows). Falls through to `which::which`.
///
/// Rejects any `cmd` containing `/`, `\`, or `:` because `when_command_exists`
/// values must be bare command names, not filesystem paths. Accepting paths would
/// allow directory traversal and absolute-path probing via `dir.join(cmd)`.
///
/// ## Hint layer (precache)
///
/// If `RUNEX_CMD_CACHE_V1` env var contains a valid cache with matching fingerprint:
/// - `cache[cmd] == true` → return true immediately (skip `which`)
/// - `cache[cmd] == false` → re-check live (avoid stale false negatives after installs)
/// - `cmd` not in cache → live check
///
/// Results are also cached in a `RefCell<HashMap>` per invocation to avoid
/// repeated `which` calls within the same CLI run.
///
/// ## Windows-specific PATH augmentation
///
/// On Windows we feed `which::which_in` the *augmented* search path from
/// [`win_path::effective_search_path`] (process PATH + HKCU + HKLM) instead
/// of relying on the inherited `PATH` env var alone.
///
/// The reason is that some parent processes — most notably the cmd.exe
/// children that clink's Lua `io.popen` spawns — inherit a PATH that's
/// missing the User-scope entries the registry holds. Without
/// augmentation, `runex hook` running under clink would fail to find
/// binaries installed under `~/.cargo/bin`,
/// `~/AppData/Local/Microsoft/WinGet/Links`, or `~/AppData/Local/mise/shims`.
/// `when_command_exists` rules pointing at those binaries would then
/// silently evaluate false and abbreviations would no-op — looking like
/// an integration bug while the real cause is environmental. The
/// regression test `runex/tests/windows_path_isolation.rs` pins this
/// behavior so the failure mode can't return unnoticed.
fn make_command_exists<'a>(
    path_prepend: Option<&'a Path>,
    precache_fingerprint: Option<&str>,
) -> impl Fn(&str) -> bool + 'a {
    use runex_core::precache;

    let hint = precache_fingerprint.and_then(precache::load_cache);
    let cache = std::cell::RefCell::new(std::collections::HashMap::<String, bool>::new());
    // On Windows we resolve commands against an *augmented* search path
    // (process PATH + HKCU + HKLM) to cope with degraded environments
    // such as the cmd.exe children that clink's Lua `io.popen` spawns.
    // See `runex/src/win_path.rs` for the rationale and history. The
    // value is computed once per CLI invocation and reused.
    #[cfg(windows)]
    let effective_path = win_path::effective_search_path();

    move |cmd: &str| -> bool {
        if cmd.contains('/') || cmd.contains('\\') || cmd.contains(':') {
            return false;
        }

        // Per-invocation memoization (covers repeated checks within one run)
        if let Some(&cached) = cache.borrow().get(cmd) {
            return cached;
        }

        // Precache hint: trust true, re-check false
        if let Some(ref h) = hint {
            if let Some(&cached) = h.commands.get(cmd) {
                if cached {
                    cache.borrow_mut().insert(cmd.to_owned(), true);
                    return true;
                }
                // cached == false → fall through to live check (may have been installed)
            }
        }

        let live_check = |c: &str| -> bool {
            #[cfg(windows)]
            {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                which::which_in(c, Some(&effective_path.combined), &cwd).is_ok()
            }
            #[cfg(not(windows))]
            {
                which::which(c).is_ok()
            }
        };

        let exists = if let Some(dir) = path_prepend {
            if dir.join(cmd).is_file() {
                true
            } else {
                #[cfg(windows)]
                {
                    if dir.join(format!("{cmd}.exe")).is_file() {
                        true
                    } else {
                        live_check(cmd)
                    }
                }
                #[cfg(not(windows))]
                {
                    live_check(cmd)
                }
            }
        } else {
            live_check(cmd)
        };

        cache.borrow_mut().insert(cmd.to_owned(), exists);
        exists
    }
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
    #[cfg(unix)]
    let mut file = {
        // `O_NOFOLLOW` matches the policy of the write side
        // (`install_rcfile_integration`). Without it, the marker check
        // here could decide "already present" by reading through a
        // symlink target, while the write side would refuse to follow
        // and try to append. Pin both sides to "no symlinks at the
        // final path component" so init's read decision and write
        // decision agree.
        use std::os::unix::fs::OpenOptionsExt;
        match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
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
    let meta = match file.metadata() {
        Ok(m) => m,
        Err(_) => return String::new(),
    };
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

/// Infer the current shell from environment variables.
///
/// On Unix, reads `$SHELL`. On Windows, the presence of `PSModulePath` indicates
/// a PowerShell parent process.
fn detect_shell() -> Option<Shell> {
    if let Ok(sh) = std::env::var("SHELL") {
        let base = Path::new(&sh)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if let Ok(s) = base.parse::<Shell>() {
            return Some(s);
        }
    }
    if std::env::var("PSModulePath").is_ok() {
        return Some(Shell::Pwsh);
    }
    None
}

/// Resolve shell from optional `--shell` flag, falling back to `detect_shell()`.
///
/// Returns `None` when no shell could be determined (both flag absent and detection failed).
fn resolve_shell(shell_flag: Option<&str>) -> Result<Option<Shell>, Box<dyn std::error::Error>> {
    if let Some(s) = shell_flag {
        let sh = s.parse::<Shell>().map_err(|e: runex_core::shell::ShellParseError| {
            Box::<dyn std::error::Error>::from(e.to_string())
        })?;
        return Ok(Some(sh));
    }
    Ok(detect_shell())
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
    let mut limited = reader.by_ref().take(MAX_CONFIRM_BYTES as u64 + 1);
    match limited.read_line(&mut input) {
        Err(_) => return false,
        Ok(0) => return false,
        Ok(_) => {}
    }
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


/// Maximum byte length accepted for `--token` (expand) and `which <token>`.
/// Tokens longer than any possible abbr key (MAX_KEY_BYTES = 1024 in config.rs)
/// can never match and would cause needless memory allocation in sanitize_for_display.
const MAX_TOKEN_BYTES: usize = 1_024;

/// Outcome of a CLI subcommand handler. The handler reports either
/// success (the process should exit 0) or a request to exit with a
/// non-zero code. `main()` is the *only* function that calls
/// `process::exit` — handlers return outcomes so they can be
/// exercised from unit tests without taking the test process down
/// with them. Unrecoverable errors still bubble up via the `Err`
/// variant of `CmdResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdOutcome {
    /// Handler succeeded; main exits with 0.
    Ok,
    /// Handler requests a specific exit code (typically 1 for
    /// validation failures or doctor-found-errors). Used in place
    /// of `std::process::exit(n)` inside handlers.
    ExitCode(i32),
}

type CmdResult = Result<CmdOutcome, Box<dyn std::error::Error>>;

fn handle_version(json: bool) -> CmdResult {
    if json {
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
    Ok(CmdOutcome::Ok)
}

fn handle_list(config: &Config, shell: Option<Shell>, json: bool) -> CmdResult {
    if json {
        println!("{}", serde_json::to_string_pretty(&config.abbr)?);
    } else {
        for (key, exp) in expand::list(config, shell) {
            println!("{}\t{}", sanitize_for_display(key), sanitize_for_display(&exp));
        }
    }
    Ok(CmdOutcome::Ok)
}

fn handle_which(
    token: String,
    config: &Config,
    shell: Shell,
    command_exists: &dyn Fn(&str) -> bool,
    json: bool,
    why: bool,
) -> CmdResult {
    if token.len() > MAX_TOKEN_BYTES {
        eprintln!(
            "error: token is too long ({} bytes); maximum is {MAX_TOKEN_BYTES}",
            token.len()
        );
        return Ok(CmdOutcome::ExitCode(1));
    }
    let result = expand::which_abbr(config, &token, shell, command_exists);
    if json {
        println!("{}", serde_json::to_string_pretty(&which_result_to_json(&result))?);
    } else {
        println!("{}", format_which_result(&result, why));
    }
    Ok(CmdOutcome::Ok)
}

fn handle_expand(
    token: String,
    config: &Config,
    shell: Shell,
    command_exists: &dyn Fn(&str) -> bool,
    json: bool,
    dry_run: bool,
) -> CmdResult {
    if token.len() > MAX_TOKEN_BYTES {
        eprintln!(
            "error: --token is too long ({} bytes); maximum is {MAX_TOKEN_BYTES}",
            token.len()
        );
        return Ok(CmdOutcome::ExitCode(1));
    }
    if dry_run {
        let result = expand::which_abbr(config, &token, shell, command_exists);
        if json {
            println!("{}", serde_json::to_string_pretty(&which_result_to_json(&result))?);
        } else {
            print!("{}", format_dry_run_result(&token, &result));
        }
    } else {
        let result = expand::expand(config, &token, shell, command_exists);
        if json {
            let v = match &result {
                ExpandResult::Expanded { text: s, .. } => serde_json::json!({
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
                ExpandResult::Expanded { text, cursor_offset } => {
                    if let Some(offset) = cursor_offset {
                        // Output text + unit separator + cursor offset for shell templates
                        print!("{text}\x1f{offset}");
                    } else {
                        print!("{text}");
                    }
                }
                ExpandResult::PassThrough(s) => print!("{s}"),
            }
        }
    }
    Ok(CmdOutcome::Ok)
}

/// Validate the `--bin` argument for `export`.
///
/// Rejects values that are empty, whitespace-only, too long, contain control
/// characters, or contain non-printable-ASCII characters. Only printable ASCII
/// is allowed to prevent Unicode homoglyphs and bidirectional overrides from
/// being silently embedded in generated shell scripts.
///
/// Returns the error message to surface to the user on validation
/// failure; `Ok(())` when the value passes. Caller is responsible
/// for `eprintln!`ing the message and returning
/// `CmdOutcome::ExitCode(1)` — keeping `validate_bin` itself I/O-free
/// makes it a pure function the unit tests can drive directly.
fn validate_bin(bin: &str) -> Result<(), String> {
    if bin.trim().is_empty() {
        return Err("--bin must not be empty or whitespace-only".into());
    }
    if bin.len() > MAX_BIN_LEN {
        return Err(format!(
            "--bin is too long ({} bytes); maximum is {MAX_BIN_LEN}",
            bin.len()
        ));
    }
    if bin.chars().any(|c| c.is_ascii_control() || c == '\u{0085}' || c == '\u{2028}' || c == '\u{2029}') {
        return Err("--bin contains an invalid control character".into());
    }
    if bin.chars().any(|c| !c.is_ascii() || !c.is_ascii_graphic()) {
        return Err("--bin must contain only printable ASCII characters".into());
    }
    Ok(())
}

fn handle_export(
    shell: String,
    bin: String,
    config_flag: Option<&Path>,
) -> CmdResult {
    if let Err(msg) = validate_bin(&bin) {
        eprintln!("error: {msg}");
        return Ok(CmdOutcome::ExitCode(1));
    }
    let s: Shell = shell.parse().map_err(|e: runex_core::shell::ShellParseError| {
        Box::<dyn std::error::Error>::from(e.to_string())
    })?;
    let config = if config_flag.is_some() {
        let (_path, cfg) = resolve_config(config_flag)?;
        Some(cfg)
    } else {
        let (_path, cfg, _err) = resolve_config_opt(None);
        cfg
    };
    // For clink, default-bin must resolve to an absolute path.
    //
    // Why: clink invokes runex via Lua's `io.popen` which spawns a fresh
    // cmd.exe child. That child inherits whatever PATH the clink-injected
    // host process happens to have, and on real machines that PATH is
    // sometimes degraded (e.g. system-only, with the User scope from
    // HKCU not yet merged in). A bare `runex` command in the lua script
    // would then fail to resolve. Embedding the absolute path of the
    // currently-running executable (which is by definition reachable —
    // we ourselves were just launched from it) sidesteps the entire
    // PATH-inheritance question for clink.
    //
    // Other shells (bash/zsh/pwsh/nu) rely on PATH-resolved bare names
    // because they're invoked from rcfiles where PATH is already correct,
    // and because users can plausibly want to override which `runex`
    // gets used. Only clink gets the absolute-path treatment.
    let effective_bin = if matches!(s, Shell::Clink) && bin == "runex" {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or(bin)
    } else {
        bin
    };
    print!("{}", runex_core::shell::export_script(s, &effective_bin, config.as_ref()));
    Ok(CmdOutcome::Ok)
}

fn handle_precache(
    shell: String,
    list_commands: bool,
    resolved: Option<String>,
    config_flag: Option<&Path>,
    path_prepend: Option<&Path>,
) -> CmdResult {
    use runex_core::precache;

    let s: Shell = shell.parse().map_err(|e: runex_core::shell::ShellParseError| {
        Box::<dyn std::error::Error>::from(e.to_string())
    })?;
    let shell_name = format!("{s:?}").to_lowercase();

    let (config_path, config) = resolve_config(config_flag)?;

    // Mode 1: print comma-separated list of commands to check externally.
    // When path_only is true, output nothing so the shell template falls
    // back to which-based precache.
    if list_commands {
        if !config.precache.path_only {
            let cmds = precache::collect_unique_commands(&config);
            print!("{}", cmds.join(","));
        }
        return Ok(CmdOutcome::Ok);
    }

    let fp = compute_precache_fingerprint(&config_path, &shell_name);

    // Mode 2: use externally resolved results instead of which::which()
    if let Some(resolved_str) = resolved {
        let cache = precache::build_cache_from_resolved(&config, &fp, &resolved_str);
        let json = precache::cache_to_json(&cache);
        println!("{}", precache::export_statement(&shell_name, &json));
        return Ok(CmdOutcome::Ok);
    }

    // Default: use which::which() for command existence checks
    let command_exists = make_command_exists(path_prepend, None);
    let cache = precache::build_cache(&config, &fp, &command_exists);
    let json = precache::cache_to_json(&cache);

    println!("{}", precache::export_statement(&shell_name, &json));
    Ok(CmdOutcome::Ok)
}

fn handle_timings(
    key: Option<String>,
    shell_str: Option<String>,
    config_flag: Option<&Path>,
    path_prepend: Option<&Path>,
    json: bool,
) -> CmdResult {
    use runex_core::timings::{PhaseTimer, Timings};

    let mut timings = Timings::new();

    let t = PhaseTimer::start();
    let (config_path, config) = resolve_config(config_flag)?;
    timings.record_phase("config_load", t.elapsed());

    let t = PhaseTimer::start();
    let shell = resolve_shell(shell_str.as_deref())?.unwrap_or(Shell::Bash);
    timings.record_phase("shell_resolve", t.elapsed());

    let fp = compute_precache_fingerprint(&config_path, &format!("{shell:?}").to_lowercase());
    let command_exists = make_command_exists(path_prepend, Some(&fp));

    match key {
        Some(k) => {
            if k.len() > MAX_TOKEN_BYTES {
                eprintln!(
                    "error: key is too long ({} bytes); maximum is {MAX_TOKEN_BYTES}",
                    k.len()
                );
                return Ok(CmdOutcome::ExitCode(1));
            }
            expand::expand_timed(&config, &k, shell, &command_exists, &mut timings);
        }
        None => {
            // Time each unique abbr key
            let keys: Vec<String> = config.abbr.iter().map(|a| a.key.clone()).collect();
            let unique_keys: Vec<String> = {
                let mut seen = std::collections::HashSet::new();
                keys.into_iter().filter(|k| seen.insert(k.clone())).collect()
            };
            for key in &unique_keys {
                expand::expand_timed(&config, key, shell, &command_exists, &mut timings);
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&format::format_timings_json(&timings))?);
    } else {
        print!("{}", format::format_timings_table(&timings));
    }
    Ok(CmdOutcome::Ok)
}

/// Compose the [`doctor::DoctorEnvInfo`] that the `doctor` subcommand
/// passes alongside the config checks. Today this only sets the
/// Windows effective-search-path breakdown; on other platforms only
/// the integration-check fields apply.
fn build_doctor_env_info(config: Option<&Config>) -> doctor::DoctorEnvInfo {
    let mut info = doctor::DoctorEnvInfo::default();

    #[cfg(windows)]
    {
        let p = win_path::effective_search_path();
        info.effective_search_path = Some(doctor::EffectiveSearchPathSummary {
            from_process: p.from_process,
            from_user_registry: p.from_user_registry,
            from_system_registry: p.from_system_registry,
        });
    }

    // Render the canonical clink export so doctor can detect drift on
    // disk. Use the absolute path of our own executable as the bin
    // (matching `handle_export`'s clink full-path fallback) so a fresh
    // `runex doctor` after upgrade matches what `runex init clink`
    // would write today.
    let clink_bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "runex".to_string());
    info.clink_export_for_drift_check = Some(runex_core::shell::export_script(
        Shell::Clink,
        &clink_bin,
        config,
    ));

    // We always want to know whether the user ran `runex init <shell>`
    // for each rcfile-bearing shell. doctor itself decides whether to
    // emit each row based on rcfile existence (a missing rcfile means
    // "user doesn't use that shell" and the check is skipped silently).
    info.check_rcfile_markers = doctor::RcfileMarkerSelection::all();

    info
}

fn handle_doctor(
    config_flag: Option<&Path>,
    path_prepend: Option<&Path>,
    no_shell_aliases: bool,
    verbose: bool,
    strict: bool,
    json: bool,
) -> CmdResult {
    let (config_path, config, parse_error) = resolve_config_opt(config_flag);
    // Doctor checks live command existence — no precache (intentional)
    let command_exists = make_command_exists(path_prepend, None);
    let spinner = Spinner::start("Checking environment...");
    // Build informational env-info that doctor renders alongside the
    // config checks: Windows effective_search_path breakdown (see
    // `runex/src/win_path.rs`), per-shell rcfile marker checks, and a
    // clink-lua drift check. The current config is forwarded so the
    // generated clink export reflects the user's keybinds & abbrs.
    let env_info = build_doctor_env_info(config.as_ref());
    let mut result = doctor::diagnose(
        &config_path,
        config.as_ref(),
        parse_error.as_deref(),
        &env_info,
        &command_exists,
    );
    if !no_shell_aliases {
        add_shell_alias_conflicts(&mut result, config.as_ref());
    }
    // Read config source once (O_NOFOLLOW, size-capped) and share across checks.
    let source = runex_core::config::read_config_source(&config_path).ok();

    // Always: report every rule rejected by per-field validation so users know
    // *all* the invalid fields, not just the first one that tripped parse_config.
    if let Some(src) = source.as_deref() {
        result.checks.extend(doctor::check_rejected_rules(src));
    }

    if strict {
        if let Some(src) = source.as_deref() {
            result.checks.extend(doctor::check_unknown_fields(src));
            result.checks.extend(doctor::check_precache_deprecation(src));
        }
        // Check for unreachable duplicate rules
        if let Some(cfg) = config.as_ref() {
            result.checks.extend(doctor::check_unreachable_duplicates(cfg));
        }
    }
    spinner.stop();

    if json {
        println!("{}", serde_json::to_string_pretty(&result.checks)?);
    } else {
        for check in &result.checks {
            println!("{}", format_check_line(check, verbose));
        }
    }

    if !result.is_healthy() {
        return Ok(CmdOutcome::ExitCode(1));
    }
    Ok(CmdOutcome::Ok)
}

fn handle_init(config_path: PathBuf, shell_override: Option<&str>, yes: bool) -> CmdResult {
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

    let shell = if let Some(s) = shell_override {
        s.parse::<Shell>().map_err(|e: runex_core::shell::ShellParseError| {
            Box::<dyn std::error::Error>::from(e.to_string())
        })?
    } else {
        detect_shell().unwrap_or_else(|| {
            eprintln!(
                "Could not detect shell. Defaulting to bash. \
                 Use `runex init <shell>` (e.g. `runex init pwsh`) to target a specific shell."
            );
            Shell::Bash
        })
    };

    let rc_path_for_next_steps = match shell {
        Shell::Clink => {
            install_clink_lua(yes, &config_path)?;
            None
        }
        _ => install_rcfile_integration(shell, yes)?,
    };

    println!();
    println!("{}", runex_init::next_steps_message(shell, rc_path_for_next_steps.as_deref()));
    Ok(CmdOutcome::Ok)
}

/// Append the integration block to the rcfile for `shell`. Returns the
/// rcfile path so the caller can show it in the Next-steps blurb.
///
/// Safety properties (documented in `docs/setup.md` for users):
///
/// - `OpenOptions::append` so existing rcfile content is **never**
///   overwritten — every byte we write goes after the file's current
///   end.
/// - `O_NOFOLLOW` on Unix so a symlink at `rc_path` doesn't redirect
///   the write to a different file.
/// - Idempotent: if `RUNEX_INIT_MARKER` is already present in the
///   file, we skip the append entirely (no duplicate blocks).
/// - User confirmation per write unless `--yes`.
fn install_rcfile_integration(shell: Shell, yes: bool) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let Some(rc_path) = runex_init::rc_file_for(shell) else {
        println!(
            "Shell integration for {:?} must be added manually. \
             Run `runex export {:?}` for the script.",
            shell, shell
        );
        return Ok(None);
    };
    let existing = read_rc_content(&rc_path);
    if existing.contains(runex_init::RUNEX_INIT_MARKER) {
        println!(
            "Shell integration already present in {}",
            sanitize_for_display(&rc_path.display().to_string())
        );
        return Ok(Some(rc_path));
    }
    let msg = format!(
        "Append shell integration to {}?",
        sanitize_for_display(&rc_path.display().to_string())
    );
    if !(yes || prompt_confirm(&msg)) {
        println!("Skipped shell integration.");
        return Ok(Some(rc_path));
    }
    let line = runex_init::integration_line(shell, "runex");
    let block = format!("\n{}\n{}\n", runex_init::RUNEX_INIT_MARKER, line);
    if let Some(parent) = rc_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut open_opts = std::fs::OpenOptions::new();
    open_opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open_opts.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = open_opts.open(&rc_path)?;
    file.write_all(block.as_bytes())?;
    println!("Appended integration to {}", sanitize_for_display(&rc_path.display().to_string()));
    Ok(Some(rc_path))
}

/// Write the clink lua integration to the resolved install path.
///
/// Unlike the rcfile flow, clink's lua file is a *static copy* of the
/// `runex export clink` output. There's no marker block to detect, so
/// we compare full file content against what would be emitted now and
/// only ask before overwriting if the on-disk content has actually
/// drifted. Identical content is a no-op (silent OK).
///
/// `config_path` is consulted so the export reflects the user's
/// keybind / abbr config (clink's lua bakes a `RUNEX_BIN` reference,
/// not abbreviation tables, so the dependency is light — but still
/// correct to thread through).
fn install_clink_lua(yes: bool, config_path: &Path) -> CmdResult {
    use runex_core::integration_check::{check_clink_lua_freshness, IntegrationCheck};

    // Compute the canonical export content for *this* runex binary.
    let bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "runex".to_string());
    let (_path, config, _err) = resolve_config_opt(Some(config_path));
    let new_content = runex_core::shell::export_script(Shell::Clink, &bin, config.as_ref());

    let install_path = runex_init::default_clink_lua_install_path();

    // Decide what to do based on what's already on disk at any of the
    // probe paths. We only write to `install_path`; the freshness check
    // is purely informational ("would this PR-style overwrite be a no-op?").
    let probe = check_clink_lua_freshness(
        &new_content,
        &runex_core::integration_check::default_clink_lua_paths(),
    );
    match probe {
        IntegrationCheck::Ok { detail, .. } => {
            println!("clink integration already up-to-date ({detail}).");
            return Ok(CmdOutcome::Ok);
        }
        IntegrationCheck::Outdated { path, .. } => {
            let msg = format!(
                "clink lua at {} is out of date. Overwrite with the current export?",
                sanitize_for_display(&path.display().to_string())
            );
            if !(yes || prompt_confirm(&msg)) {
                println!("Skipped clink integration update.");
                return Ok(CmdOutcome::Ok);
            }
        }
        IntegrationCheck::Skipped { .. } | IntegrationCheck::Missing { .. } => {
            // No clink lua on disk yet; ask before creating it.
            let msg = format!(
                "Write clink integration to {}?",
                sanitize_for_display(&install_path.display().to_string())
            );
            if !(yes || prompt_confirm(&msg)) {
                println!("Skipped clink integration.");
                return Ok(CmdOutcome::Ok);
            }
        }
    }

    write_clink_lua_safely(&install_path, &new_content)?;
    println!(
        "Wrote clink integration to {}",
        sanitize_for_display(&install_path.display().to_string())
    );
    Ok(CmdOutcome::Ok)
}

/// Write `contents` to `install_path` with two safety properties the
/// previous `std::fs::write` call did not give us:
///
///   1. **Refuse to follow a symlink at `install_path`.** An attacker
///      who can place a symlink in the user's clink scripts directory
///      could otherwise redirect the write to any file the runex
///      process can write (same threat model as the rcfile path). The
///      check uses `symlink_metadata`, which on Windows also catches
///      directory junctions and other reparse points.
///   2. **Atomic replace via sibling temp + rename.** A crash partway
///      through `std::fs::write` would leave a half-written lua file
///      that clink would then load and fail to parse on the next cmd
///      window. Writing to a sibling temp first and renaming on
///      success gives the user either the old content or the new
///      content, never something between.
fn write_clink_lua_safely(install_path: &Path, contents: &str) -> CmdResult {
    use std::io::Write;

    let parent = install_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            Box::<dyn std::error::Error>::from(format!(
                "clink lua install path has no parent directory: {}",
                sanitize_for_display(&install_path.display().to_string())
            ))
        })?;
    std::fs::create_dir_all(parent)?;

    if let Ok(meta) = std::fs::symlink_metadata(install_path) {
        if meta.file_type().is_symlink() {
            return Err(Box::<dyn std::error::Error>::from(format!(
                "refusing to write through a symlink at {}",
                sanitize_for_display(&install_path.display().to_string())
            )));
        }
    }

    let file_name = install_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            Box::<dyn std::error::Error>::from(format!(
                "clink lua install path has no file name: {}",
                sanitize_for_display(&install_path.display().to_string())
            ))
        })?;
    let tmp_path = parent.join(format!(".{file_name}.runex.tmp"));
    // Best-effort cleanup of a stale temp from a previous crash.
    let _ = std::fs::remove_file(&tmp_path);

    let mut tmp_opts = std::fs::OpenOptions::new();
    tmp_opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        tmp_opts.custom_flags(libc::O_NOFOLLOW);
    }
    let mut tmp_file = tmp_opts.open(&tmp_path)?;
    tmp_file.write_all(contents.as_bytes())?;
    tmp_file.sync_all()?;
    drop(tmp_file);

    if let Err(e) = std::fs::rename(&tmp_path, install_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(Box::new(e));
    }
    Ok(CmdOutcome::Ok)
}


/// Per-keystroke hook handler. Writes the shell-specific eval text to stdout
/// on success. On config-load failure we silently emit nothing and return
/// exit code 2; the shell wrapper treats that as "insert a literal space"
/// (so runex never breaks the user's terminal even with a broken config).
/// Maximum byte length of the `--line` value `runex hook` will
/// process. The hook runs on every keystroke, so the worst-case cost
/// of token extraction and is_command_position scanning has to stay
/// bounded. 16 KiB is far above any realistic shell buffer (a long
/// pasted command is typically a few KiB at most) and comfortably
/// below the Windows `CreateProcess` ~32 KiB argv limit, which lets
/// integration tests feed an oversize value through argv without
/// exceeding the OS cap before our own.
///
/// When the cap is exceeded the handler short-circuits to InsertSpace
/// at the cursor and returns. This is the same fall-back the trigger
/// key would have produced anyway, so the user only loses expansion
/// on that single keypress.
const MAX_HOOK_LINE_BYTES: usize = 16 * 1024;

fn handle_hook(
    shell_str: &str,
    line: &str,
    cursor: usize,
    paste_pending: bool,
    config_override: Option<&Path>,
    path_prepend: Option<&Path>,
) -> CmdResult {
    let shell = Shell::from_str(shell_str)
        .map_err(|e| format!("{}", e))?;

    // Per-keystroke cost guard. An oversize --line short-circuits to
    // a literal-space InsertSpace before any expansion logic runs.
    if line.len() > MAX_HOOK_LINE_BYTES {
        let cursor_safe = cursor.min(line.len());
        let mut s = String::with_capacity(line.len() + 1);
        s.push_str(&line[..cursor_safe]);
        s.push(' ');
        s.push_str(&line[cursor_safe..]);
        let action = runex_core::hook::HookAction::InsertSpace {
            line: s,
            cursor: cursor_safe + 1,
        };
        println!("{}", runex_core::hook::render_action(shell, &action));
        return Ok(CmdOutcome::Ok);
    }

    // If the user pasted a block, the pwsh wrapper sets this flag so we skip
    // expansion entirely and behave like a normal space keypress.
    if paste_pending {
        let action = runex_core::hook::HookAction::InsertSpace {
            line: {
                let mut s = String::with_capacity(line.len() + 1);
                let cursor = cursor.min(line.len());
                s.push_str(&line[..cursor]);
                s.push(' ');
                s.push_str(&line[cursor..]);
                s
            },
            cursor: cursor.min(line.len()) + 1,
        };
        println!("{}", runex_core::hook::render_action(shell, &action));
        return Ok(CmdOutcome::Ok);
    }

    // Config load failures are treated as "no expansion" — we still return the
    // InsertSpace action so the wrapper inserts a literal space on behalf of
    // the user.
    let (config_path, config_opt, _err) = resolve_config_opt(config_override);
    let Some(config) = config_opt else {
        // No valid config: emit a plain InsertSpace and return. This avoids
        // making every keypress a no-op (which would swallow the trigger key)
        // when a user has a malformed config they haven't fixed yet.
        let cursor_safe = cursor.min(line.len());
        let mut s = String::with_capacity(line.len() + 1);
        s.push_str(&line[..cursor_safe]);
        s.push(' ');
        s.push_str(&line[cursor_safe..]);
        let action = runex_core::hook::HookAction::InsertSpace { line: s, cursor: cursor_safe + 1 };
        println!("{}", runex_core::hook::render_action(shell, &action));
        return Ok(CmdOutcome::Ok);
    };

    let fp = compute_precache_fingerprint(&config_path, &format!("{shell:?}").to_lowercase());
    let command_exists = make_command_exists(path_prepend, Some(&fp));
    let action = runex_core::hook::hook(&config, shell, line, cursor, command_exists);
    println!("{}", runex_core::hook::render_action(shell, &action));
    Ok(CmdOutcome::Ok)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let outcome: CmdOutcome = match cli.command {
        Commands::Version => handle_version(cli.json)?,
        Commands::List { shell: shell_str } => {
            let (_config_path, config) = resolve_config(cli.config.as_deref())?;
            let shell = resolve_shell(shell_str.as_deref())?;
            handle_list(&config, shell, cli.json)?
        }
        Commands::Which { token, why, shell: shell_str } => {
            let (config_path, config) = resolve_config(cli.config.as_deref())?;
            let shell = resolve_shell(shell_str.as_deref())?.unwrap_or(Shell::Bash);
            let fp = compute_precache_fingerprint(&config_path, &format!("{shell:?}").to_lowercase());
            let command_exists = make_command_exists(cli.path_prepend.as_deref(), Some(&fp));
            handle_which(token, &config, shell, &command_exists, cli.json, why)?
        }
        Commands::Expand { token, dry_run, shell: shell_str } => {
            let (config_path, config) = resolve_config(cli.config.as_deref())?;
            let shell = resolve_shell(shell_str.as_deref())?.unwrap_or(Shell::Bash);
            let fp = compute_precache_fingerprint(&config_path, &format!("{shell:?}").to_lowercase());
            let command_exists = make_command_exists(cli.path_prepend.as_deref(), Some(&fp));
            handle_expand(token, &config, shell, &command_exists, cli.json, dry_run)?
        }
        Commands::Export { shell, bin } => handle_export(shell, bin, cli.config.as_deref())?,
        Commands::Doctor { no_shell_aliases, verbose, strict } => handle_doctor(
            cli.config.as_deref(),
            cli.path_prepend.as_deref(),
            no_shell_aliases,
            verbose,
            strict,
            cli.json,
        )?,
        Commands::Precache { shell, list_commands, resolved } => handle_precache(
            shell,
            list_commands,
            resolved,
            cli.config.as_deref(),
            cli.path_prepend.as_deref(),
        )?,
        Commands::Timings { key, shell: shell_str } => handle_timings(
            key,
            shell_str,
            cli.config.as_deref(),
            cli.path_prepend.as_deref(),
            cli.json,
        )?,
        Commands::Init { shell, yes } => {
            let config_path = if let Some(p) = cli.config.as_deref() {
                p.to_path_buf()
            } else {
                default_config_path()?
            };
            handle_init(config_path, shell.as_deref(), yes)?
        }
        Commands::Hook { shell, line, cursor, paste_pending } => handle_hook(
            &shell,
            &line,
            cursor,
            paste_pending,
            cli.config.as_deref(),
            cli.path_prepend.as_deref(),
        )?,
        Commands::Add { key, expand, when } => {
            let config_path = if let Some(p) = cli.config.as_deref() {
                p.to_path_buf()
            } else {
                default_config_path()?
            };
            runex_core::config::append_abbr_to_file(
                &config_path,
                &key,
                &expand,
                when.as_deref(),
            )?;
            println!("Added: {} -> {}", sanitize_for_display(&key), sanitize_for_display(&expand));
            CmdOutcome::Ok
        }
        Commands::Remove { key } => {
            let config_path = if let Some(p) = cli.config.as_deref() {
                p.to_path_buf()
            } else {
                default_config_path()?
            };
            let removed = runex_core::config::remove_abbr_from_file(&config_path, &key)?;
            if removed > 0 {
                println!("Removed {} rule(s) for '{}'", removed, sanitize_for_display(&key));
            } else {
                println!("No rule found for '{}'", sanitize_for_display(&key));
            }
            CmdOutcome::Ok
        }
    };

    // Single point of `process::exit` for the whole CLI. Handlers
    // never call `std::process::exit` themselves — they return
    // `CmdOutcome::ExitCode(n)` and main translates that here. This
    // is what makes handlers unit-testable: a test harness can call
    // them directly and inspect the returned outcome instead of
    // having the test process silently die.
    match outcome {
        CmdOutcome::Ok => Ok(()),
        CmdOutcome::ExitCode(code) => std::process::exit(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod command_exists {
        use super::*;

    #[test]
    /// `cargo` is guaranteed to be on PATH in a Rust build environment.
    fn make_command_exists_no_prepend_uses_which() {
        let exists = make_command_exists(None, None);
        assert!(exists("cargo"));
        assert!(!exists("__runex_fake_cmd_that_does_not_exist__"));
    }

    #[test]
    fn make_command_exists_prepend_finds_file() {
        let dir = tempfile::tempdir().unwrap();
        let fake_bin = dir.path().join("myfaketool");
        std::fs::write(&fake_bin, b"").unwrap();
        let exists = make_command_exists(Some(dir.path()), None);
        assert!(exists("myfaketool"));
        assert!(!exists("__runex_other_fake__"));
    }

    /// On Windows, child cmd.exe processes spawned by clink's lua `io.popen`
    /// inherit only a subset of the user's PATH (the User-scope PATH from the
    /// registry is sometimes missing). A binary in `~/.cargo/bin` (which is
    /// always in HKCU User PATH for a Rust developer machine, but isn't
    /// in the system PATH) must still be discoverable by `command_exists`.
    ///
    /// This test simulates that environment: it strips PATH down to a minimal
    /// system value and verifies that we can still find a known cargo-installed
    /// binary by consulting the registry's User PATH.
    #[test]
    #[cfg(windows)]
    fn make_command_exists_finds_user_path_binary_when_process_path_is_minimal() {
        // Probe whether cargo is on the registry User PATH; if not, this test
        // can't reliably assert anything (e.g. CI without cargo in user PATH).
        let user_path = read_user_path_for_test();
        if !user_path
            .split(';')
            .any(|p| std::path::Path::new(&p.replace("%UserProfile%", &std::env::var("USERPROFILE").unwrap_or_default()))
                .join("cargo.exe").is_file())
        {
            eprintln!("skipping: cargo.exe not found via registry User PATH");
            return;
        }

        // Strip the process PATH to a minimal Windows system value.
        let original = std::env::var_os("PATH");
        // SAFETY: tests run sequentially within this test module. We do not
        // mutate PATH from any other test, and we restore it before returning.
        unsafe { std::env::set_var("PATH", r"C:\Windows\System32;C:\Windows"); }

        let exists = make_command_exists(None, None);
        let found = exists("cargo");

        // SAFETY: see above.
        unsafe {
            match original {
                Some(v) => std::env::set_var("PATH", v),
                None => std::env::remove_var("PATH"),
            }
        }

        assert!(
            found,
            "make_command_exists must consult HKCU Environment Path on Windows so commands installed under the User PATH (e.g. ~/.cargo/bin) are discoverable even when the process PATH lacks them"
        );
    }

    #[cfg(windows)]
    fn read_user_path_for_test() -> String {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let env = match hkcu.open_subkey("Environment") {
            Ok(k) => k,
            Err(_) => return String::new(),
        };
        env.get_value("Path").unwrap_or_default()
    }

    } // mod command_exists

    /// Regression coverage for the `init` subcommand surface. The earlier
    /// implementation only accepted `-y`; doctor and the docs assumed
    /// `runex init <shell>` worked, leaving users following dead advice.
    mod init_cli {
        use super::*;
        use clap::Parser;

        #[test]
        fn init_without_args_parses() {
            let cli = Cli::try_parse_from(["runex", "init"]).expect("init parses without args");
            match cli.command {
                Commands::Init { shell, yes } => {
                    assert!(shell.is_none(), "no positional → shell must be None");
                    assert!(!yes, "no -y → yes must be false");
                }
                _ => panic!("expected Init"),
            }
        }

        #[test]
        fn init_with_shell_positional_parses() {
            let cli = Cli::try_parse_from(["runex", "init", "bash"]).expect("init bash parses");
            match cli.command {
                Commands::Init { shell, .. } => {
                    assert_eq!(shell.as_deref(), Some("bash"));
                }
                _ => panic!("expected Init"),
            }
        }

        #[test]
        fn init_with_shell_and_yes_parses() {
            let cli = Cli::try_parse_from(["runex", "init", "-y", "clink"])
                .expect("init -y clink parses");
            match cli.command {
                Commands::Init { shell, yes } => {
                    assert_eq!(shell.as_deref(), Some("clink"));
                    assert!(yes);
                }
                _ => panic!("expected Init"),
            }
        }
    }

    /// `init` reads the rc file to check for RUNEX_INIT_MARKER before appending.
    /// If the rc file is extremely large (e.g. corrupted or adversarially crafted),
    /// `read_to_string` would consume unbounded memory. `read_rc_content` must
    /// refuse files larger than MAX_RC_FILE_BYTES and return an empty string so
    /// that the marker check fails safe (appends as if unseen — idempotent).
    mod rc_file_size_limit {
        use super::*;

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

    } // mod rc_file_size_limit

    /// `prompt_confirm` reads one line from stdin to get a y/N answer.
    /// Without a size limit, a caller piping 10 MB of data would cause
    /// `read_line()` to allocate a 10 MB String before returning, wasting memory.
    /// The internal `prompt_confirm_from` helper must cap reading at
    /// MAX_CONFIRM_BYTES so that oversized input is treated as "no" without
    /// accumulating it all.
    mod prompt_confirm_limit {
        use super::*;

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

    /// A line far exceeding MAX_CONFIRM_BYTES must be treated as "no",
    /// not buffered in full. The function must return false without OOM.
    #[test]
    fn prompt_confirm_from_rejects_oversized_input() {
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

    } // mod prompt_confirm_limit

    /// `read_rc_content` reads the shell rc file to detect the RUNEX_INIT_MARKER.
    /// It must reject non-regular files (named pipes, device files) to prevent:
    /// - Named pipe (FIFO): `metadata().len() == 0`, `read_to_string()` blocks
    ///   indefinitely waiting for a writer — process hangs.
    /// - Device files (`/dev/zero`, `/dev/urandom`): report len=0, `read_to_string()`
    ///   fills memory unboundedly.
    /// The function must check `metadata().is_file()` before attempting to read.
    #[cfg(unix)]
    mod rc_file_non_regular {
        use super::*;

    #[test]
    fn read_rc_content_rejects_named_pipe() {
        use std::ffi::CString;
        let dir = tempfile::tempdir().unwrap();
        let pipe = dir.path().join("fake_rc.sh");
        let path_c = CString::new(pipe.to_str().unwrap()).unwrap();
        unsafe { libc::mkfifo(path_c.as_ptr(), 0o600) };
        let content = read_rc_content(&pipe);
        assert_eq!(
            content, "",
            "read_rc_content must return empty string for a named pipe (FIFO), not block"
        );
    }

    #[test]
    #[cfg(unix)]
    fn read_rc_content_rejects_dev_zero() {
        let path = std::path::Path::new("/dev/zero");
        let content = read_rc_content(path);
        assert_eq!(
            content, "",
            "read_rc_content must return empty string for /dev/zero (device file)"
        );
    }

    } // mod rc_file_non_regular

    /// Handlers that previously called `std::process::exit(1)`
    /// directly are now testable from inside the process. These
    /// tests confirm the new contract: a validation failure
    /// returns `Ok(CmdOutcome::ExitCode(1))` instead of bringing
    /// the test process down with it.
    ///
    /// The handlers exercised here are the ones that owned the
    /// pre-B1 exits: handle_which / handle_expand / handle_timings
    /// for over-long tokens, validate_bin (via handle_export) for
    /// invalid bin strings.
    mod handler_outcomes {
        use super::*;
        use runex_core::model::Config;

        fn over_long_token() -> String {
            // MAX_TOKEN_BYTES is 1_024; 1025 trips the guard with
            // exactly one byte of slack.
            "a".repeat(MAX_TOKEN_BYTES + 1)
        }

        fn never_exists(_: &str) -> bool {
            false
        }

        #[test]
        fn handle_which_over_long_token_returns_exit_code_1() {
            let cfg = Config {
                version: 1,
                keybind: Default::default(),
                precache: Default::default(),
                abbr: Vec::new(),
            };
            let outcome = handle_which(
                over_long_token(),
                &cfg,
                Shell::Bash,
                &never_exists,
                false,
                false,
            )
            .expect("handle_which must return Ok, not Err, for an over-long token");
            assert_eq!(outcome, CmdOutcome::ExitCode(1));
        }

        #[test]
        fn handle_expand_over_long_token_returns_exit_code_1() {
            let cfg = Config {
                version: 1,
                keybind: Default::default(),
                precache: Default::default(),
                abbr: Vec::new(),
            };
            let outcome = handle_expand(
                over_long_token(),
                &cfg,
                Shell::Bash,
                &never_exists,
                false,
                false,
            )
            .expect("handle_expand must return Ok, not Err, for an over-long token");
            assert_eq!(outcome, CmdOutcome::ExitCode(1));
        }

        #[test]
        fn validate_bin_rejects_empty() {
            assert!(validate_bin("").is_err());
            assert!(validate_bin("   ").is_err());
        }

        #[test]
        fn validate_bin_rejects_oversize() {
            let huge = "a".repeat(MAX_BIN_LEN + 1);
            assert!(validate_bin(&huge).is_err());
        }

        #[test]
        fn validate_bin_rejects_control_characters() {
            assert!(validate_bin("ru\nnex").is_err()); // literal newline
            assert!(validate_bin("ru\x07nex").is_err()); // BEL
        }

        #[test]
        fn validate_bin_rejects_non_ascii() {
            assert!(validate_bin("rünex").is_err());
        }

        #[test]
        fn validate_bin_accepts_normal_name() {
            assert!(validate_bin("runex").is_ok());
            assert!(validate_bin("/usr/local/bin/runex").is_ok());
        }
    }

}
