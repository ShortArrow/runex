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

use std::path::Path;
use std::time::Duration;

use expectrl::Regex;
use expectrl::session::Session;

/// The sentinel prompt every PTY session installs. Chosen to be
/// unmistakably ours so an `expect(Regex(SENTINEL))` cannot match
/// anything that scrolls in from the shell's own MOTD or readline
/// banners.
pub const SENTINEL_PROMPT: &str = "__RUNEX_PROMPT__> ";

/// Default per-`expect` timeout. CI runners with slow IO need
/// generous headroom; production keystroke latency is microseconds,
/// so 5 seconds is "definitely broken if we hit it" rather than
/// "might be slow".
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

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
        session.set_expect_timeout(Some(DEFAULT_TIMEOUT));
        bootstrap(&mut session, shell, runex_bin, config)?;
        Some(Self { inner: session })
    }

    /// Send `s` followed by an Enter keystroke. Mirrors expectrl's
    /// `send_line`; surfaced here so callers don't have to reach
    /// into `inner`.
    pub fn send_line(&mut self, s: &str) -> Option<()> {
        self.inner.send_line(s).ok()
    }

    /// Send `s` *without* a trailing newline. Used when a test wants
    /// to type a token and then a *separate* keystroke (e.g. Space)
    /// to trigger the abbr expansion.
    pub fn send(&mut self, s: &str) -> Option<()> {
        self.inner.send(s).ok()
    }

    /// Block until `pattern` (a regex) matches output from the
    /// child. Returns `Some(())` on match, `None` on timeout — same
    /// permissive style as [`Self::spawn`].
    pub fn expect_regex(&mut self, pattern: &str) -> Option<()> {
        self.inner.expect(Regex(pattern)).ok().map(|_| ())
    }

    /// Block until the [`SENTINEL_PROMPT`] appears. The sentinel
    /// contains no regex metacharacters (`__RUNEX_PROMPT__> ` is
    /// underscores + caps + `> ` — none are special), so we hand it
    /// to expectrl as-is.
    pub fn expect_prompt(&mut self) -> Option<()> {
        self.expect_regex(SENTINEL_PROMPT)
    }

    /// Polite shutdown. Sends an EOF; if the shell ignores it, drop
    /// will reap the child anyway.
    pub fn quit(mut self) {
        let _ = self.inner.send_line("exit");
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
    session: &mut Session,
    shell: PtyShell,
    runex_bin: &str,
    config: &Path,
) -> Option<()> {
    let cfg = config.display();
    match shell {
        PtyShell::Bash => {
            // Disable bracketed paste so individual key sends aren't
            // wrapped in ESC[200~ … ESC[201~ by terminals that try
            // to be clever.
            session
                .send_line("bind 'set enable-bracketed-paste off' 2>/dev/null")
                .ok()?;
            session.send_line(&format!("PS1='{SENTINEL_PROMPT}'")).ok()?;
            session.send_line(&format!("export RUNEX_CONFIG={cfg}")).ok()?;
            session
                .send_line(&format!(
                    r#"eval "$('{runex_bin}' export bash --bin '{runex_bin}')""#
                ))
                .ok()?;
        }
        PtyShell::Zsh => {
            session.send_line(&format!("PROMPT='{SENTINEL_PROMPT}'")).ok()?;
            session.send_line(&format!("export RUNEX_CONFIG={cfg}")).ok()?;
            session
                .send_line(&format!(
                    r#"eval "$('{runex_bin}' export zsh --bin '{runex_bin}')""#
                ))
                .ok()?;
        }
        PtyShell::Pwsh => {
            // pwsh `prompt` is a function returning the prompt
            // string. Quoting the sentinel as a single-quoted string
            // keeps PowerShell from interpolating anything inside.
            session
                .send_line(&format!(
                    "function prompt {{ '{SENTINEL_PROMPT}' }}"
                ))
                .ok()?;
            session
                .send_line(&format!("$env:RUNEX_CONFIG = '{cfg}'"))
                .ok()?;
            session
                .send_line(&format!(
                    "Invoke-Expression (& '{runex_bin}' export pwsh --bin '{runex_bin}' | Out-String)"
                ))
                .ok()?;
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
            session
                .send_line(&format!(
                    "$env.PROMPT_COMMAND = '{SENTINEL_PROMPT}'; $env.PROMPT_COMMAND_RIGHT = ''; $env.PROMPT_INDICATOR = ''; $env.PROMPT_INDICATOR_VI_INSERT = ''; $env.PROMPT_INDICATOR_VI_NORMAL = ''; $env.PROMPT_MULTILINE_INDICATOR = ''"
                ))
                .ok()?;
            session
                .send_line(&format!("$env.RUNEX_CONFIG = '{cfg}'"))
                .ok()?;
            session
                .send_line(&format!("source '{}'", nu_path.display()))
                .ok()?;
        }
    }
    // Wait for two sentinels: the post-PROMPT one, then the post-
    // integration-source one. Matching them as a pair guarantees the
    // integration is in place before any keystroke test fires.
    // Sentinel has no regex metacharacters so it's literal-safe.
    let pat = format!(r"{SENTINEL_PROMPT}.*{SENTINEL_PROMPT}");
    session.expect(Regex(&pat)).ok()?;
    Some(())
}
