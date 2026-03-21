#[cfg(target_os = "windows")]
mod pwsh {
    use std::io::Write;
    use std::process::Command;
    use tempfile::NamedTempFile;

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
        let script = r#"
Invoke-Expression ((& $env:RUNEX_BIN export pwsh --bin $env:RUNEX_BIN) -join "`n")
$state = __runex_expand_space $env:RUNEX_LINE ([int]$env:RUNEX_CURSOR)
Write-Output "$($state.Line)|$($state.Cursor)"
"#;

        let output = Command::new("pwsh")
            .args(["-NoLogo", "-NoProfile", "-Command", script])
            .env("RUNEX_BIN", bin_path())
            .env("RUNEX_CONFIG", config.path())
            .env("RUNEX_LINE", line)
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

    #[test]
    fn expand_at_end() {
        let config = write_config();
        assert_eq!(run_helper(&config, "gcm", 3), "echo EXPANDED |14");
    }

    #[test]
    fn midline_space_is_plain_insert() {
        let config = write_config();
        assert_eq!(run_helper(&config, "gcm tail", 1), "g cm tail|2");
    }

    #[test]
    fn expands_token_before_cursor() {
        let config = write_config();
        assert_eq!(
            run_helper(&config, "echo gcm", 8),
            "echo gcm |9"
        );
    }

    #[test]
    fn expands_after_separator() {
        let config = write_config();
        assert_eq!(
            run_helper(&config, "echo foo && gcm", 15),
            "echo foo && echo EXPANDED |26"
        );
    }

    #[test]
    fn expands_after_sudo() {
        let config = write_config();
        assert_eq!(run_helper(&config, "sudo gcm", 8), "sudo echo EXPANDED |19");
    }

    #[test]
    fn unknown_token_stays_as_is() {
        let config = write_config();
        assert_eq!(run_helper(&config, "xyz", 3), "xyz |4");
    }

    #[test]
    fn option_like_token_stays_intact() {
        let config = write_config();
        assert_eq!(
            run_helper(&config, "cargo install --path", 20),
            "cargo install --path |21"
        );
    }

    #[test]
    fn known_token_in_argument_position_does_not_expand() {
        let config = write_config();
        assert_eq!(run_helper(&config, "echo gcm", 8), "echo gcm |9");
    }
}
