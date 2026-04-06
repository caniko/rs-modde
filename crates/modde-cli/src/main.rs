use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod commands;

#[derive(Parser)]
#[command(name = "modde", version, about = "NixOS-native game mod manager")]
struct Cli {
    /// Override data directory (default: ~/.local/share/modde or $MODDE_DATA_DIR)
    #[arg(long, global = true, env = "MODDE_DATA_DIR")]
    data_dir: Option<PathBuf>,

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
    /// Switch profile, deploy mods, and launch the game
    Play {
        /// Profile to activate (uses active profile if omitted)
        profile: Option<String>,
        #[arg(long)]
        game: String,
        /// Skip mod deployment (just switch + launch)
        #[arg(long)]
        no_deploy: bool,
        /// Skip profile switch (just deploy + launch active)
        #[arg(long)]
        no_switch: bool,
        /// Skip save auto-capture after game exit
        #[arg(long)]
        no_capture: bool,
    },
    /// Deploy mods for the active or specified profile
    Deploy {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
    /// Rollback to the previous deployment
    Rollback {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
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
        #[arg(long)]
        game: Option<String>,
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
    /// FOMOD installer utilities
    Fomod {
        #[command(subcommand)]
        action: FomodAction,
    },
    /// Manage save file assignments
    Save {
        #[command(subcommand)]
        action: SaveAction,
    },
    /// Check for mod updates from Nexus
    Update {
        #[command(subcommand)]
        action: UpdateAction,
    },
    /// LOOT masterlist integration (Bethesda plugin sorting)
    Loot {
        #[command(subcommand)]
        action: LootAction,
    },
    /// Run external tools with overwrite capture
    Tool {
        #[command(subcommand)]
        action: ToolAction,
    },
    /// Handle nxm:// download links from Nexus Mods
    Nxm {
        #[command(subcommand)]
        action: NxmAction,
    },
    /// Detect installed games across Steam and Heroic launchers
    Detect,
    /// Import existing TOML profiles into the database
    Import,
    /// Launch the graphical user interface
    Gui,
}

#[derive(Subcommand)]
enum ProfileAction {
    /// List all profiles
    List {
        /// Filter by game
        #[arg(long)]
        game: Option<String>,
    },
    /// Switch to a profile (swaps saves automatically)
    Switch {
        name: String,
        #[arg(long)]
        game: String,
    },
    /// Create a new profile
    Create {
        name: String,
        #[arg(long)]
        game: String,
    },
    /// Delete a profile
    Delete {
        name: String,
        #[arg(long)]
        game: Option<String>,
    },
    /// Experiment with a profile (can be stacked, like git branches)
    Try {
        name: String,
        #[arg(long)]
        game: String,
    },
    /// Roll back to the previous profile (undo the last `try`)
    Rollback {
        #[arg(long)]
        game: String,
    },
    /// Accept the current experiment, clearing the rollback stack
    Commit {
        #[arg(long)]
        game: String,
    },
    /// Show the active profile for a game
    Active {
        #[arg(long)]
        game: String,
    },
    /// Fork a profile (clone mods + saves into a new profile)
    Fork {
        /// Source profile to clone from
        source: String,
        /// Name for the new profile
        name: String,
        #[arg(long)]
        game: String,
    },
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
        path: PathBuf,
        #[arg(long)]
        profile: Option<String>,
        /// Game installation directory to deploy mods into
        #[arg(long)]
        game_dir: Option<PathBuf>,
        /// Force full reinstall, skipping preflight checks
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Install a single mod from Nexus
    Mod {
        url: String,
        #[arg(long)]
        profile: Option<String>,
        /// Path to a FOMOD declarative config (TOML or JSON) for non-interactive installation
        #[arg(long)]
        fomod_config: Option<String>,
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
enum FomodAction {
    /// Generate a declarative FOMOD config template from a mod's ModuleConfig.xml
    Generate {
        /// Path to the mod directory containing fomod/ModuleConfig.xml
        mod_path: String,
        /// Include all plugins (not just defaults)
        #[arg(long)]
        all: bool,
        /// Output format: toml, json, or nix
        #[arg(long, default_value = "toml")]
        format: String,
    },
    /// Apply a declarative FOMOD config non-interactively
    Apply {
        /// Path to the mod directory
        mod_path: String,
        /// Path to the declarative config (TOML or JSON)
        #[arg(long)]
        config: String,
        /// Destination directory for installed files
        #[arg(long)]
        dest: String,
    },
    /// Inspect a mod's FOMOD steps, groups, and plugins
    Inspect {
        /// Path to the mod directory containing fomod/ModuleConfig.xml
        mod_path: String,
    },
}

#[derive(Subcommand)]
enum StockAction {
    /// Create a vanilla game snapshot
    Snapshot { game_id: String },
    /// Verify snapshot integrity
    Verify { game_id: String },
}

#[derive(Subcommand)]
enum UpdateAction {
    /// Check for updates on Nexus Mods
    Check {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        /// Time period to check: "1d", "1w", or "1m"
        #[arg(long, default_value = "1w")]
        period: String,
    },
}

#[derive(Subcommand)]
enum LootAction {
    /// Sort plugins using LOOT masterlist rules
    Sort {
        #[arg(long)]
        game: String,
        /// Path to game Data directory (auto-detected if omitted)
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Validate plugins for Form 43 and missing master errors
    Validate {
        #[arg(long)]
        game: String,
    },
}

#[derive(Subcommand)]
enum ToolAction {
    /// Run an external tool with overwrite capture
    Run {
        /// Path to executable
        executable: PathBuf,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        /// Arguments to pass to the tool
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// List detected tools for a game
    List {
        #[arg(long)]
        game: String,
    },
}

#[derive(Subcommand)]
enum NxmAction {
    /// Handle an nxm:// download URI
    Handle {
        /// The nxm:// URI
        uri: String,
        #[arg(long)]
        profile: Option<String>,
    },
    /// Install the nxm:// URI handler for your desktop
    Install,
}

#[derive(Subcommand)]
enum SaveAction {
    /// Assign a save to a profile
    Assign {
        /// Path to the save file or directory
        path: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        game: Option<String>,
        /// Optional label for this save
        #[arg(long)]
        label: Option<String>,
    },
    /// Remove a save assignment
    Unassign {
        /// Path to the save file or directory
        path: PathBuf,
    },
    /// List saves for a profile
    List {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        game: Option<String>,
    },
    /// Scan for unassigned saves
    Scan {
        #[arg(long)]
        game: String,
    },
    /// Adopt existing saves from the game directory into a profile's vault
    Adopt {
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: String,
    },
    /// Capture current saves into the vault (creates a new snapshot)
    Capture {
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: String,
        /// Optional message for this snapshot
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Show save snapshot history for a profile
    History {
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: String,
        /// Max entries to show
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Restore saves from a specific snapshot
    Restore {
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: String,
        /// Commit ID (or prefix) to restore
        commit: String,
    },
    /// Auto-detect and capture new saves (called by launch wrapper on game exit)
    AutoCapture {
        #[arg(long)]
        game: String,
        /// Profile to capture into (defaults to active profile)
        #[arg(long)]
        profile: Option<String>,
    },
    /// Watch for save changes and auto-capture (polling)
    Watch {
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: Option<String>,
        /// Poll interval in seconds
        #[arg(long, default_value = "30")]
        interval: u64,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    if let Some(dir) = cli.data_dir {
        modde_core::paths::set_data_dir(dir);
    }

    // GUI launches its own runtime (iced), so handle it outside tokio.
    if matches!(cli.command, Commands::Gui) {
        modde_ui::app::run().map_err(|e| anyhow::anyhow!("GUI error: {e}"))?;
        return Ok(());
    }

    // Sync commands that don't need the tokio runtime
    match cli.command {
        Commands::Profile { action } => return commands::profile::handle(action),
        Commands::Detect => return commands::detect::handle(),
        Commands::Import => return commands::import::handle(),
        Commands::Fomod { action } => return commands::fomod::handle(action),
        Commands::Loot { action } => {
            return match action {
                LootAction::Sort { game, data_dir } => commands::loot::handle_sort(&game, data_dir),
                LootAction::Validate { game } => commands::loot::handle_validate(&game),
            };
        }
        Commands::Tool { action: ToolAction::List { game } } => {
            return commands::tool::handle_list(&game);
        }
        Commands::Nxm { action: NxmAction::Install } => {
            commands::nxm::install_handler()?;
            return Ok(());
        }
        _ => {}
    }

    tokio::runtime::Runtime::new()?.block_on(async {
        match cli.command {
            Commands::Play { profile, game, no_deploy, no_switch, no_capture } => {
                commands::play::handle(profile, game, no_deploy, no_switch, no_capture).await?
            }
            Commands::Deploy { profile, game } => commands::deploy::handle(profile, game).await?,
            Commands::Rollback { profile, game } => {
                commands::rollback::handle(profile, game).await?
            }
            Commands::Install { source } => commands::install::handle(source).await?,
            Commands::Verify { profile, game } => {
                commands::verify::handle(profile, game).await?
            }
            Commands::Nexus { action } => commands::nexus::handle(action).await?,
            Commands::Stock { action } => commands::stock::handle(action).await?,
            Commands::Save { action } => commands::save::handle(action).await?,
            Commands::Update { action } => match action {
                UpdateAction::Check { profile, game, period } => {
                    commands::update::handle_check(profile, game, period).await?
                }
            },
            Commands::Tool { action } => match action {
                ToolAction::Run { executable, profile, game, args } => {
                    commands::tool::handle_run(executable, args, profile, game).await?
                }
                ToolAction::List { .. } => unreachable!(),
            },
            Commands::Nxm { action } => match action {
                NxmAction::Handle { uri, profile } => {
                    commands::nxm::handle(uri, profile).await?
                }
                NxmAction::Install => unreachable!(),
            },
            // Already handled above
            Commands::Profile { .. } | Commands::Detect | Commands::Import
            | Commands::Fomod { .. } | Commands::Loot { .. } | Commands::Gui => unreachable!(),
        }
        Ok(())
    })
}
