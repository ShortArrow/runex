//! Integration tests for bash shell integration.
//!
//! Spawns a non-interactive bash subprocess, sources the integration script,
//! then exercises `__runex_expand` via READLINE_LINE / READLINE_POINT.
//! No PTY required. Tests are skipped at runtime if `bash` is not found.

#[cfg(target_family = "unix")]
mod bash {
    use std::io::Write;
    use std::process::Command;
    use tempfile::NamedTempFile;

    fn bin_path() -> &'static str {
        env!("CARGO_BIN_EXE_runex")
    }

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

    /// Returns false if bash is not found or is too old (< 4.0).
    /// macOS ships bash 3.2 (GPLv2 constraint) which does not support
    /// process substitution in non-interactive mode. Require bash 4+.
    fn bash_available() -> bool {
        let Ok(path) = which::which("bash") else { return false };
        let out = Command::new(path)
            .args(["--norc", "--noprofile", "-c", "echo $BASH_VERSION"])
            .output();
        let Ok(out) = out else { return false };
        let ver = String::from_utf8_lossy(&out.stdout);
        // BASH_VERSION looks like "5.2.37(1)-release"; major version must be >= 4
        ver.trim()
            .split('.')
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .map(|major| major >= 4)
            .unwrap_or(false)
    }

    /// Run a snippet inside a non-interactive bash that has sourced the runex
    /// integration script. Returns stdout trimmed.
    fn run_bash(config: &NamedTempFile, snippet: &str) -> String {
        let bin = bin_path();
        let script = format!(
            "source <({bin} export bash --bin {bin})\n{snippet}"
        );
        let output = Command::new("bash")
            .args(["--norc", "--noprofile", "-c", &script])
            .env("RUNEX_CONFIG", config.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "bash helper should succeed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn test_source_no_error() {
        if !bash_available() { return; }
        let config = write_config();
        // run_bash itself asserts exit 0
        run_bash(&config, "true");
    }

    #[test]
    fn test_expand_helper() {
        if !bash_available() { return; }
        let config = write_config();
        let out = run_bash(
            &config,
            r#"READLINE_LINE='gcm'; READLINE_POINT=3; __runex_expand; printf '<%s>\n' "$READLINE_LINE""#,
        );
        assert_eq!(out, "<echo EXPANDED >");
    }

    #[test]
    fn test_midline_space_is_plain_insert() {
        if !bash_available() { return; }
        let config = write_config();
        let out = run_bash(
            &config,
            r#"READLINE_LINE='gcm tail'; READLINE_POINT=1; __runex_expand; printf '<%s>\n' "$READLINE_LINE""#,
        );
        assert_eq!(out, "<g cm tail>");
    }

    #[test]
    fn test_expand_after_separator() {
        if !bash_available() { return; }
        let config = write_config();
        let out = run_bash(
            &config,
            r#"READLINE_LINE='echo foo && gcm'; READLINE_POINT=15; __runex_expand; printf '<%s>\n' "$READLINE_LINE""#,
        );
        assert_eq!(out, "<echo foo && echo EXPANDED >");
    }

    #[test]
    fn test_argument_position_does_not_expand() {
        if !bash_available() { return; }
        let config = write_config();
        let out = run_bash(
            &config,
            r#"READLINE_LINE='echo gcm'; READLINE_POINT=8; __runex_expand; printf '<%s>\n' "$READLINE_LINE""#,
        );
        assert_eq!(out, "<echo gcm >");
    }

    #[test]
    fn test_sudo_position_expands() {
        if !bash_available() { return; }
        let config = write_config();
        let out = run_bash(
            &config,
            r#"READLINE_LINE='sudo gcm'; READLINE_POINT=8; __runex_expand; printf '<%s>\n' "$READLINE_LINE""#,
        );
        assert_eq!(out, "<sudo echo EXPANDED >");
    }

    #[test]
    fn test_no_expand_unknown_token() {
        if !bash_available() { return; }
        let config = write_config();
        let out = run_bash(
            &config,
            r#"READLINE_LINE='xyz'; READLINE_POINT=3; __runex_expand; printf '<%s>\n' "$READLINE_LINE""#,
        );
        assert_eq!(out, "<xyz >");
    }

    #[test]
    fn test_option_like_token_stays_intact() {
        if !bash_available() { return; }
        let config = write_config();
        let out = run_bash(
            &config,
            r#"READLINE_LINE='cargo install --path'; READLINE_POINT=20; __runex_expand; printf '<%s>\n' "$READLINE_LINE""#,
        );
        assert_eq!(out, "<cargo install --path >");
    }
}
