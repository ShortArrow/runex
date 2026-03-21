use clap::{Parser, Subcommand};
use runex_core::config::{default_config_path, load_config};
use runex_core::doctor::{self, Check, CheckStatus, DiagResult};
use runex_core::expand;
use runex_core::model::{Abbr, Config, ExpandResult};
use runex_core::shell::Shell;
use std::process::Command;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";

#[derive(Parser)]
#[command(name = "runex", about = "Rune-to-cast expansion engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Expand a token to its cast
    Expand {
        #[arg(long)]
        token: String,
    },
    /// List all abbreviations
    List,
    /// Check environment health
    Doctor,
    /// Export shell integration script
    Export {
        /// Target shell: bash, pwsh, clink, nu
        shell: String,
        /// Binary name used in the generated script
        #[arg(long, default_value = "runex")]
        bin: String,
    },
}

fn bash_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r#"'\''"#))
}

fn format_check_tag(status: &CheckStatus) -> String {
    match status {
        CheckStatus::Ok => format!("[{ANSI_GREEN}OK{ANSI_RESET}]"),
        CheckStatus::Warn => format!("[{ANSI_YELLOW}WARN{ANSI_RESET}]"),
        CheckStatus::Error => format!("[{ANSI_RED}ERROR{ANSI_RESET}]"),
    }
}

fn format_check_line(check: &Check) -> String {
    format!(
        "{:>8}  {}: {}",
        format_check_tag(&check.status),
        check.name,
        check.detail
    )
}

fn run_pwsh_alias_lookup(token: &str) -> Option<String> {
    if which::which("pwsh").is_err() {
        return None;
    }

    let output = Command::new("pwsh")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-Command",
            &format!(
                "Get-Alias -Name {} -ErrorAction Stop | Select-Object -ExpandProperty Definition",
                token
            ),
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let definition = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!definition.is_empty()).then_some(definition)
}

fn check_pwsh_alias_with<F>(token: &str, lookup: F) -> Option<Check>
where
    F: Fn(&str) -> Option<String>,
{
    let definition = lookup(token)?;
    Some(Check {
        name: format!("shell:pwsh:key:{token}"),
        status: CheckStatus::Warn,
        detail: format!("conflicts with existing alias '{token}' -> {definition}"),
    })
}

fn run_bash_alias_lookup(token: &str) -> Option<String> {
    if cfg!(windows) {
        return None;
    }

    if which::which("bash").is_err() {
        return None;
    }

    let output = Command::new("bash")
        .args([
            "-ic",
            &format!("alias {}", bash_single_quote(token)),
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let detail = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!detail.is_empty()).then_some(detail)
}

fn check_bash_alias_with<F>(token: &str, lookup: F) -> Option<Check>
where
    F: Fn(&str) -> Option<String>,
{
    let detail = lookup(token)?;
    Some(Check {
        name: format!("shell:bash:key:{token}"),
        status: CheckStatus::Warn,
        detail: format!("conflicts with existing alias {detail}"),
    })
}

fn collect_shell_alias_conflicts_with<FPwsh, FBash>(
    abbrs: &[Abbr],
    pwsh_lookup: FPwsh,
    bash_lookup: FBash,
) -> Vec<Check>
where
    FPwsh: Fn(&str) -> Option<String> + Copy,
    FBash: Fn(&str) -> Option<String> + Copy,
{
    let mut checks = Vec::new();
    for abbr in abbrs {
        if let Some(check) = check_pwsh_alias_with(&abbr.key, pwsh_lookup) {
            checks.push(check);
        }
        if let Some(check) = check_bash_alias_with(&abbr.key, bash_lookup) {
            checks.push(check);
        }
    }
    checks
}

fn add_shell_alias_conflicts(result: &mut DiagResult, config: Option<&Config>) {
    let Some(config) = config else {
        return;
    };

    result
        .checks
        .extend(collect_shell_alias_conflicts_with(
            &config.abbr,
            run_pwsh_alias_lookup,
            run_bash_alias_lookup,
        ));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Expand { token } => {
            let config_path = default_config_path()?;
            let config = load_config(&config_path)?;
            let result = expand::expand(&config, &token, |cmd| which::which(cmd).is_ok());
            match result {
                ExpandResult::Expanded(s) => print!("{s}"),
                ExpandResult::PassThrough(s) => print!("{s}"),
            }
        }
        Commands::List => {
            let config_path = default_config_path()?;
            let config = load_config(&config_path)?;
            for (key, exp) in expand::list(&config) {
                println!("{key}\t{exp}");
            }
        }
        Commands::Export { shell, bin } => {
            let s: Shell = shell.parse().map_err(|e: runex_core::shell::ShellParseError| {
                Box::<dyn std::error::Error>::from(e.to_string())
            })?;
            let config_path = default_config_path().ok();
            let config = config_path
                .as_ref()
                .and_then(|path| load_config(path).ok());
            print!("{}", runex_core::shell::export_script(s, &bin, config.as_ref()));
        }
        Commands::Doctor => {
            let config_path = default_config_path().unwrap_or_default();
            let config = load_config(&config_path).ok();
            let mut result =
                doctor::diagnose(&config_path, config.as_ref(), |cmd| which::which(cmd).is_ok());
            add_shell_alias_conflicts(&mut result, config.as_ref());

            for check in &result.checks {
                println!("{}", format_check_line(check));
            }

            if !result.is_healthy() {
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_abbr(key: &str) -> Abbr {
        Abbr {
            key: key.into(),
            expand: format!("expand-{key}"),
            when_command_exists: None,
        }
    }

    #[test]
    fn format_check_line_colors_only_tag_text() {
        let check = Check {
            name: "config_file".into(),
            status: CheckStatus::Warn,
            detail: "detail".into(),
        };

        let line = format_check_line(&check);
        assert!(line.starts_with(&format!("[{ANSI_YELLOW}WARN{ANSI_RESET}]")));
        assert!(line.contains("config_file: detail"));
    }

    #[test]
    fn collect_shell_alias_conflicts_reports_pwsh_and_bash() {
        let checks = collect_shell_alias_conflicts_with(
            &[test_abbr("gcm"), test_abbr("nv")],
            |token| (token == "gcm").then_some("Get-Command".to_string()),
            |token| (token == "nv").then_some("alias nv='nvim'".to_string()),
        );

        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].name, "shell:pwsh:key:gcm");
        assert!(checks[0].detail.contains("Get-Command"));
        assert_eq!(checks[1].name, "shell:bash:key:nv");
        assert!(checks[1].detail.contains("alias nv='nvim'"));
    }

    #[test]
    fn collect_shell_alias_conflicts_skips_missing_aliases() {
        let checks = collect_shell_alias_conflicts_with(&[test_abbr("gcm")], |_| None, |_| None);
        assert!(checks.is_empty());
    }
}
