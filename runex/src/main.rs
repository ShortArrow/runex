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

type CmdResult = Result<(), Box<dyn std::error::Error>>;

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
    Ok(())
}

fn handle_list(config: &Config, shell: Option<Shell>, json: bool) -> CmdResult {
    if json {
        println!("{}", serde_json::to_string_pretty(&config.abbr)?);
    } else {
        for (key, exp) in expand::list(config, shell) {
            println!("{}\t{}", sanitize_for_display(key), sanitize_for_display(&exp));
        }
    }
    Ok(())
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
        std::process::exit(1);
    }
    let result = expand::which_abbr(config, &token, shell, command_exists);
    if json {
        println!("{}", serde_json::to_string_pretty(&which_result_to_json(&result))?);
    } else {
        println!("{}", format_which_result(&result, why));
    }
    Ok(())
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
        std::process::exit(1);
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
    Ok(())
}

/// Validate and parse the `--bin` argument for `export`.
///
/// Rejects values that are empty, whitespace-only, too long, contain control
/// characters, or contain non-printable-ASCII characters.  Only printable ASCII
/// is allowed to prevent Unicode homoglyphs and bidirectional overrides from
/// being silently embedded in generated shell scripts.
fn validate_bin(bin: &str) {
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
    if bin.chars().any(|c| !c.is_ascii() || !c.is_ascii_graphic()) {
        eprintln!("error: --bin must contain only printable ASCII characters");
        std::process::exit(1);
    }
}

fn handle_export(
    shell: String,
    bin: String,
    config_flag: Option<&Path>,
) -> CmdResult {
    validate_bin(&bin);
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
    Ok(())
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
        return Ok(());
    }

    let fp = compute_precache_fingerprint(&config_path, &shell_name);

    // Mode 2: use externally resolved results instead of which::which()
    if let Some(resolved_str) = resolved {
        let cache = precache::build_cache_from_resolved(&config, &fp, &resolved_str);
        let json = precache::cache_to_json(&cache);
        println!("{}", precache::export_statement(&shell_name, &json));
        return Ok(());
    }

    // Default: use which::which() for command existence checks
    let command_exists = make_command_exists(path_prepend, None);
    let cache = precache::build_cache(&config, &fp, &command_exists);
    let json = precache::cache_to_json(&cache);

    println!("{}", precache::export_statement(&shell_name, &json));
    Ok(())
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
                std::process::exit(1);
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
    Ok(())
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
        std::process::exit(1);
    }
    Ok(())
}

fn handle_init(config_path: PathBuf, yes: bool) -> CmdResult {
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
                let msg = format!(
                    "Append shell integration to {}?",
                    sanitize_for_display(&rc_path.display().to_string())
                );
                if yes || prompt_confirm(&msg) {
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
                } else {
                    println!("Skipped shell integration.");
                }
            }
        }
    }
    Ok(())
}


/// Per-keystroke hook handler. Writes the shell-specific eval text to stdout
/// on success. On config-load failure we silently emit nothing and return
/// exit code 2; the shell wrapper treats that as "insert a literal space"
/// (so runex never breaks the user's terminal even with a broken config).
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
        return Ok(());
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
        return Ok(());
    };

    let fp = compute_precache_fingerprint(&config_path, &format!("{shell:?}").to_lowercase());
    let command_exists = make_command_exists(path_prepend, Some(&fp));
    let action = runex_core::hook::hook(&config, shell, line, cursor, command_exists);
    println!("{}", runex_core::hook::render_action(shell, &action));
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Version => handle_version(cli.json)?,
        Commands::List { shell: shell_str } => {
            let (_config_path, config) = resolve_config(cli.config.as_deref())?;
            let shell = resolve_shell(shell_str.as_deref())?;
            handle_list(&config, shell, cli.json)?;
        }
        Commands::Which { token, why, shell: shell_str } => {
            let (config_path, config) = resolve_config(cli.config.as_deref())?;
            let shell = resolve_shell(shell_str.as_deref())?.unwrap_or(Shell::Bash);
            let fp = compute_precache_fingerprint(&config_path, &format!("{shell:?}").to_lowercase());
            let command_exists = make_command_exists(cli.path_prepend.as_deref(), Some(&fp));
            handle_which(token, &config, shell, &command_exists, cli.json, why)?;
        }
        Commands::Expand { token, dry_run, shell: shell_str } => {
            let (config_path, config) = resolve_config(cli.config.as_deref())?;
            let shell = resolve_shell(shell_str.as_deref())?.unwrap_or(Shell::Bash);
            let fp = compute_precache_fingerprint(&config_path, &format!("{shell:?}").to_lowercase());
            let command_exists = make_command_exists(cli.path_prepend.as_deref(), Some(&fp));
            handle_expand(token, &config, shell, &command_exists, cli.json, dry_run)?;
        }
        Commands::Export { shell, bin } => {
            handle_export(shell, bin, cli.config.as_deref())?;
        }
        Commands::Doctor { no_shell_aliases, verbose, strict } => {
            handle_doctor(
                cli.config.as_deref(),
                cli.path_prepend.as_deref(),
                no_shell_aliases,
                verbose,
                strict,
                cli.json,
            )?;
        }
        Commands::Precache { shell, list_commands, resolved } => {
            handle_precache(shell, list_commands, resolved, cli.config.as_deref(), cli.path_prepend.as_deref())?;
        }
        Commands::Timings { key, shell: shell_str } => {
            handle_timings(
                key,
                shell_str,
                cli.config.as_deref(),
                cli.path_prepend.as_deref(),
                cli.json,
            )?;
        }
        Commands::Init { yes } => {
            let config_path = if let Some(p) = cli.config.as_deref() {
                p.to_path_buf()
            } else {
                default_config_path()?
            };
            handle_init(config_path, yes)?;
        }
        Commands::Hook { shell, line, cursor, paste_pending } => {
            handle_hook(
                &shell,
                &line,
                cursor,
                paste_pending,
                cli.config.as_deref(),
                cli.path_prepend.as_deref(),
            )?;
        }
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
        }
    }

    Ok(())
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

}
