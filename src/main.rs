use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;
mod workspace;

#[derive(Parser)]
#[command(
    name = "homecmdr",
    about = "HomeCmdr CLI — install, manage adapters, and deploy your HomeCmdr instance",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bootstrap a new HomeCmdr workspace on this machine.
    ///
    /// Downloads the HomeCmdr API source, generates a config with a secure
    /// master key, and optionally builds the initial binary.
    Init {
        /// Where to create the workspace directory.
        /// Defaults to ~/.local/share/homecmdr/workspace/
        #[arg(long)]
        dir: Option<PathBuf>,

        /// Overwrite an existing workspace at the target directory.
        #[arg(long)]
        force: bool,
    },

    /// Manage installed adapters.
    Adapter {
        #[command(subcommand)]
        subcommand: AdapterCommands,
    },

    /// Build the HomeCmdr binary.
    Build {
        /// Build an optimised release binary and install it to
        /// /usr/local/bin/homecmdr, then restart the service if running.
        #[arg(long)]
        release: bool,
    },

    /// Manage the HomeCmdr systemd service.
    Service {
        #[command(subcommand)]
        subcommand: ServiceCommands,
    },

    /// (Legacy) Pull an official adapter — use 'homecmdr adapter add' instead.
    #[command(hide = true)]
    Pull {
        /// Name of the adapter to pull (e.g. adapter-elgato-lights)
        name: String,
    },

    /// (Legacy) Rebuild the workspace — use 'homecmdr build' instead.
    #[command(hide = true)]
    Rebuild,
}

#[derive(Subcommand)]
enum AdapterCommands {
    /// Download an official adapter, patch the workspace, and rebuild.
    Add {
        /// Name of the adapter (e.g. adapter-elgato-lights)
        name: String,
    },
    /// Remove an installed adapter, unpatch the workspace, and rebuild.
    Remove {
        /// Name of the adapter to remove
        name: String,
    },
    /// List available adapters and show which are installed.
    List,
}

#[derive(Subcommand)]
enum ServiceCommands {
    /// Install HomeCmdr as a systemd service (requires sudo).
    Install,
    /// Remove the systemd unit (preserves config and data).
    Uninstall,
    /// Start the service.
    Start,
    /// Stop the service.
    Stop,
    /// Restart the service.
    Restart,
    /// Show service status.
    Status,
    /// Follow the service logs (Ctrl-C to exit).
    Logs,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init { dir, force } => commands::init::run(dir, force),
        Commands::Adapter { subcommand } => match subcommand {
            AdapterCommands::Add { name } => commands::adapter::add::run(&name),
            AdapterCommands::Remove { name } => commands::adapter::remove::run(&name),
            AdapterCommands::List => commands::adapter::list::run(),
        },
        Commands::Build { release } => commands::build::run(release),
        Commands::Service { subcommand } => match subcommand {
            ServiceCommands::Install => commands::service::install::run(),
            ServiceCommands::Uninstall => commands::service::install::run_uninstall(),
            ServiceCommands::Start => commands::service::manage::start(),
            ServiceCommands::Stop => commands::service::manage::stop(),
            ServiceCommands::Restart => commands::service::manage::restart(),
            ServiceCommands::Status => commands::service::manage::status(),
            ServiceCommands::Logs => commands::service::manage::logs(),
        },
        // Legacy aliases
        Commands::Pull { name } => commands::adapter::add::run(&name),
        Commands::Rebuild => commands::build::run(false),
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
