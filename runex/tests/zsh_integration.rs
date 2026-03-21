#[cfg(target_family = "unix")]
mod zsh {
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

    fn run_helper(config: &NamedTempFile, left: &str, right: &str) -> String {
        let script = r#"
eval "$($RUNEX_BIN export zsh --bin $RUNEX_BIN)"
LBUFFER="$RUNEX_LEFT"
RBUFFER="$RUNEX_RIGHT"
__runex_expand_buffer
printf '%s|%s\n' "$LBUFFER" "$RBUFFER"
"#;

        let output = Command::new("zsh")
            .args(["-f", "-c", script])
            .env("RUNEX_BIN", bin_path())
            .env("RUNEX_CONFIG", config.path())
            .env("RUNEX_LEFT", left)
            .env("RUNEX_RIGHT", right)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "zsh helper should succeed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn expand_at_end() {
        let config = write_config();
        assert_eq!(run_helper(&config, "gcm", ""), "echo EXPANDED |");
    }

    #[test]
    fn midline_space_is_plain_insert() {
        let config = write_config();
        assert_eq!(run_helper(&config, "g", "cm tail"), "g |cm tail");
    }

    #[test]
    fn expands_after_separator() {
        let config = write_config();
        assert_eq!(
            run_helper(&config, "echo foo && gcm", ""),
            "echo foo && echo EXPANDED |"
        );
    }

    #[test]
    fn expands_after_sudo() {
        let config = write_config();
        assert_eq!(run_helper(&config, "sudo gcm", ""), "sudo echo EXPANDED |");
    }

    #[test]
    fn argument_position_does_not_expand() {
        let config = write_config();
        assert_eq!(run_helper(&config, "echo gcm", ""), "echo gcm |");
    }

    #[test]
    fn unknown_token_stays_as_is() {
        let config = write_config();
        assert_eq!(run_helper(&config, "xyz", ""), "xyz |");
    }

    #[test]
    fn option_like_token_stays_intact() {
        let config = write_config();
        assert_eq!(
            run_helper(&config, "cargo install --path", ""),
            "cargo install --path |"
        );
    }
}
