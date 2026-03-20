use clap::{Parser, Subcommand};
use runex_core::config::{default_config_path, load_config};
use runex_core::doctor::{self, CheckStatus};
use runex_core::expand;
use runex_core::model::ExpandResult;
use runex_core::shell::Shell;

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
            print!("{}", runex_core::shell::export_script(s, &bin));
        }
        Commands::Doctor => {
            let config_path = default_config_path().unwrap_or_default();
            let config = load_config(&config_path).ok();
            let result =
                doctor::diagnose(&config_path, config.as_ref(), |cmd| which::which(cmd).is_ok());

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
