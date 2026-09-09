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
}

impl PtySession {
    /// Spawn `shell` under a PTY, set `RUNEX_CONFIG=<config>`,
    /// install the [`SENTINEL_PROMPT`], source the runex integration
    /// produced by `runex export <shell>`, and block until the post-
    /// source prompt has settled.
    ///
    /// Returns `None` if the shell can't be launched or any of the
    /// setup steps don't complete within [`DEFAULT_TIMEOUT`]. This
    /// is intentionally permissive — tests use `let Some(s) = … else
    /// { return; };` as a runtime skip when the shell isn't
    /// installed.
    pub fn spawn(shell: PtyShell, runex_bin: &str, config: &Path) -> Option<Self> {
        let launch = launch_command(shell, runex_bin);
        let mut session = expectrl::spawn(&launch).ok()?;
        // A wide, tall window so a wrapped line doesn't derail the
        // editor's cursor tracking during a keystroke test. The PTY
        // otherwise defaults to a small size. Best-effort.
        let _ = session.get_process_mut().set_window_size(240, 60);
        let mut this = Self { inner: session, seen: String::new(), sync_counter: 0 };
        // Wait for the shell's first interactive prompt before sending
        // anything. reedline / PSReadLine emit a DSR query on startup
        // and block until it is answered; `read_until` answers it. A
        // shell whose prompt we can't predict (bash/zsh here still have
        // their default prompt) just needs its startup output to settle,
        // so we drain briefly by waiting on a token that won't come and
        // letting the deadline pass is wasteful — instead, run one sync
        // round against the *current* (default) prompt via a harmless
        // command. `send_line_synced` already proves round-trip.
        this.settle_startup(shell)?;
        bootstrap(&mut this, shell, runex_bin, config)?;
        // Drop the bootstrap chatter so a test's first `expect` matches
        // only what its own keystrokes produce.
        this.seen.clear();
        Some(this)
    }

    /// Send `s` followed by Enter (CR), the way a user pressing Return
    /// drives the line editor. Unlike expectrl's `send_line` (which
    /// sends `\n`), this sends `\r` so PSReadLine and reedline accept
    /// the line instead of treating it as a multi-line continuation.
    pub fn send_line(&mut self, s: &str) -> Option<()> {
        self.inner.write_all(s.as_bytes()).ok()?;
        self.inner.write_all(ENTER).ok()?;
        self.inner.flush().ok()
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
    fn send_line_synced(&mut self, s: &str, sep: &str, echo: &str) -> Option<()> {
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
        self.send_line(&format!("{s}{sep}{printer}"))?;
        self.read_until(&joined, DEFAULT_TIMEOUT)
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
    /// line editor never blocks on us. Returns `Some(())` on match,
    /// `None` on the [`DEFAULT_TIMEOUT`] deadline — the permissive
    /// style [`Self::spawn`] relies on for its runtime skip.
    ///
    /// `needle` is matched as a literal substring, not a regex: the
    /// sentinel and the expansions under test contain no metacharacters
    /// and a literal match can't be fooled by an unescaped `.`.
    pub fn expect_regex(&mut self, needle: &str) -> Option<()> {
        self.read_until(needle, DEFAULT_TIMEOUT)
    }

    /// Block until the `n`th occurrence of `needle` appears — used to
    /// distinguish a buffer render of a word from the command output
    /// that prints the same word after submission.
    pub fn expect_regex_nth(&mut self, needle: &str, n: usize) -> Option<()> {
        self.read_until_nth(needle, n, DEFAULT_TIMEOUT)
    }

    /// Block until the [`SENTINEL_PROMPT`] appears.
    pub fn expect_prompt(&mut self) -> Option<()> {
        self.expect_regex(SENTINEL_PROMPT)
    }

    /// Read from the child until `needle` is seen or `deadline`
    /// elapses, answering every [`DSR_QUERY`] as it arrives. All
    /// output is accumulated in `self.seen` so a later `expect` can
    /// match text that arrived before it was called (the child is
    /// faster than the test).
    fn read_until(&mut self, needle: &str, deadline: Duration) -> Option<()> {
        self.read_until_nth(needle, 1, deadline)
    }

    /// Like [`Self::read_until`] but waits for the `n`th occurrence of
    /// `needle`. Used to skip a command's own terminal echo (1st
    /// occurrence) and wait for the output it printed (2nd).
    fn read_until_nth(&mut self, needle: &str, n: usize, deadline: Duration) -> Option<()> {
        let start = Instant::now();
        let mut buf = [0u8; 4096];
        loop {
            if self.seen.matches(needle).count() >= n {
                return Some(());
            }
            if start.elapsed() > deadline {
                return None;
            }
            match self.inner.try_read(&mut buf) {
                Ok(0) => std::thread::sleep(Duration::from_millis(10)),
                Ok(n) => {
                    let chunk = &buf[..n];
                    if chunk.windows(DSR_QUERY.len()).any(|w| w == DSR_QUERY) {
                        let _ = self.inner.write_all(DSR_ANSWER);
                        let _ = self.inner.flush();
                    }
                    self.seen.push_str(&String::from_utf8_lossy(chunk));
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return None,
            }
        }
    }

    /// Let the shell's startup output settle before the bootstrap
    /// lines are sent, answering the DSR cursor-position query that
    /// reedline / PSReadLine emit on startup and block on. We can't
    /// wait for a known prompt (each shell's default prompt differs and
    /// isn't ours yet), so we drain for a short fixed window, replying
    /// to any DSR query, which is enough for the line editor to reach
    /// its first interactive prompt. `read_until` on a needle that
    /// never arrives is exactly this drain-with-DSR loop, bounded by
    /// the deadline.
    fn settle_startup(&mut self, _shell: PtyShell) -> Option<()> {
        // Deliberately wait on a sentinel that never comes so the loop
        // spends its whole (short) budget draining + answering DSR.
        let _ = self.read_until("\u{0}__never__\u{0}", Duration::from_millis(600));
        self.seen.clear();
        Some(())
    }

    /// Polite shutdown. Sends `exit`; if the shell ignores it, drop
    /// reaps the child anyway.
    pub fn quit(mut self) {
        let _ = self.send_line("exit");
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
) -> Option<()> {
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
            let out = std::process::Command::new(runex_bin)
                .args(["--config"])
                .arg(config)
                .args(["export", "nu", "--bin", runex_bin])
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            std::fs::write(&nu_path, &out.stdout).ok()?;

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
    Some(())
}
