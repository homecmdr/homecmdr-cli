use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;
mod workspace;

#[derive(Parser)]
#[command(
    name = "homecmdr",
    about = "HomeCmdr CLI — install, manage plugins, and deploy your HomeCmdr instance",
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

    /// Manage installed plugins.
    Plugin {
        #[command(subcommand)]
        subcommand: PluginCommands,
    },

    /// Build the HomeCmdr binary.
    Build {
        /// Build an optimised release binary and install it to
        /// /usr/local/bin/homecmdr, then restart the service if running.
        ///
        /// By default a release build will attempt to download a pre-built
        /// binary from GitHub Releases for this platform before invoking
        /// cargo.  Use --from-source to skip the download and always build
        /// locally (required when you have just added a plugin).
        #[arg(long)]
        release: bool,

        /// Skip the pre-built binary download and always compile from source.
        /// This is set automatically when a plugin has just been added or
        /// removed; pass it manually if you need a fully custom local build.
        #[arg(long)]
        from_source: bool,
    },

    /// Manage the HomeCmdr systemd service.
    Service {
        #[command(subcommand)]
        subcommand: ServiceCommands,
    },
}

#[derive(Subcommand)]
enum PluginCommands {
    /// Download a plugin, patch the workspace, and rebuild.
    ///
    /// Accepts either the full name (adapter-elgato-lights) or the short
    /// name without the 'adapter-' prefix (elgato-lights).
    Add {
        /// Name of the plugin (e.g. elgato-lights or adapter-elgato-lights)
        name: String,
    },
    /// Remove an installed plugin, unpatch the workspace, and rebuild.
    Remove {
        /// Name of the plugin to remove
        name: String,
    },
    /// List available plugins and show which are installed.
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
        Commands::Plugin { subcommand } => match subcommand {
            PluginCommands::Add { name } => commands::plugin::add::run(&name),
            PluginCommands::Remove { name } => commands::plugin::remove::run(&name),
            PluginCommands::List => commands::plugin::list::run(),
        },
        Commands::Build { release, from_source } => commands::build::run(release, from_source),
        Commands::Service { subcommand } => match subcommand {
            ServiceCommands::Install => commands::service::install::run(),
            ServiceCommands::Uninstall => commands::service::install::run_uninstall(),
            ServiceCommands::Start => commands::service::manage::start(),
            ServiceCommands::Stop => commands::service::manage::stop(),
            ServiceCommands::Restart => commands::service::manage::restart(),
            ServiceCommands::Status => commands::service::manage::status(),
            ServiceCommands::Logs => commands::service::manage::logs(),
        },
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
