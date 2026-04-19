use clap::{Parser, Subcommand};

mod commands;

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
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Pull { name } => commands::pull::run(&name),
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
