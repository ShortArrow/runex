//! PTY-based integration tests for bash shell integration.
//!
//! These tests spawn a real bash process via PTY (rexpect) and verify that
//! `bind -x` space-key expansion works end-to-end.
//!
//! Run with: `cargo test -p runex-cli -- --ignored`

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
        env!("CARGO_BIN_EXE_runex-cli")
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

        p.send_line(&format!("eval \"$({bin} export bash --bin {bin})\""))
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
            "eval \"$({bin} export bash --bin {bin})\"; echo EXIT:$?"
        ))
        .unwrap();

        let (out, _) = p.exp_regex("EXIT:[0-9]+").unwrap();
        assert!(
            !out.contains("EXIT:1"),
            "source should succeed without errors, got: {out}"
        );
    }

    /// `gcm` + space → `echo EXPANDED` に展開され、Enter で EXPANDED が出力される。
    #[test]
    #[ignore]
    fn test_expand_on_space() {
        let config = write_config();
        let mut p = spawn_bash_with_integration(&config);

        // "gcm" + space triggers bind-x → READLINE_LINE becomes "echo EXPANDED "
        // Enter executes the expanded command
        p.send("gcm ").unwrap();
        p.send_line("").unwrap();

        p.exp_string("EXPANDED").unwrap();
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
}
