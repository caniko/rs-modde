use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod commands;

#[derive(Parser)]
#[command(name = "modde", version, about = "NixOS-native game mod manager")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage profiles
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Deploy mods for the active or specified profile
    Deploy {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Rollback to the previous deployment
    Rollback {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Install mods from various sources
    Install {
        #[command(subcommand)]
        source: InstallSource,
    },
    /// Verify installed file integrity
    Verify {
        #[arg(long)]
        profile: Option<String>,
    },
    /// Nexus Mods account management
    Nexus {
        #[command(subcommand)]
        action: NexusAction,
    },
    /// Manage vanilla game snapshots
    Stock {
        #[command(subcommand)]
        action: StockAction,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    /// List all profiles
    List,
    /// Switch to a profile
    Switch { name: String },
    /// Create a new profile
    Create {
        name: String,
        #[arg(long)]
        game: String,
    },
    /// Delete a profile
    Delete { name: String },
}

#[derive(Subcommand)]
enum InstallSource {
    /// Install a Nexus Collection
    NexusCollection {
        slug: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        profile: Option<String>,
    },
    /// Install from a Wabbajack modlist
    Wabbajack {
        path: std::path::PathBuf,
        #[arg(long)]
        profile: Option<String>,
    },
    /// Install a single mod from Nexus
    Mod {
        url: String,
        #[arg(long)]
        profile: Option<String>,
    },
}

#[derive(Subcommand)]
enum NexusAction {
    /// Save API key to keyring
    Auth,
    /// Show API key validity and premium status
    Status,
}

#[derive(Subcommand)]
enum StockAction {
    /// Create a vanilla game snapshot
    Snapshot { game_id: String },
    /// Verify snapshot integrity
    Verify { game_id: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Profile { action } => commands::profile::handle(action).await?,
        Commands::Deploy { profile } => commands::deploy::handle(profile).await?,
        Commands::Rollback { profile } => commands::rollback::handle(profile).await?,
        Commands::Install { source } => commands::install::handle(source).await?,
        Commands::Verify { profile } => commands::verify::handle(profile).await?,
        Commands::Nexus { action } => commands::nexus::handle(action).await?,
        Commands::Stock { action } => commands::stock::handle(action).await?,
    }

    Ok(())
}
