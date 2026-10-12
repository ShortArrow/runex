//! expectrl-backed PTY session wrapper, shared by the bash / zsh /
//! pwsh PTY integration tests.
//!
//! The interesting part of these tests is asserting that *real
//! keystrokes through readline / zle / PSReadLine* drive the runex
//! integration end-to-end. The mechanics of "spawn a shell with a
//! sentinel prompt, source the integration script, wait for the
//! prompt to settle" are identical across shells; only the
//! per-shell launch flags and prompt-setup syntax differ. This
//! module factors out the identical part.
//!
//! Unix only — expectrl 0.7's Windows ConPTY backend is unstable
//! (per the dev-dep declaration in `runex/Cargo.toml`).

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use expectrl::session::Session;

/// The sentinel prompt every PTY session installs. Chosen to be
/// unmistakably ours so an `expect(Regex(SENTINEL))` cannot match
/// anything that scrolls in from the shell's own MOTD or readline
/// banners.
pub const SENTINEL_PROMPT: &str = "__RUNEX_PROMPT__> ";

/// Default wait deadline. CI runners with slow IO need generous
/// headroom; production keystroke latency is microseconds, so 5
/// seconds is "definitely broken if we hit it" rather than "might be
/// slow".
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// A Device Status Report (cursor position) query. reedline (nu) and
/// PSReadLine (pwsh) emit this on startup and after each render and
/// then **block until the terminal answers**. A real terminal replies
/// automatically; our PTY does not, so the harness must, or the
/// shell's line editor never reaches the point where it reads
/// keystrokes and every wait below times out. bash and zsh's zle do
/// not query, so answering is a harmless no-op for them.
const DSR_QUERY: &[u8] = b"\x1b[6n";

/// A plausible cursor-position response (row 1, col 1). The value does
/// not matter — the line editor only needs *an* answer to unblock.
const DSR_ANSWER: &[u8] = b"\x1b[1;1R";

/// Carriage return. PSReadLine and reedline treat `\r` (Enter), not
/// `\n`, as "accept the line"; a bare `\n` is read as a literal
/// newline inside the buffer and the command never runs. expectrl's
/// `send_line` sends `\n` on Unix, so the harness sends CR itself.
const ENTER: &[u8] = b"\r";

/// Which shell to launch under the PTY. Each variant carries the
/// per-shell launch flags / prompt-setup syntax in `bootstrap`
/// below.
#[derive(Debug, Clone, Copy)]
pub enum PtyShell {
    Bash,
    Zsh,
    Pwsh,
    /// nushell. Sources runex.nu via `source` after writing the
    /// generated script to a tempfile (nu resolves `source` paths at
    /// parse time so we cannot generate-and-source in one step).
    Nu,
}

/// A live PTY session with the runex integration sourced and the
/// sentinel prompt installed. Drop drains the child the way
/// `Session` does on its own, so explicit `quit()` is optional.
pub struct PtySession {
    inner: Session,
    /// Everything read from the child so far. `read_until` appends to
    /// it and scans it, so a match survives output that arrived before
    /// the corresponding `expect` call.
    seen: String,
    /// Monotonic counter for the per-line sync token in
    /// [`Self::send_line_synced`].
    sync_counter: u64,
    /// How long each bootstrap step may take: the shell starting up, or
    /// [`Self::send_line_synced`] waiting for a line to finish.
    step_timeout: Duration,
    /// The last `DSR_QUERY.len() - 1` bytes read, carried across
    /// `try_read` calls so a query split over two reads is still
    /// recognised and answered.
    dsr_carry: Vec<u8>,
    /// When the child last printed anything; `None` until it has.
    last_output: Option<Instant>,
    /// When the shell was launched, so a failure can say how long the
    /// whole bootstrap had taken.
    started: Instant,
    /// Everything read since launch. Unlike `seen` it is never
    /// cleared, so a bootstrap failure can show the startup output too.
    transcript: String,
}

/// How much of the transcript a [`BootstrapError`] keeps, counted
/// from the end: enough for a banner and a few prompts.
const TRANSCRIPT_TAIL: usize = 4096;

/// What stopped a step of the bootstrap.
#[derive(Debug)]
pub enum Cause {
    Launch(String),
    Write(String),
    Read(String),
    /// The step's output did not arrive within this deadline.
    Timeout(Duration),
    /// `runex export nu` could not be run or written to a file.
    Export(String),
}

/// Why [`PtySession::spawn`] could not hand back a ready session. Its
/// `Debug` form is what a failed `.expect(..)` prints, so it carries
/// what is needed to tell a slow shell from a broken one.
pub struct BootstrapError {
    /// `launch`, `startup`, `runex export nu`, or the line the harness
    /// had sent to the shell.
    pub step: String,
    pub cause: Cause,
    /// Time from launch to the failure.
    pub elapsed: Duration,
    /// The last [`TRANSCRIPT_TAIL`] bytes the shell printed since launch.
    pub output: String,
}

impl std::fmt::Debug for BootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "step {:?} failed {:.1?} after launch: {:?}", self.step, self.elapsed, self.cause)?;
        write!(f, "output since launch: {:?}", self.output)
    }
}

impl PtySession {
    /// Spawn `shell` under a PTY, set `RUNEX_CONFIG=<config>`,
    /// install the [`SENTINEL_PROMPT`], source the runex integration
    /// produced by `runex export <shell>`, and block until the post-
    /// source prompt has settled.
    ///
    /// Returns a [`BootstrapError`] if the shell can't be launched or
    /// any of the setup steps don't complete within [`DEFAULT_TIMEOUT`].
    /// Tests check that the shell is installed *before* calling this,
    /// so they treat the error as a failure (`.expect(..)`), never as a
    /// skip: a harness that quietly stops bootstrapping must turn the
    /// suite red, not green.
    pub fn spawn(shell: PtyShell, runex_bin: &str, config: &Path) -> Result<Self, BootstrapError> {
        let mut this = Self::launch(&launch_command(shell, runex_bin))?;
        this.settle_startup(shell)?;
        bootstrap(&mut this, shell, runex_bin, config)?;
        // Drop the bootstrap chatter so a test's first `expect` matches
        // only what its own keystrokes produce.
        this.seen.clear();
        Ok(this)
    }

    /// Run `command` under a PTY. The window is made wide and tall so a
    /// wrapped line doesn't derail the editor's cursor tracking during
    /// a keystroke test; the PTY otherwise defaults to a small size.
    fn launch(command: &str) -> Result<Self, BootstrapError> {
        let started = Instant::now();
        let mut session = expectrl::spawn(command).map_err(|e| BootstrapError {
            step: "launch".to_string(),
            cause: Cause::Launch(e.to_string()),
            elapsed: started.elapsed(),
            output: String::new(),
        })?;
        let _ = session.get_process_mut().set_window_size(240, 60);
        Ok(Self {
            inner: session,
            seen: String::new(),
            sync_counter: 0,
            step_timeout: DEFAULT_TIMEOUT,
            dsr_carry: Vec::new(),
            last_output: None,
            started,
            transcript: String::new(),
        })
    }

    /// A [`BootstrapError`] for `step`, with the tail of the transcript.
    fn failure(&self, step: &str, cause: Cause) -> BootstrapError {
        let mut from = self.transcript.len().saturating_sub(TRANSCRIPT_TAIL);
        while !self.transcript.is_char_boundary(from) {
            from += 1;
        }
        BootstrapError {
            step: step.to_string(),
            cause,
            elapsed: self.started.elapsed(),
            output: self.transcript[from..].to_string(),
        }
    }

    /// Send `s` followed by Enter (CR), the way a user pressing Return
    /// drives the line editor. Unlike expectrl's `send_line` (which
    /// sends `\n`), this sends `\r` so PSReadLine and reedline accept
    /// the line instead of treating it as a multi-line continuation.
    pub fn send_line(&mut self, s: &str) -> Option<()> {
        self.write_line(s).ok()
    }

    fn write_line(&mut self, s: &str) -> std::io::Result<()> {
        self.inner.write_all(s.as_bytes())?;
        self.inner.write_all(ENTER)?;
        self.inner.flush()
    }

    /// Send `s` and Enter, then block until the shell has finished
    /// executing it and returned to a prompt. Proven by printing a
    /// token that appears **only in the command's output, never in its
    /// echo**: the line concatenates two halves (`echo <a><b>`) that
    /// the shell joins at runtime, so the terminal echo shows the two
    /// literals separately while the executed output shows the joined
    /// token. Waiting for the joined form therefore fires exactly once,
    /// when the line has run — regardless of whether the shell echoes
    /// typed input (pwsh, nu do; bash, zsh do not). Serialising each
    /// bootstrap line this way stops a line editor from accumulating
    /// several unexecuted lines into one multi-line buffer.
    fn send_line_synced(&mut self, s: &str, sep: &str, echo: &str) -> Result<(), BootstrapError> {
        // A fresh token per call so a stale one from an earlier line
        // can't satisfy the wait.
        self.sync_counter += 1;
        let joined = format!("RXSYNC{}DONE", self.sync_counter);
        let (a, b) = joined.split_at(joined.len() / 2);
        self.seen.clear();
        // `'<a>' + '<b>'` (pwsh) / `'<a>' + '<b>'`… differ per shell,
        // so the caller passes the concatenation form via `echo`. But
        // every supported shell concatenates two adjacent quoted
        // string literals inside its echo/print with `+`, except the
        // POSIX shells which need no operator. Keep it uniform by
        // interpolating the two halves into the shell's own string
        // syntax: for all four, `<echo>'<a>' + '<b>'` is wrong for
        // bash. So build per style below.
        let printer = if echo.starts_with("echo ") {
            // POSIX shells: adjacent quoted literals concatenate.
            format!("{echo}'{a}''{b}'")
        } else {
            // pwsh (Write-Host) and nu (print): `+` joins two strings.
            format!("{echo}('{a}' + '{b}')")
        };
        self.write_line(&format!("{s}{sep}{printer}"))
            .map_err(|e| self.failure(s, Cause::Write(e.to_string())))?;
        self.read_until(&joined, self.step_timeout).map_err(|cause| self.failure(s, cause))
    }

    /// Send `s` *without* a trailing Enter. Used when a test wants to
    /// type a token and then a *separate* keystroke (e.g. Space) to
    /// trigger the abbr expansion.
    pub fn send(&mut self, s: &str) -> Option<()> {
        self.inner.write_all(s.as_bytes()).ok()?;
        self.inner.flush().ok()
    }

    /// Send a bare Enter (CR) — used after [`Self::send`] typed a
    /// trigger key, to submit the resulting line.
    pub fn enter(&mut self) -> Option<()> {
        self.inner.write_all(ENTER).ok()?;
        self.inner.flush().ok()
    }

    /// Block until `needle` appears in the child's output, answering
    /// any DSR cursor-position query in the meantime so the shell's
    /// line editor never blocks on us. Returns `Some(())` on match and
    /// `None` on the [`DEFAULT_TIMEOUT`] deadline or a read error.
    ///
    /// `needle` is matched as a literal substring, not a regex: the
    /// sentinel and the expansions under test contain no metacharacters
    /// and a literal match can't be fooled by an unescaped `.`.
    pub fn expect_regex(&mut self, needle: &str) -> Option<()> {
        self.read_until(needle, DEFAULT_TIMEOUT).ok()
    }

    /// Block until the [`SENTINEL_PROMPT`] appears.
    pub fn expect_prompt(&mut self) -> Option<()> {
        self.expect_regex(SENTINEL_PROMPT)
    }

    /// Read from the child until `needle` is seen or `deadline`
    /// elapses, answering every [`DSR_QUERY`] as it arrives. All
    /// output is accumulated in `self.seen` so a later `expect` can
    /// match text that arrived before it was called (the child is
    /// faster than the test). Everything read also goes to
    /// `self.transcript`.
    fn read_until(&mut self, needle: &str, deadline: Duration) -> Result<(), Cause> {
        let start = Instant::now();
        let mut buf = [0u8; 4096];
        // Only the bytes that arrived since the last scan can complete
        // a match, so scan the tail rather than all of `seen`: with
        // small reads a full rescan per read is quadratic and can eat
        // the deadline before a chatty shell has even finished its
        // banner.
        let mut scanned: usize = 0;
        loop {
            // A match can start no earlier than needle.len()-1 bytes
            // before the previously scanned end.
            let from = scanned
                .saturating_sub(needle.len().saturating_sub(1))
                .min(self.seen.len());
            let from = (0..=from).rev().find(|&i| self.seen.is_char_boundary(i)).unwrap_or(0);
            if self.seen[from..].contains(needle) {
                return Ok(());
            }
            scanned = self.seen.len();
            if start.elapsed() > deadline {
                return Err(Cause::Timeout(deadline));
            }
            match self.inner.try_read(&mut buf) {
                Ok(0) => std::thread::sleep(Duration::from_millis(10)),
                Ok(n) => {
                    let chunk = &buf[..n];
                    if dsr_query_completes(&mut self.dsr_carry, chunk) {
                        let _ = self.inner.write_all(DSR_ANSWER);
                        let _ = self.inner.flush();
                    }
                    self.last_output = Some(Instant::now());
                    let text = String::from_utf8_lossy(chunk);
                    self.seen.push_str(&text);
                    self.transcript.push_str(&text);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(Cause::Read(e.to_string())),
            }
        }
    }

    /// Wait until the shell's line editor reads keystrokes (see
    /// [`startup_is_ready`]), answering any DSR query on the way. A
    /// line sent earlier can reach the editor before it is ready: on a
    /// slow macOS runner pwsh then rendered the first bootstrap line but
    /// never ran it. A shell that has not printed its prompt within
    /// `step_timeout`, including one that exited, fails at `startup`.
    fn settle_startup(&mut self, shell: PtyShell) -> Result<(), BootstrapError> {
        let start = Instant::now();
        loop {
            // A needle that never arrives makes `read_until` a drain
            // that answers DSR queries for one poll interval.
            match self.read_until("\u{0}__never__\u{0}", STARTUP_POLL) {
                Ok(()) | Err(Cause::Timeout(_)) => {}
                Err(cause) => return Err(self.failure("startup", cause)),
            }
            let quiet = self.last_output.map_or(Duration::ZERO, |t| t.elapsed());
            if startup_is_ready(shell, &self.seen, quiet) {
                self.seen.clear();
                return Ok(());
            }
            if start.elapsed() > self.step_timeout {
                return Err(self.failure("startup", Cause::Timeout(self.step_timeout)));
            }
        }
    }

    #[allow(dead_code)]
    /// Whether `needle` has appeared in the child's output so far.
    /// The negative-control tests use it to assert that something did
    /// *not* happen, which `expect_regex` (a positive wait) cannot say.
    pub fn saw(&self, needle: &str) -> bool {
        self.seen.contains(needle)
    }

    /// Polite shutdown. Sends `exit`; if the shell ignores it, drop
    /// reaps the child anyway.
    pub fn quit(mut self) {
        let _ = self.send_line("exit");
    }
}

/// How long the shell must print nothing after its ready signal
/// before the first bootstrap line is sent.
const STARTUP_QUIET: Duration = Duration::from_millis(100);

/// How often [`PtySession::settle_startup`] re-checks readiness.
const STARTUP_POLL: Duration = Duration::from_millis(20);

/// Whether a freshly launched `shell` reads keystrokes, given what it
/// has printed since launch and how long it has printed nothing since.
///
/// The shell is ready once its default prompt has been printed and
/// [`STARTUP_QUIET`] has passed without output. The quiet period covers
/// what an editor does after the prompt: pwsh's PSReadLine sends a DSR
/// query, except under `TERM=dumb` in a wide window, and blocks until it
/// is answered.
fn startup_is_ready(shell: PtyShell, output: &str, quiet: Duration) -> bool {
    default_prompt_ends(shell).iter().any(|end| output.contains(end)) && quiet >= STARTUP_QUIET
}

/// Text that ends `shell`'s default prompt as launched by
/// [`launch_command`]: `bash-5.3$ `, `host% `, `PS /tmp> `, and for nu
/// the OSC 133 mark that reedline prints where input starts. bash and
/// zsh end in `# ` for root.
fn default_prompt_ends(shell: PtyShell) -> &'static [&'static str] {
    match shell {
        PtyShell::Bash => &["$ ", "# "],
        PtyShell::Zsh => &["% ", "# "],
        PtyShell::Pwsh => &["> "],
        PtyShell::Nu => &["\x1b]133;B"],
    }
}

fn launch_command(shell: PtyShell, _runex_bin: &str) -> String {
    match shell {
        // Interactive bash so readline loads, but no rcfile so the
        // user's environment can't smuggle aliases or prompt code in.
        PtyShell::Bash => "bash --norc --noprofile -i".to_string(),
        // zsh: -f skips zshrc/zshenv (`--no-rcs`-ish), -i forces
        // interactive so zle loads.
        PtyShell::Zsh => "zsh -f -i".to_string(),
        // pwsh: -NoLogo silences the banner, -NoProfile avoids
        // sourcing $PROFILE. PSReadLine ships in the default
        // distribution so no extra import is needed for the runex
        // integration to bind.
        PtyShell::Pwsh => "pwsh -NoLogo -NoProfile".to_string(),
        // nu --no-config-file: skip the user's $env / config.nu so
        // unrelated keybindings don't interfere. We still need an
        // interactive REPL so reedline reads keystrokes; nu defaults
        // to interactive when stdin is a tty (which the PTY provides),
        // so no extra flag is needed.
        PtyShell::Nu => "nu --no-config-file".to_string(),
    }
}

fn bootstrap(
    session: &mut PtySession,
    shell: PtyShell,
    runex_bin: &str,
    config: &Path,
) -> Result<(), BootstrapError> {
    let cfg = config.display();
    // Per-shell statement separator and echo command, used by
    // `send_line_synced` to append a "this line finished" token to
    // each bootstrap line. Serialising the lines this way is what
    // stops PSReadLine / reedline from accumulating several unexecuted
    // lines into a single multi-line buffer.
    let (sep, echo) = match shell {
        PtyShell::Bash | PtyShell::Zsh => ("; ", "echo "),
        PtyShell::Pwsh => ("; ", "Write-Host "),
        PtyShell::Nu => ("; ", "print "),
    };
    match shell {
        PtyShell::Bash => {
            // Disable bracketed paste so individual key sends aren't
            // wrapped in ESC[200~ … ESC[201~ by terminals that try
            // to be clever.
            session.send_line_synced("bind 'set enable-bracketed-paste off' 2>/dev/null", sep, echo)?;
            session.send_line_synced(&format!("PS1='{SENTINEL_PROMPT}'"), sep, echo)?;
            session.send_line_synced(&format!("export RUNEX_CONFIG={cfg}"), sep, echo)?;
            session.send_line_synced(
                &format!(r#"eval "$('{runex_bin}' export bash --bin '{runex_bin}')""#),
                sep,
                echo,
            )?;
        }
        PtyShell::Zsh => {
            session.send_line_synced(&format!("PROMPT='{SENTINEL_PROMPT}'"), sep, echo)?;
            session.send_line_synced(&format!("export RUNEX_CONFIG={cfg}"), sep, echo)?;
            session.send_line_synced(
                &format!(r#"eval "$('{runex_bin}' export zsh --bin '{runex_bin}')""#),
                sep,
                echo,
            )?;
        }
        PtyShell::Pwsh => {
            // pwsh `prompt` is a function returning the prompt
            // string. Quoting the sentinel as a single-quoted string
            // keeps PowerShell from interpolating anything inside.
            session.send_line_synced(&format!("function prompt {{ '{SENTINEL_PROMPT}' }}"), sep, echo)?;
            session.send_line_synced(&format!("$env:RUNEX_CONFIG = '{cfg}'"), sep, echo)?;
            session.send_line_synced(
                &format!("Invoke-Expression (& '{runex_bin}' export pwsh --bin '{runex_bin}' | Out-String)"),
                sep,
                echo,
            )?;
        }
        PtyShell::Nu => {
            // nu's `source` resolves paths at parse time, so we cannot
            // pipe `runex export nu` into source the way bash/zsh's
            // `eval "$(...)"` works. We use the test runner's $TMPDIR
            // (or /tmp) to write runex.nu and then source it. The path
            // ends up uniquely named per session, so concurrent test
            // invocations don't clobber each other.
            let nu_path = std::env::temp_dir()
                .join(format!("runex-pty-{}.nu", std::process::id()));
            // Generate the script *outside* the PTY to avoid having
            // to wait for a sentinel between the save and the source.
            let export_step = "runex export nu";
            let out = std::process::Command::new(runex_bin)
                .args(["--config"])
                .arg(config)
                .args(["export", "nu", "--bin", runex_bin])
                .output()
                .map_err(|e| session.failure(export_step, Cause::Export(e.to_string())))?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                return Err(session.failure(export_step, Cause::Export(format!("{}: {stderr}", out.status))));
            }
            std::fs::write(&nu_path, &out.stdout)
                .map_err(|e| session.failure(export_step, Cause::Export(e.to_string())))?;

            // Install a custom prompt by setting PROMPT_COMMAND. nu's
            // PROMPT_COMMAND is evaluated each render, so a static
            // string is fine. PROMPT_INDICATOR* vars must be cleared
            // so reedline doesn't append `> ` after our sentinel.
            session.send_line_synced(
                "$env.PROMPT_COMMAND = '__RUNEX_PROMPT__> '; $env.PROMPT_COMMAND_RIGHT = ''; $env.PROMPT_INDICATOR = ''; $env.PROMPT_INDICATOR_VI_INSERT = ''; $env.PROMPT_INDICATOR_VI_NORMAL = ''; $env.PROMPT_MULTILINE_INDICATOR = ''",
                sep,
                echo,
            )?;
            session.send_line_synced(&format!("$env.RUNEX_CONFIG = '{cfg}'"), sep, echo)?;
            session.send_line_synced(&format!("source '{}'", nu_path.display()), sep, echo)?;
        }
    }
    Ok(())
}

/// Whether `chunk`, read after the bytes in `carry`, completes a
/// [`DSR_QUERY`]. A query can straddle two `try_read` results, so the
/// scan runs over `carry ++ chunk` and `carry` is left holding the
/// last `DSR_QUERY.len() - 1` bytes for the next call. Pure so it can
/// be tested with hand-split sequences.
fn dsr_query_completes(carry: &mut Vec<u8>, chunk: &[u8]) -> bool {
    let mut window = std::mem::take(carry);
    window.extend_from_slice(chunk);
    let found = window.windows(DSR_QUERY.len()).any(|w| w == DSR_QUERY);
    let keep = window.len().saturating_sub(DSR_QUERY.len() - 1);
    *carry = window[keep..].to_vec();
    found
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    const QUIET: Duration = STARTUP_QUIET;
    const BUSY: Duration = Duration::from_millis(10);

    /// What each shell printed under a PTY before its first prompt, and
    /// the prompt itself, as recorded from bash 5.3, zsh 5.9, pwsh 7.6
    /// and nu 0.112.2.
    const STARTUPS: [(PtyShell, &str, &str); 4] = [
        (PtyShell::Bash, "\x1b[?2004h", "bash-5.3$ "),
        (
            PtyShell::Zsh,
            "\x1b[1m\x1b[7m%\x1b[27m\x1b[1m\x1b[0m    \r \r",
            "\r\x1b[0m\x1b[27m\x1b[24m\x1b[Jhost% \x1b[K\x1b[?2004h",
        ),
        (PtyShell::Pwsh, "\x1b[?1h\x1b=\x1b[6n", "PS /tmp> \x1b[?1h\x1b[6n"),
        (
            PtyShell::Nu,
            "\x1b]2;/tmp\x07\x1b[?2004h\x1b[6n\x1b[6n",
            "\x1b[0m\x1b]133;A;k=i\x1b\\\x1b[38;5;10m/tmp\x1b[38;5;14m> \x1b]133;B\x1b\\\x1b[0m",
        ),
    ];

    #[test]
    fn no_shell_is_ready_before_its_prompt() {
        for (shell, before, _) in STARTUPS {
            assert!(!startup_is_ready(shell, before, QUIET), "{shell:?}");
        }
    }

    #[test]
    fn each_shell_is_ready_once_its_prompt_is_followed_by_quiet() {
        for (shell, before, prompt) in STARTUPS {
            assert!(startup_is_ready(shell, &format!("{before}{prompt}"), QUIET), "{shell:?}");
        }
    }

    #[test]
    fn no_shell_is_ready_while_it_is_still_printing() {
        for (shell, before, prompt) in STARTUPS {
            assert!(!startup_is_ready(shell, &format!("{before}{prompt}"), BUSY), "{shell:?}");
        }
    }

    #[test]
    fn pwsh_under_a_dumb_terminal_is_ready_without_a_query_after_its_prompt() {
        assert!(startup_is_ready(PtyShell::Pwsh, "\x1b[6nPS /tmp> ", QUIET));
    }

    #[test]
    fn bash_and_zsh_are_ready_on_a_root_prompt() {
        assert!(startup_is_ready(PtyShell::Bash, "bash-5.3# ", QUIET));
        assert!(startup_is_ready(PtyShell::Zsh, "host# ", QUIET));
    }

    #[test]
    fn a_shell_that_exits_without_a_prompt_fails_at_startup() {
        let mut session = PtySession::launch("echo bye").expect("echo runs under a PTY");
        session.step_timeout = Duration::from_millis(200);
        let err = session
            .settle_startup(PtyShell::Bash)
            .expect_err("echo prints a line and exits without a prompt");
        assert_eq!(err.step, "startup", "{err:?}");
    }
}

#[cfg(test)]
mod bootstrap_error_tests {
    use super::*;

    #[test]
    fn a_line_that_never_finishes_reports_the_line_the_cause_and_the_output() {
        // Short, so this test does not load the runner while the shell
        // tests in the same binary are starting their shells.
        let deadline = Duration::from_millis(200);
        let mut session = PtySession::launch("cat").expect("cat runs under a PTY");
        session.step_timeout = deadline;
        let err = session
            .send_line_synced("true", "; ", "echo ")
            .expect_err("cat echoes the halves but never prints the joined sync token");
        assert_eq!(err.step, "true");
        assert!(matches!(err.cause, Cause::Timeout(d) if d == deadline), "{err:?}");
        assert!(err.elapsed >= deadline, "{err:?}");
        assert!(err.output.contains("RXSYN"), "{err:?}");
    }

    #[test]
    fn the_failure_message_names_the_step_the_cause_and_the_output() {
        let err = BootstrapError {
            step: "function prompt { 'p' }".to_string(),
            cause: Cause::Timeout(DEFAULT_TIMEOUT),
            elapsed: Duration::from_millis(6200),
            output: "PowerShell 7\r\n".to_string(),
        };
        let message = format!("{err:?}");
        for part in ["function prompt { 'p' }", "Timeout(5s)", "6.2s", r#""PowerShell 7\r\n""#] {
            assert!(message.contains(part), "{part:?} missing from {message}");
        }
    }
}

#[cfg(test)]
mod dsr_tests {
    use super::*;

    #[test]
    fn query_inside_one_chunk_is_found() {
        let mut carry = Vec::new();
        assert!(dsr_query_completes(&mut carry, b"prompt> \x1b[6n more"));
    }

    #[test]
    fn query_split_across_two_chunks_is_found_on_the_second() {
        let mut carry = Vec::new();
        assert!(!dsr_query_completes(&mut carry, b"text\x1b["));
        assert!(dsr_query_completes(&mut carry, b"6n"));
    }

    #[test]
    fn query_split_one_byte_per_chunk_is_found_on_the_last() {
        let mut carry = Vec::new();
        for b in [b"\x1b" as &[u8], b"[", b"6"] {
            assert!(!dsr_query_completes(&mut carry, b));
        }
        assert!(dsr_query_completes(&mut carry, b"n"));
    }

    #[test]
    fn unrelated_bytes_never_match_and_carry_stays_bounded() {
        let mut carry = Vec::new();
        assert!(!dsr_query_completes(&mut carry, b"\x1b[0m\x1b[K some output"));
        assert!(carry.len() < DSR_QUERY.len());
    }
}
