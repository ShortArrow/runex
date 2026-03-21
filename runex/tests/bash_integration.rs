//! PTY-based integration tests for bash shell integration.
//!
//! These tests spawn a real bash process via PTY (rexpect) and verify that
//! `bind -x` space-key expansion works end-to-end.
//!
//! Run with: `cargo test -p runex -- --ignored`

#[cfg(target_family = "unix")]
mod bash {
    use std::io::Write;
    use tempfile::NamedTempFile;

    const PROMPT: &str = "TEST> ";
    const TIMEOUT_MS: u64 = 5_000;

    /// Create a temp config where `gcm` expands to `echo EXPANDED`.
    fn write_config() -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(
            f,
            "version = 1\n\n[[abbr]]\nkey = \"gcm\"\nexpand = \"echo EXPANDED\"\n"
        )
        .unwrap();
        f.flush().unwrap();
        f
    }

    fn bin_path() -> &'static str {
        env!("CARGO_BIN_EXE_runex")
    }

    /// Spawn bash with a known prompt, env, and sourced integration script.
    fn spawn_bash_with_integration(
        config: &NamedTempFile,
    ) -> rexpect::session::PtySession {
        let bin = bin_path();
        let cmd = format!(
            "env RUNEX_CONFIG={} PS1='{PROMPT}' bash --norc --noprofile -i",
            config.path().display()
        );
        let mut p = rexpect::spawn(&cmd, Some(TIMEOUT_MS)).unwrap();
        p.exp_string(PROMPT).unwrap(); // wait for initial prompt

        // PTY test input can be treated as bracketed paste by modern readline,
        // which bypasses the space key binding we want to verify.
        p.send_line("bind 'set enable-bracketed-paste off'")
            .unwrap();
        p.exp_string(PROMPT).unwrap();

        p.send_line(&format!("source <({bin} export bash --bin {bin})"))
            .unwrap();
        p.exp_string(PROMPT).unwrap();
        p
    }

    /// source the integration script without errors.
    #[test]
    #[ignore]
    fn test_source_no_error() {
        let config = write_config();
        let bin = bin_path();

        let cmd = format!(
            "env RUNEX_CONFIG={} PS1='{PROMPT}' bash --norc --noprofile -i",
            config.path().display()
        );
        let mut p = rexpect::spawn(&cmd, Some(TIMEOUT_MS)).unwrap();
        p.exp_string(PROMPT).unwrap();

        p.send_line(&format!(
            "source <({bin} export bash --bin {bin}); echo EXIT:$?"
        ))
        .unwrap();

        let (out, _) = p.exp_regex("EXIT:[0-9]+").unwrap();
        assert!(
            !out.contains("EXIT:1"),
            "source should succeed without errors, got: {out}"
        );
    }

    /// The exported helper expands the current readline token and appends a space.
    #[test]
    #[ignore]
    fn test_expand_helper() {
        let config = write_config();
        let mut p = spawn_bash_with_integration(&config);

        p.send_line("READLINE_LINE='gcm'; READLINE_POINT=3; __runex_expand; printf '<%s>\\n' \"$READLINE_LINE\"").unwrap();
        p.exp_string("<echo EXPANDED >").unwrap();
    }

    /// Mid-line space insertion should not trigger expansion.
    #[test]
    #[ignore]
    fn test_midline_space_is_plain_insert() {
        let config = write_config();
        let mut p = spawn_bash_with_integration(&config);

        p.send_line(
            "READLINE_LINE='gcm tail'; READLINE_POINT=1; __runex_expand; printf '<%s>\\n' \"$READLINE_LINE\"",
        )
        .unwrap();
        p.exp_string("<g cm tail>").unwrap();
    }

    /// The token after a command separator is expanded without touching the prefix.
    #[test]
    #[ignore]
    fn test_expand_after_separator() {
        let config = write_config();
        let mut p = spawn_bash_with_integration(&config);

        p.send_line(
            "READLINE_LINE='echo foo && gcm'; READLINE_POINT=15; __runex_expand; printf '<%s>\\n' \"$READLINE_LINE\"",
        )
        .unwrap();
        p.exp_string("<echo foo && echo EXPANDED >").unwrap();
    }

    /// Argument positions should stay plain even if the token matches an abbreviation.
    #[test]
    #[ignore]
    fn test_argument_position_does_not_expand() {
        let config = write_config();
        let mut p = spawn_bash_with_integration(&config);

        p.send_line(
            "READLINE_LINE='echo gcm'; READLINE_POINT=8; __runex_expand; printf '<%s>\\n' \"$READLINE_LINE\"",
        )
        .unwrap();
        p.exp_string("<echo gcm >").unwrap();
    }

    /// `sudo` preserves command-position expansion for the following token.
    #[test]
    #[ignore]
    fn test_sudo_position_expands() {
        let config = write_config();
        let mut p = spawn_bash_with_integration(&config);

        p.send_line(
            "READLINE_LINE='sudo gcm'; READLINE_POINT=8; __runex_expand; printf '<%s>\\n' \"$READLINE_LINE\"",
        )
        .unwrap();
        p.exp_string("<sudo echo EXPANDED >").unwrap();
    }

    /// Unknown token is not expanded — stays as-is.
    #[test]
    #[ignore]
    fn test_no_expand_unknown_token() {
        let config = write_config();
        let mut p = spawn_bash_with_integration(&config);

        // "xyz" is not in config → stays as "xyz ", bash reports command not found
        p.send("xyz ").unwrap();
        p.send_line("").unwrap();

        // Wait for the next prompt (command will fail, that's OK)
        let (out, _) = p.exp_regex(PROMPT).unwrap();
        assert!(
            !out.contains("EXPANDED"),
            "unknown token must not expand, got: {out}"
        );
    }

    /// Option-like tokens must not be eaten by the helper.
    #[test]
    #[ignore]
    fn test_option_like_token_stays_intact() {
        let config = write_config();
        let mut p = spawn_bash_with_integration(&config);

        p.send_line(
            "READLINE_LINE='cargo install --path'; READLINE_POINT=20; __runex_expand; printf '<%s>\\n' \"$READLINE_LINE\"",
        )
        .unwrap();
        p.exp_string("<cargo install --path >").unwrap();
    }
}
