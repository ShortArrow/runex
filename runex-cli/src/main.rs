use clap::{Parser, Subcommand};
use runex_core::config::{default_config_path, load_config};
use runex_core::expand;
use runex_core::model::ExpandResult;

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
    /// Check environment health (stub)
    Doctor,
    /// Export shell integration (stub)
    Export {
        /// Target shell: pwsh, bash, nu, clink
        shell: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config_path = default_config_path()?;
    let config = load_config(&config_path)?;

    match cli.command {
        Commands::Expand { token } => {
            let result = expand::expand(&config, &token, |cmd| which::which(cmd).is_ok());
            match result {
                ExpandResult::Expanded(s) => print!("{s}"),
                ExpandResult::PassThrough(s) => print!("{s}"),
            }
        }
        Commands::List => {
            for (key, exp) in expand::list(&config) {
                println!("{key}\t{exp}");
            }
        }
        Commands::Doctor => {
            println!("doctor: not yet implemented");
        }
        Commands::Export { shell } => {
            println!("export {shell}: not yet implemented");
        }
    }

    Ok(())
}
