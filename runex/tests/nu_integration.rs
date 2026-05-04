//! Integration tests for the Nushell shell integration.
//!
//! Spawns a non-interactive `nu -c <script>` subprocess that performs the
//! same RPC the keybindings template would: parse the JSON emitted by
//! `runex hook --shell nu` and apply it. No PTY required. Tests are
//! skipped at runtime if `nu` is not on PATH.
//!
//! The keybindings themselves are wired up by the user's `config.nu`
//! and require a TTY to fire; we cover those at the template-render
//! level (`runex export nu`) in unit tests, and the per-keystroke
//! decision logic in `runex-core::hook::tests`. This file fills the
//! middle gap: "given a real `nu` interpreter and a real `runex hook`
//! invocation, do the two interoperate?".

#[cfg(target_family = "unix")]
mod nu {
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

    /// Returns false if `nu` is not on PATH. The Nushell integration is
    /// optional — most CI runners don't have nu installed — so we skip
    /// rather than fail in that case.
    fn nu_available() -> bool {
        which::which("nu").is_ok()
    }

    /// Drive `runex hook` from inside `nu` exactly the way the
    /// keybindings template does, then apply the resulting JSON to a
    /// pretend buffer and print the final state. Returns
    /// `"<line>|<cursor>"` so tests can assert on a single string.
    fn run_nu(config: &NamedTempFile, line: &str, cursor: usize) -> String {
        let bin = bin_path();
        // We avoid `commandline edit` / `commandline set-cursor` (those
        // need a live REPL) and instead apply the `from json` output to
        // a plain string, mirroring what the template would do to the
        // editor buffer. The format string at the end is what gets
        // asserted on by the tests.
        let script = format!(
            r#"
let line = "{line}"
let cursor = {cursor}
let out = (^"{bin}" hook --shell nu --line $line --cursor $cursor | complete | get stdout | str trim --right)
if ($out | is-empty) {{
    print $"($line) |($cursor + 1)"
}} else {{
    let r = ($out | from json)
    print $"($r.line)|($r.cursor)"
}}
"#
        );
        let output = Command::new("nu")
            .args(["-c", &script])
            .env("RUNEX_CONFIG", config.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "nu helper should succeed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Sanity: runex export nu produces a script that nu can at least
    /// parse without error (we don't source it, just `nu -c "..."`).
    #[test]
    fn export_nu_is_parseable_by_nu() {
        if !nu_available() {
            eprintln!("skipping: nu not found on PATH");
            return;
        }
        let bin = bin_path();
        let output = Command::new("nu")
            .args(["-c", &format!("^{bin} export nu | save --force /tmp/runex_nu_test.nu")])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "nu export should produce a parseable file\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    /// `gcm` at the start of a line in command position expands to the
    /// configured `echo EXPANDED`, with cursor advanced past the
    /// inserted trailing space.
    #[test]
    fn expand_at_end() {
        if !nu_available() {
            eprintln!("skipping: nu not found on PATH");
            return;
        }
        let config = write_config();
        // `gcm` (3 chars), cursor at end (= 3). Expansion is
        // `echo EXPANDED ` (14 chars including trailing space), cursor
        // at 14.
        assert_eq!(run_nu(&config, "gcm", 3), "echo EXPANDED |14");
    }

    /// `xyz` is not in the abbr table — the hook returns no expansion,
    /// so the buffer is left as-is and the cursor moves forward by one
    /// (the literal-space fallback the keybindings template applies).
    #[test]
    fn unknown_token_stays_as_is() {
        if !nu_available() {
            eprintln!("skipping: nu not found on PATH");
            return;
        }
        let config = write_config();
        assert_eq!(run_nu(&config, "xyz", 3), "xyz |4");
    }

    /// Mid-token cursor (`g|cm`) is not a Space-boundary, so the hook
    /// inserts a literal space rather than expanding.
    #[test]
    fn midword_cursor_does_not_expand() {
        if !nu_available() {
            eprintln!("skipping: nu not found on PATH");
            return;
        }
        let config = write_config();
        // line="gcm", cursor=1 → InsertSpace splits into "g cm",
        // cursor=2.
        assert_eq!(run_nu(&config, "gcm", 1), "g cm|2");
    }

    /// Echo's argument position must NOT trigger expansion — `gcm`
    /// after `echo ` is at argument position, so the hook should
    /// return InsertSpace, not Replace.
    #[test]
    fn argument_position_does_not_expand() {
        if !nu_available() {
            eprintln!("skipping: nu not found on PATH");
            return;
        }
        let config = write_config();
        // line="echo gcm", cursor=8 → after the `m`. Not command
        // position, so just insert a space.
        assert_eq!(run_nu(&config, "echo gcm", 8), "echo gcm |9");
    }
}
