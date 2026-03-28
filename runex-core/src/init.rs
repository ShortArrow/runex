use std::path::PathBuf;

use crate::shell::Shell;

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
        Shell::Bash => format!("eval \"$({bin} export bash)\""),
        Shell::Zsh => format!("eval \"$({bin} export zsh)\""),
        Shell::Pwsh => format!("Invoke-Expression (& {bin} export pwsh | Out-String)"),
        Shell::Nu => {
            // Use the XDG config dir at runtime if we can; fall back to ~/.config.
            let cfg_dir = dirs::config_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "~/.config".to_string());
            format!(
                "{bin} export nu | save --force {cfg_dir}/runex/runex.nu\nsource {cfg_dir}/runex/runex.nu"
            )
        }
        Shell::Clink => format!("-- add '{bin} export clink' output to your clink scripts directory"),
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
            // Prefer XDG config dir over hard-coded ~/.config.
            let cfg = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
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
            r#"eval "$(runex export bash)""#
        );
    }

    #[test]
    fn integration_line_pwsh() {
        let line = integration_line(Shell::Pwsh, "runex");
        assert!(line.contains("Invoke-Expression"));
        assert!(line.contains("runex export pwsh"));
    }

    #[test]
    fn rc_file_for_bash_ends_with_bashrc() {
        if let Some(path) = rc_file_for(Shell::Bash) {
            assert!(path.to_str().unwrap().ends_with(".bashrc"));
        }
    }
}
