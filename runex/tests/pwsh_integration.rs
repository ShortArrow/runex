mod pwsh {
    use std::io::Write;
    use std::process::Command;
    use base64::Engine;
    use tempfile::NamedTempFile;

    fn pwsh_available() -> bool {
        which::which("pwsh").is_ok()
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

    fn run_helper(config: &NamedTempFile, line: &str, cursor: usize) -> String {
        // The new hook-based bootstrap puts all buffer logic in the Rust
        // binary; pwsh just reads buffer state and evals the output. We
        // mirror that here by calling `runex hook` directly and formatting
        // the result the same way the legacy `__runex_expand_space` helper
        // used to report it ("line|cursor").
        let script = r#"
$line = [System.Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:RUNEX_LINE_B64))
$cursor = [int]$env:RUNEX_CURSOR
$out = & $env:RUNEX_BIN hook --shell pwsh --line $line --cursor $cursor 2>$null
$__RUNEX_LINE = $null
$__RUNEX_CURSOR = $null
if ($out) { Invoke-Expression ($out -join "`n") }
if ($null -ne $__RUNEX_LINE -and $null -ne $__RUNEX_CURSOR) {
    Write-Output "$__RUNEX_LINE|$__RUNEX_CURSOR"
} else {
    # Fallback: insert a space at the cursor, mirroring the bootstrap.
    $left  = $line.Substring(0, $cursor)
    $right = $line.Substring($cursor)
    Write-Output "$left $right|$($cursor + 1)"
}
"#;

        let output = Command::new("pwsh")
            .args(["-NoLogo", "-NoProfile", "-Command", script])
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
            "pwsh helper should succeed\nstdout:\n{}\nstderr:\n{}",
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
}
