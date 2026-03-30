use std::path::PathBuf;

use crate::config::xdg_config_home;
use crate::shell::{bash_quote_string, nu_quote_string, pwsh_quote_string, Shell};

/// Quote a filesystem path for embedding in a Nu shell string literal.
/// Uses Nu double-quoted string syntax: escapes `\` and `"`.
/// Unlike `nu_quote_string`, this does NOT add the `^` external-command prefix.
fn nu_quote_path(path: &str) -> String {
    let mut out = String::from("\"");
    for ch in path.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Marker comment written into rc files to enable idempotent init.
pub const RUNEX_INIT_MARKER: &str = "# runex-init";

/// Minimal config template written by `runex init`.
pub fn default_config_content() -> &'static str {
    r#"version = 1

# Add your abbreviations below.
# [[abbr]]
# key = "gcm"
# expand = "git commit -m"
"#
}

/// The single line appended to the shell rc file.
pub fn integration_line(shell: Shell, bin: &str) -> String {
    match shell {
        Shell::Bash => format!("eval \"$({} export bash)\"", bash_quote_string(bin)),
        Shell::Zsh => format!("eval \"$({} export zsh)\"", bash_quote_string(bin)),
        Shell::Pwsh => format!(
            "Invoke-Expression (& {} export pwsh | Out-String)",
            pwsh_quote_string(bin)
        ),
        Shell::Nu => {
            let cfg_dir = xdg_config_home()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "~/.config".to_string());
            // Nu requires ^"..." syntax to invoke an external command by quoted name.
            let nu_bin = nu_quote_string(bin);
            // Paths must be quoted so spaces and backslashes don't break tokenization.
            let nu_path = nu_quote_path(&format!("{cfg_dir}/runex/runex.nu"));
            format!(
                "{nu_bin} export nu | save --force {nu_path}\nsource {nu_path}"
            )
        }
        Shell::Clink => format!(
            "-- add '{} export clink' output to your clink scripts directory",
            bin
        ),
    }
}

/// The rc file path for a given shell (best-effort; may not exist yet).
pub fn rc_file_for(shell: Shell) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    match shell {
        Shell::Bash => Some(home.join(".bashrc")),
        Shell::Zsh => Some(home.join(".zshrc")),
        Shell::Pwsh => {
            // $PROFILE is a runtime variable; use the conventional Windows path.
            let base = if cfg!(windows) {
                home.join("Documents").join("PowerShell")
            } else {
                home.join(".config").join("powershell")
            };
            Some(base.join("Microsoft.PowerShell_profile.ps1"))
        }
        Shell::Nu => {
            let cfg = xdg_config_home().unwrap_or_else(|| home.join(".config"));
            Some(cfg.join("nushell").join("env.nu"))
        }
        Shell::Clink => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_content_has_version() {
        assert!(default_config_content().contains("version = 1"));
    }

    #[test]
    fn integration_line_bash() {
        assert_eq!(
            integration_line(Shell::Bash, "runex"),
            r#"eval "$('runex' export bash)""#
        );
    }

    #[test]
    fn integration_line_pwsh() {
        let line = integration_line(Shell::Pwsh, "runex");
        assert!(line.contains("Invoke-Expression"));
        assert!(line.contains("'runex' export pwsh"));
    }

    #[test]
    fn integration_line_bash_escapes_single_quote_in_bin() {
        // bin = "run'ex" must not break out of the eval context
        let line = integration_line(Shell::Bash, "run'ex");
        assert!(!line.contains("run'ex"), "unescaped quote in bash line: {line}");
        // must contain the properly escaped form
        assert!(line.contains(r"run'\''ex"), "expected bash-escaped form: {line}");
    }

    #[test]
    fn integration_line_zsh_escapes_single_quote_in_bin() {
        let line = integration_line(Shell::Zsh, "run'ex");
        assert!(!line.contains("run'ex"), "unescaped quote in zsh line: {line}");
        assert!(line.contains(r"run'\''ex"), "expected zsh-escaped form: {line}");
    }

    #[test]
    fn integration_line_pwsh_escapes_single_quote_in_bin() {
        let line = integration_line(Shell::Pwsh, "run'ex");
        assert!(!line.contains("run'ex"), "unescaped quote in pwsh line: {line}");
        // PowerShell doubles single quotes inside single-quoted strings
        assert!(line.contains("run''ex"), "expected pwsh-escaped form: {line}");
    }

    #[test]
    fn integration_line_bash_semicolon_does_not_inject() {
        // bin = "app; echo PWNED" — semicolon must be enclosed in single quotes
        let line = integration_line(Shell::Bash, "app; echo PWNED");
        // The entire bin value must appear inside single quotes, neutralising ';'
        assert!(
            line.contains("'app; echo PWNED'"),
            "bin must be single-quoted in bash line: {line}"
        );
    }

    #[test]
    fn integration_line_pwsh_semicolon_does_not_inject() {
        // bin = "app; Write-Host PWNED" — semicolon must be enclosed in single quotes
        let line = integration_line(Shell::Pwsh, "app; Write-Host PWNED");
        assert!(
            line.contains("'app; Write-Host PWNED'"),
            "bin must be single-quoted in pwsh line: {line}"
        );
    }

    #[test]
    fn integration_line_nu_uses_caret_external_command_syntax() {
        // Nu: quoting without ^ makes a string, not a command — must use ^"runex"
        let line = integration_line(Shell::Nu, "runex");
        assert!(
            line.contains("^\"runex\""),
            "nu integration line must use ^\"...\" syntax: {line}"
        );
    }

    #[test]
    fn integration_line_nu_escapes_special_chars_in_bin() {
        let line = integration_line(Shell::Nu, "my\"app");
        assert!(line.contains("^\"my\\\"app\""), "nu: special chars must be escaped: {line}");
    }

    #[test]
    fn integration_line_nu_quotes_cfg_dir_with_spaces() {
        // nu_quote_path must wrap the path in double quotes so Nu doesn't tokenize on spaces.
        let quoted = nu_quote_path("/home/my user/.config");
        assert_eq!(quoted, "\"/home/my user/.config\"");
        assert!(!quoted.starts_with('/'), "path must be quoted, not raw");
    }

    #[test]
    fn integration_line_nu_quotes_cfg_dir_with_backslash() {
        // Windows paths: backslashes must be escaped inside Nu double-quoted strings.
        let quoted = nu_quote_path(r"C:\Users\my user\AppData");
        assert_eq!(quoted, r#""C:\\Users\\my user\\AppData""#);
    }

    #[test]
    fn integration_line_nu_save_path_is_quoted() {
        // The generated Nu integration line must quote the runex.nu path.
        let line = integration_line(Shell::Nu, "runex");
        // save and source must use quoted paths (starting with ")
        for fragment in ["save --force \"", "source \""] {
            assert!(
                line.contains(fragment),
                "nu line must contain `{fragment}`: {line}"
            );
        }
    }

    #[test]
    fn rc_file_for_bash_ends_with_bashrc() {
        if let Some(path) = rc_file_for(Shell::Bash) {
            assert!(path.to_str().unwrap().ends_with(".bashrc"));
        }
    }
}
