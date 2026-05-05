//! `runex` — cross-shell abbreviation expansion CLI.
//!
//! ## Layering
//!
//! ```text
//!   cmd  → app  → domain
//!     ↓     ↓
//!   util   infra → domain
//! ```
//!
//! - `domain/`  — pure data types and rule-evaluation logic. No I/O,
//!   no env, no time. Imports from sibling layers are forbidden.
//! - `app/`     — orchestration / use-case wrappers. Composes
//!   `domain` types with `infra` adapters to answer "what should
//!   `runex doctor` actually check?", "what should `expand` return
//!   for this token?". No `std::fs::*` calls.
//! - `infra/`   — file system, environment, registry adapters.
//!   Implements injection traits (`HomeDirResolver`) for `app/`.
//!   `infra → domain` only.
//! - `cmd/`     — per-subcommand handlers. Reach behaviour through
//!   `app/`; reach leaf utilities through `util/`. The architecture
//!   test forbids `cmd → domain::{expand, hook}` and
//!   `cmd → domain::shell::export_script`.
//! - `util/`    — leaf helpers shared by `cmd/*` (shell detection,
//!   prompt confirmation, command-existence factory). No
//!   command-specific policy.
//! - `format` / `shell_alias` / `win_path` — single-purpose modules
//!   pre-dating the layering split; safe in their current location.
//!
//! Cycles are prevented at compile-time by
//! `runex/tests/architecture.rs` (`no_infra_to_app_imports`,
//! `no_domain_to_anyone_else_imports`,
//! `no_cmd_to_domain_behavior_imports`,
//! `no_filesystem_calls_in_app_layer`).

mod app;
mod cmd;
mod domain;
mod format;
mod infra;
mod shell_alias;
mod util;
#[cfg(windows)]
mod win_path;

use clap::{Parser, Subcommand};
use crate::app::config::{default_config_path, load_config};
use crate::domain::model::Config;
use crate::domain::sanitize::sanitize_for_display;
use crate::domain::shell::Shell;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
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
pub(crate) const MAX_BIN_LEN: usize = 255;

pub(crate) struct Spinner {
    done: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Spinner {
    pub(crate) fn start(message: &'static str) -> Self {
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

    pub(crate) fn stop(mut self) {
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
pub(crate) fn resolve_config(
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
pub(crate) fn resolve_config_opt(config_override: Option<&Path>) -> (PathBuf, Option<Config>, Option<String>) {
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
pub(crate) fn compute_precache_fingerprint(config_path: &Path, shell: &str) -> String {
    let path_env = std::env::var("PATH").unwrap_or_default();
    let mtime = crate::app::precache::config_mtime(config_path);
    crate::app::precache::compute_fingerprint(&path_env, mtime, shell)
}

/// Per-invocation runtime context shared by every command handler
/// that needs config + resolved shell + a `command_exists` probe.
///
/// Pre-B2 the `(config_path, config) = resolve_config(...);
/// shell = resolve_shell(...).unwrap_or(Shell::Bash); fp =
/// compute_precache_fingerprint(...); command_exists =
/// make_command_exists(path_prepend, Some(&fp));` four-line dance
/// was open-coded in `which`, `expand`, `timings`, `precache`,
/// `hook`, and `doctor` (5+ sites, all subtly different in fp /
/// command_exists tuning). Putting it behind one constructor
/// removes the "five places to change" tax on any future runtime
/// change and turns the precache-fingerprint policy (`Some(fp)` vs
/// `None`) into a single decision per command rather than five
/// scattered ones.
///
/// `command_exists` is `Box<dyn Fn>` so the context is movable;
/// the underlying closure owns its `path_prepend` so the context
/// has no lifetime parameter.
pub(crate) struct AppContext {
    /// Path actually loaded — kept on the context for diagnostics
    /// (and for handlers that may want to surface it). Currently
    /// only constructed; future code may surface it on errors.
    #[allow(dead_code)]
    pub(crate) config_path: PathBuf,
    pub(crate) config: Config,
    /// `None` only when `--shell` was omitted *and* the command is
    /// shell-agnostic (e.g. `list` shows all shells). Most handlers
    /// fall back to `Shell::Bash` themselves; `AppContext` keeps the
    /// raw `Option` so each handler can decide.
    pub(crate) shell: Option<Shell>,
    /// Stable digest of `(config_path, shell)`. Threaded through to
    /// the precache layer for cache-key isolation; not read after
    /// construction in this file but kept on the struct for
    /// completeness of the context shape.
    #[allow(dead_code)]
    pub(crate) fingerprint: String,
    pub(crate) command_exists: Box<dyn Fn(&str) -> bool>,
}

impl AppContext {
    /// Build a context for commands that *require* a loadable config
    /// (`which`, `expand`, `timings`, `precache`). Fails fast if the
    /// config is missing or unparseable.
    ///
    /// `precache_enabled` controls whether the `command_exists`
    /// closure consults the on-disk precache hint. Most commands
    /// pass `true`; the historical `make_command_exists(.., None)`
    /// callers (precache itself, doctor) pass `false`.
    pub(crate) fn build(
        config_flag: Option<&Path>,
        shell_flag: Option<&str>,
        path_prepend: Option<&Path>,
        precache_enabled: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (config_path, config) = resolve_config(config_flag)?;
        Ok(Self::assemble(config_path, config, shell_flag, path_prepend, precache_enabled)?)
    }

    /// Graceful variant: missing/unparseable config is *not* a hard
    /// error; callers (`hook`, `doctor`) want to keep running and
    /// surface the error themselves rather than abort. Returns the
    /// parse-error message in `parse_error` when applicable.
    ///
    /// `OptionalContext` is the same shape as `AppContext` but with
    /// `Option<Config>` so the caller can branch on absence.
    pub(crate) fn build_optional(
        config_flag: Option<&Path>,
        shell_flag: Option<&str>,
        path_prepend: Option<&Path>,
        precache_enabled: bool,
    ) -> OptionalContext {
        let (config_path, config_opt, parse_error) = resolve_config_opt(config_flag);
        let shell = resolve_shell(shell_flag).ok().flatten();
        let resolved_shell = shell.unwrap_or(Shell::Bash);
        let fingerprint = compute_precache_fingerprint(
            &config_path,
            &format!("{resolved_shell:?}").to_lowercase(),
        );
        let path_prepend_owned = path_prepend.map(|p| p.to_path_buf());
        let command_exists: Box<dyn Fn(&str) -> bool> = if precache_enabled {
            Box::new(make_command_exists_owned(path_prepend_owned, Some(fingerprint.clone())))
        } else {
            Box::new(make_command_exists_owned(path_prepend_owned, None))
        };
        OptionalContext {
            config_path,
            config: config_opt,
            parse_error,
            shell,
            fingerprint,
            command_exists,
        }
    }

    fn assemble(
        config_path: PathBuf,
        config: Config,
        shell_flag: Option<&str>,
        path_prepend: Option<&Path>,
        precache_enabled: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let shell = resolve_shell(shell_flag)?;
        let resolved_shell = shell.unwrap_or(Shell::Bash);
        let fingerprint = compute_precache_fingerprint(
            &config_path,
            &format!("{resolved_shell:?}").to_lowercase(),
        );
        let path_prepend_owned = path_prepend.map(|p| p.to_path_buf());
        let command_exists: Box<dyn Fn(&str) -> bool> = if precache_enabled {
            Box::new(make_command_exists_owned(path_prepend_owned, Some(fingerprint.clone())))
        } else {
            Box::new(make_command_exists_owned(path_prepend_owned, None))
        };
        Ok(Self {
            config_path,
            config,
            shell,
            fingerprint,
            command_exists,
        })
    }
}

/// Same fields as [`AppContext`] but `config` is optional and a
/// `parse_error` is carried forward — used by commands that must
/// survive a missing or broken config (`hook`, `doctor`).
pub(crate) struct OptionalContext {
    #[allow(dead_code)]
    pub(crate) config_path: PathBuf,
    pub(crate) config: Option<Config>,
    #[allow(dead_code)]
    pub(crate) parse_error: Option<String>,
    #[allow(dead_code)]
    pub(crate) shell: Option<Shell>,
    #[allow(dead_code)]
    pub(crate) fingerprint: String,
    pub(crate) command_exists: Box<dyn Fn(&str) -> bool>,
}

/// Items at crate root use the util fns directly. cmd/* and util/*
/// reach the rest of util via fully-qualified paths. After B5 the
/// only crate-root caller of these is `AppContext` (for
/// `make_command_exists_owned`) and the `Commands::List` dispatch
/// arm (for `resolve_shell`).
use util::path::make_command_exists_owned;
use util::shell::resolve_shell;

#[cfg(test)]
use util::prompt::{prompt_confirm_from, MAX_CONFIRM_BYTES, MAX_RC_FILE_BYTES};


/// Maximum byte length accepted for `--token` (expand) and `which <token>`.
/// Tokens longer than any possible abbr key (MAX_KEY_BYTES = 1024 in config.rs)
/// can never match and would cause needless memory allocation in sanitize_for_display.
pub(crate) const MAX_TOKEN_BYTES: usize = 1_024;

/// Outcome of a CLI subcommand handler. The handler reports either
/// success (the process should exit 0) or a request to exit with a
/// non-zero code. `main()` is the *only* function that calls
/// `process::exit` — handlers return outcomes so they can be
/// exercised from unit tests without taking the test process down
/// with them. Unrecoverable errors still bubble up via the `Err`
/// variant of `CmdResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CmdOutcome {
    /// Handler succeeded; main exits with 0.
    Ok,
    /// Handler requests a specific exit code (typically 1 for
    /// validation failures or doctor-found-errors). Used in place
    /// of `std::process::exit(n)` inside handlers.
    ExitCode(i32),
}

pub(crate) type CmdResult = Result<CmdOutcome, Box<dyn std::error::Error>>;

// Per-subcommand handlers live in `cmd::*`. main()'s dispatch just
// forwards there.



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
pub(crate) const MAX_HOOK_LINE_BYTES: usize = 16 * 1024;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let outcome: CmdOutcome = match cli.command {
        Commands::Version => cmd::version::handle(cli.json)?,
        Commands::List { shell: shell_str } => {
            let (_config_path, config) = resolve_config(cli.config.as_deref())?;
            let shell = resolve_shell(shell_str.as_deref())?;
            cmd::list::handle(&config, shell, cli.json)?
        }
        Commands::Which { token, why, shell: shell_str } => {
            let ctx = AppContext::build(
                cli.config.as_deref(),
                shell_str.as_deref(),
                cli.path_prepend.as_deref(),
                true,
            )?;
            cmd::which::handle(
                token,
                &ctx.config,
                ctx.shell.unwrap_or(Shell::Bash),
                &*ctx.command_exists,
                cli.json,
                why,
            )?
        }
        Commands::Expand { token, dry_run, shell: shell_str } => {
            let ctx = AppContext::build(
                cli.config.as_deref(),
                shell_str.as_deref(),
                cli.path_prepend.as_deref(),
                true,
            )?;
            cmd::expand::handle(
                token,
                &ctx.config,
                ctx.shell.unwrap_or(Shell::Bash),
                &*ctx.command_exists,
                cli.json,
                dry_run,
            )?
        }
        Commands::Export { shell, bin } => cmd::export::handle(shell, bin, cli.config.as_deref())?,
        Commands::Doctor { no_shell_aliases, verbose, strict } => cmd::doctor::handle(
            cli.config.as_deref(),
            cli.path_prepend.as_deref(),
            no_shell_aliases,
            verbose,
            strict,
            cli.json,
        )?,
        Commands::Precache { shell, list_commands, resolved } => cmd::precache::handle(
            shell,
            list_commands,
            resolved,
            cli.config.as_deref(),
            cli.path_prepend.as_deref(),
        )?,
        Commands::Timings { key, shell: shell_str } => cmd::timings::handle(
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
            cmd::init::handle(
                config_path,
                shell.as_deref(),
                yes,
                &infra::env::SystemHomeDir,
            )?
        }
        Commands::Hook { shell, line, cursor, paste_pending } => cmd::hook::handle(
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
            cmd::add_remove::handle_add(&config_path, &key, &expand, when.as_deref())?
        }
        Commands::Remove { key } => {
            let config_path = if let Some(p) = cli.config.as_deref() {
                p.to_path_buf()
            } else {
                default_config_path()?
            };
            cmd::add_remove::handle_remove(&config_path, &key)?
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

    #[test]
    /// `cargo` is guaranteed to be on PATH in a Rust build environment.
    fn make_command_exists_no_prepend_uses_which() {
        let exists = crate::util::path::make_command_exists(None, None);
        assert!(exists("cargo"));
        assert!(!exists("__runex_fake_cmd_that_does_not_exist__"));
    }

    #[test]
    fn make_command_exists_prepend_finds_file() {
        let dir = tempfile::tempdir().unwrap();
        let fake_bin = dir.path().join("myfaketool");
        std::fs::write(&fake_bin, b"").unwrap();
        let exists = crate::util::path::make_command_exists(Some(dir.path()), None);
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
    #[serial_test::serial(env_path)]
    fn make_command_exists_finds_user_path_binary_when_process_path_is_minimal() {
        // `serial(env_path)` shares an exclusion group with the
        // AppContext fingerprint stability test: both mutate / read
        // $PATH and would otherwise produce flakes when run in
        // parallel on Windows. The comment about "tests run
        // sequentially within this test module" below predates B2 —
        // that's no longer true crate-wide, hence the explicit
        // `serial` attribute.

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

        let exists = crate::util::path::make_command_exists(None, None);
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
        let content = crate::util::prompt::read_rc_content(f.path());
        assert!(content.contains("# runex-init"), "normal rc file must be readable");
    }

    #[test]
    fn read_rc_content_returns_empty_for_oversized_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        f.write_all(&vec![b'x'; MAX_RC_FILE_BYTES + 1]).unwrap();
        let content = crate::util::prompt::read_rc_content(f.path());
        assert!(
            content.is_empty(),
            "read_rc_content must return empty string for oversized rc file"
        );
    }

    #[test]
    fn read_rc_content_returns_empty_for_missing_file() {
        let content = crate::util::prompt::read_rc_content(std::path::Path::new("/nonexistent/runex_test.rc"));
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
        let content = crate::util::prompt::read_rc_content(f.path());
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
        let content = crate::util::prompt::read_rc_content(&pipe);
        assert_eq!(
            content, "",
            "read_rc_content must return empty string for a named pipe (FIFO), not block"
        );
    }

    #[test]
    #[cfg(unix)]
    fn read_rc_content_rejects_dev_zero() {
        let path = std::path::Path::new("/dev/zero");
        let content = crate::util::prompt::read_rc_content(path);
        assert_eq!(
            content, "",
            "read_rc_content must return empty string for /dev/zero (device file)"
        );
    }

    } // mod rc_file_non_regular

    /// `AppContext` collapses the `resolve_config + resolve_shell +
    /// compute_precache_fingerprint + make_command_exists` four-line
    /// dance that used to live in five handlers. These tests pin the
    /// builder's contract: same inputs produce same fingerprints
    /// (cache stability), graceful builder survives missing config.
    mod app_context {
        use super::*;
        use std::io::Write;

        fn write_minimal_config(toml: &str) -> tempfile::NamedTempFile {
            let mut f = tempfile::NamedTempFile::new().unwrap();
            f.write_all(toml.as_bytes()).unwrap();
            f.flush().unwrap();
            f
        }

        #[test]
        #[cfg_attr(windows, serial_test::serial(env_path))]
        fn build_returns_same_fingerprint_for_identical_args() {
            // The fingerprint depends on $PATH (via
            // `compute_precache_fingerprint`). The Windows-only
            // `make_command_exists_finds_user_path_binary_…` test
            // also mutates $PATH; without the `serial` attribute on
            // both, the two can interleave on a multi-threaded test
            // runner and the fingerprints diverge. Linux has no
            // equivalent mutator so the gating is Windows-only.
            let cfg = write_minimal_config("version = 1\n");
            let a = AppContext::build(Some(cfg.path()), Some("bash"), None, true)
                .expect("build must succeed for a valid config");
            let b = AppContext::build(Some(cfg.path()), Some("bash"), None, true)
                .expect("build must succeed twice");
            assert_eq!(
                a.fingerprint, b.fingerprint,
                "two builds with the same inputs must produce the same fingerprint, \
                 otherwise on-disk precache hits would alternate between calls"
            );
        }

        #[test]
        fn build_optional_returns_none_config_when_path_missing() {
            let nonexistent = std::path::Path::new("/nonexistent/runex/config.toml");
            let ctx = AppContext::build_optional(Some(nonexistent), Some("bash"), None, true);
            assert!(
                ctx.config.is_none(),
                "build_optional must return None config (not an Err) when the file is missing — \
                 hook depends on this so it can fall back to InsertSpace"
            );
        }

        #[test]
        fn build_fails_when_config_path_missing() {
            let nonexistent = std::path::Path::new("/nonexistent/runex/config.toml");
            let result = AppContext::build(Some(nonexistent), Some("bash"), None, true);
            assert!(
                result.is_err(),
                "build (non-graceful) must Err on missing config — \
                 which/expand depend on this to surface the error"
            );
        }
    }

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
        use crate::domain::model::Config;

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
            let outcome = cmd::which::handle(
                over_long_token(),
                &cfg,
                Shell::Bash,
                &never_exists,
                false,
                false,
            )
            .expect("cmd::which::handle must return Ok, not Err, for an over-long token");
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
            let outcome = cmd::expand::handle(
                over_long_token(),
                &cfg,
                Shell::Bash,
                &never_exists,
                false,
                false,
            )
            .expect("cmd::expand::handle must return Ok, not Err, for an over-long token");
            assert_eq!(outcome, CmdOutcome::ExitCode(1));
        }

        #[test]
        fn validate_bin_rejects_empty() {
            assert!(cmd::export::validate_bin("").is_err());
            assert!(cmd::export::validate_bin("   ").is_err());
        }

        #[test]
        fn validate_bin_rejects_oversize() {
            let huge = "a".repeat(MAX_BIN_LEN + 1);
            assert!(cmd::export::validate_bin(&huge).is_err());
        }

        #[test]
        fn validate_bin_rejects_control_characters() {
            assert!(cmd::export::validate_bin("ru\nnex").is_err()); // literal newline
            assert!(cmd::export::validate_bin("ru\x07nex").is_err()); // BEL
        }

        #[test]
        fn validate_bin_rejects_non_ascii() {
            assert!(cmd::export::validate_bin("rünex").is_err());
        }

        #[test]
        fn validate_bin_accepts_normal_name() {
            assert!(cmd::export::validate_bin("runex").is_ok());
            assert!(cmd::export::validate_bin("/usr/local/bin/runex").is_ok());
        }
    }

}
