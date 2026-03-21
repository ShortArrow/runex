use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Pwsh,
    Clink,
    Nu,
}

impl FromStr for Shell {
    type Err = ShellParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "bash" => Ok(Shell::Bash),
            "pwsh" => Ok(Shell::Pwsh),
            "clink" => Ok(Shell::Clink),
            "nu" => Ok(Shell::Nu),
            _ => Err(ShellParseError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellParseError(pub String);

impl fmt::Display for ShellParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown shell '{}' (expected: bash, pwsh, clink, nu)",
            self.0
        )
    }
}

impl std::error::Error for ShellParseError {}

/// Generate a shell integration script.
///
/// `{BIN}` placeholders in the template are replaced with `bin`.
pub fn export_script(shell: Shell, bin: &str) -> String {
    let template = match shell {
        Shell::Bash => include_str!("templates/bash.sh"),
        Shell::Pwsh => include_str!("templates/pwsh.ps1"),
        Shell::Clink => include_str!("templates/clink.lua"),
        Shell::Nu => include_str!("templates/nu.nu"),
    };
    template.replace("\r\n", "\n").replace("{BIN}", bin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bash() {
        assert_eq!(Shell::from_str("bash").unwrap(), Shell::Bash);
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(Shell::from_str("PWSH").unwrap(), Shell::Pwsh);
        assert_eq!(Shell::from_str("Clink").unwrap(), Shell::Clink);
        assert_eq!(Shell::from_str("Nu").unwrap(), Shell::Nu);
    }

    #[test]
    fn parse_unknown_errors() {
        let err = Shell::from_str("fish").unwrap_err();
        assert_eq!(err.0, "fish");
    }

    #[test]
    fn export_script_contains_bin() {
        for shell in [Shell::Bash, Shell::Pwsh, Shell::Clink, Shell::Nu] {
            let script = export_script(shell, "my-runex");
            assert!(
                script.contains("my-runex"),
                "{shell:?} script must contain the bin name"
            );
        }
    }

    #[test]
    fn bash_script_has_bind() {
        let s = export_script(Shell::Bash, "runex");
        assert!(s.contains("bind"), "bash script must use bind");
        assert!(s.contains("READLINE_LINE"), "bash script must use READLINE_LINE");
        assert!(s.contains("READLINE_POINT"), "bash script must inspect the cursor");
    }

    #[test]
    fn pwsh_script_has_psreadline() {
        let s = export_script(Shell::Pwsh, "runex");
        assert!(s.contains("Set-PSReadLineKeyHandler"), "pwsh script must use PSReadLine");
        assert!(s.contains("$cursor -lt $line.Length"), "pwsh script must guard mid-line insertion");
    }

    #[test]
    fn clink_script_has_clink() {
        let s = export_script(Shell::Clink, "runex");
        assert!(s.contains("clink"), "clink script must reference clink");
    }

    #[test]
    fn nu_script_has_keybindings() {
        let s = export_script(Shell::Nu, "runex");
        assert!(s.contains("keybindings"), "nu script must reference keybindings");
        assert!(s.contains("commandline get-cursor"), "nu script must inspect the cursor");
    }
}
