mod pwsh {
    use std::io::Write;
    use std::process::Command;
    use base64::Engine;
    use tempfile::NamedTempFile;

    fn pwsh_available() -> bool {
        which::which("pwsh").is_ok()
    }

    #[cfg(windows)]
    fn windows_powershell_available() -> bool {
        which::which("powershell").is_ok()
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

    fn bin_path() -> &'static str {
        env!("CARGO_BIN_EXE_runex")
    }

    /// Drives `runex hook` the way the pwsh bootstrap does and reports
    /// the result as "line|cursor".
    ///
    /// The invocation is not written here: the script exports the real
    /// bootstrap (`runex export pwsh`), lifts the `$hookArgs = @(...)`
    /// line out of it and evaluates that with `$line` / `$cursor` in
    /// scope, so the arguments handed to `runex hook` are exactly the
    /// template's — a change to the template's argument form (such as
    /// the joined `--line=$line` that keeps PowerShell from rewriting a
    /// leading `~`) is exercised here rather than mirrored by hand.
    ///
    /// The hook producing no usable eval text is reported as
    /// `NOOUT|`, never patched over: the bootstrap's own fallback
    /// (insert a literal space) would make a broken hook look right.
    fn run_helper(config: &NamedTempFile, line: &str, cursor: usize) -> String {
        run_helper_in_host("pwsh", config, line, cursor)
    }

    /// Same as `run_helper`, against a named PowerShell host executable.
    ///
    /// `powershell` (Windows PowerShell 5.1) and `pwsh` (PowerShell 7)
    /// parse native-command arguments differently, so the transport the
    /// template chooses has to be exercised in both.
    fn run_helper_in_host(
        host: &str,
        config: &NamedTempFile,
        line: &str,
        cursor: usize,
    ) -> String {
        let script = r#"
$line = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:RUNEX_LINE_B64))
$cursor = [int]$env:RUNEX_CURSOR
$template = & $env:RUNEX_BIN export pwsh --bin $env:RUNEX_BIN
$argsLine = @($template | Where-Object { $_ -match '^\s*\$hookArgs = @\(' })
if ($argsLine.Count -ne 1) { Write-Output "TEMPLATE-HAS-$($argsLine.Count)-HOOKARGS-LINES|"; exit 0 }
Invoke-Expression $argsLine[0]
$out = & $env:RUNEX_BIN @hookArgs 2>$null
$__RUNEX_LINE = $null
$__RUNEX_CURSOR = $null
if ($out) { Invoke-Expression ($out -join "`n") }
if ($null -ne $__RUNEX_LINE -and $null -ne $__RUNEX_CURSOR) {
    Write-Output "$__RUNEX_LINE|$__RUNEX_CURSOR"
} else {
    Write-Output "NOOUT|"
}
"#;

        let output = Command::new(host)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .env("RUNEX_BIN", bin_path())
            .env("RUNEX_CONFIG", config.path())
            .env(
                "RUNEX_LINE_B64",
                base64::engine::general_purpose::STANDARD.encode(line),
            )
            .env("RUNEX_CURSOR", cursor.to_string())
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "{host} helper should succeed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    // Invoke-Expression format regression tests were removed when the pwsh
    // bootstrap stopped relying on Invoke-Expression to materialise inline
    // function definitions. The new bootstrap is a small script that defines
    // its own functions and calls `runex hook` at keypress time — no more
    // "function body vanishes when array is space-joined" hazard.

    #[test]
    fn expand_at_end() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(run_helper(&config, "gcm", 3), "echo EXPANDED |14");
    }

    #[test]
    fn midline_space_is_plain_insert() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(run_helper(&config, "gcm tail", 1), "g cm tail|2");
    }

    #[test]
    fn expands_token_before_cursor() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(
            run_helper(&config, "echo gcm", 8),
            "echo gcm |9"
        );
    }

    #[test]
    fn expands_after_separator() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(
            run_helper(&config, "echo foo && gcm", 15),
            "echo foo && echo EXPANDED |26"
        );
    }

    #[test]
    fn expands_after_sudo() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(run_helper(&config, "sudo gcm", 8), "sudo echo EXPANDED |19");
    }

    #[test]
    fn unknown_token_stays_as_is() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(run_helper(&config, "xyz", 3), "xyz |4");
    }

    #[test]
    fn option_like_token_stays_intact() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(
            run_helper(&config, "cargo install --path", 20),
            "cargo install --path |21"
        );
    }

    #[test]
    fn known_token_in_argument_position_does_not_expand() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(run_helper(&config, "echo gcm", 8), "echo gcm |9");
    }

    #[test]
    fn path_argument_with_backslashes_stays_intact() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(
            run_helper(&config, r"cd .\ShortArrow.github.io\", 26),
            r"cd .\ShortArrow.github.io\ |27"
        );
    }

    /// PowerShell rewrites a leading `~` in a native-command argument to
    /// `$HOME` even when the value arrives through a variable, so the
    /// buffer runex receives no longer matches the cursor the shell sent
    /// (issue #18). The space must land at the end of the untouched line.
    #[test]
    fn tilde_prefixed_line_is_passed_to_runex_verbatim() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(
            run_helper(&config, "~/.local/bin/claude.exe", 23),
            "~/.local/bin/claude.exe |24"
        );
    }

    /// A pasted multi-line path (issue #21) reaches the hook with an
    /// embedded newline. The returned line must still contain it, so the
    /// buffer keeps its text and the cursor stays on the trigger space.
    #[test]
    fn multiline_buffer_round_trips_with_newline_intact() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(
            run_helper(&config, "scp -r \"\\\\srv\\帳票（レポート\n）\\x\"", 26),
            "scp -r \"\\\\srv\\帳票（レポート\n）\\x\" |27"
        );
    }

    /// A buffer containing `"` reaches the hook intact under PowerShell 7.
    /// Characterisation: this already held before the hex transport and
    /// must keep holding after it, so the fix for 5.1 (issue #35) cannot
    /// be paid for with a regression on 7.
    #[test]
    fn double_quoted_argument_round_trips() {
        if !pwsh_available() { return; }
        let config = write_config();
        assert_eq!(
            run_helper(&config, "echo \"a b\" c", 12),
            "echo \"a b\" c |13"
        );
    }

    /// Windows PowerShell 5.1 re-splits a native-command argument that
    /// contains `"`, so `--line=echo "a b" c` arrived at runex as two
    /// arguments and clap rejected the second (issue #35). The buffer
    /// must round-trip there exactly as it does under PowerShell 7.
    #[cfg(windows)]
    #[test]
    fn double_quoted_argument_round_trips_under_windows_powershell_51() {
        if !windows_powershell_available() { return; }
        let config = write_config();
        assert_eq!(
            run_helper_in_host("powershell", &config, "echo \"a b\" c", 12),
            "echo \"a b\" c |13"
        );
    }
}
