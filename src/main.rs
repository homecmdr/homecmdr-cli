use clap::{Parser, Subcommand};

mod commands;
mod workspace;

#[derive(Parser)]
#[command(
    name = "homecmdr",
    about = "HomeCmdr CLI — manage adapters for your HomeCmdr workspace",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Pull an official adapter into the current HomeCmdr workspace
    Pull {
        /// Name of the adapter to pull (e.g. adapter-elgato-lights)
        name: String,
    },
    /// Rebuild the HomeCmdr workspace (runs cargo build)
    Rebuild,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Pull { name } => commands::pull::run(&name),
        Commands::Rebuild => commands::rebuild::run(),
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
