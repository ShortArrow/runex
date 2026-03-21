use clap::{Parser, Subcommand};
use runex_core::config::{default_config_path, load_config};
use runex_core::doctor::{self, Check, CheckStatus, DiagResult};
use runex_core::expand;
use runex_core::model::{Config, ExpandResult};
use runex_core::shell::Shell;
use std::process::Command;

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

fn check_pwsh_alias(token: &str) -> Option<Check> {
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
    if definition.is_empty() {
        return None;
    }

    Some(Check {
        name: format!("shell:pwsh:key:{token}"),
        status: CheckStatus::Warn,
        detail: format!("conflicts with existing alias '{token}' -> {definition}"),
    })
}

fn check_bash_alias(token: &str) -> Option<Check> {
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
    if detail.is_empty() {
        return None;
    }

    Some(Check {
        name: format!("shell:bash:key:{token}"),
        status: CheckStatus::Warn,
        detail: format!("conflicts with existing alias {detail}"),
    })
}

fn add_shell_alias_conflicts(result: &mut DiagResult, config: Option<&Config>) {
    let Some(config) = config else {
        return;
    };

    for abbr in &config.abbr {
        if let Some(check) = check_pwsh_alias(&abbr.key) {
            result.checks.push(check);
        }
        if let Some(check) = check_bash_alias(&abbr.key) {
            result.checks.push(check);
        }
    }
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
                let tag = match check.status {
                    CheckStatus::Ok => "[OK]",
                    CheckStatus::Warn => "[WARN]",
                    CheckStatus::Error => "[ERROR]",
                };
                println!("{tag:>8}  {}: {}", check.name, check.detail);
            }

            if !result.is_healthy() {
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
