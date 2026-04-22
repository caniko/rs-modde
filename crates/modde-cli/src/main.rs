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
    /// Manage individual mods (remove, diagnose unknown-type dossiers)
    Mod {
        #[command(subcommand)]
        action: ModAction,
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
    /// Scan game directory for installed mods
    Scan {
        #[arg(long)]
        game: String,
        /// Path to game installation (auto-detected if omitted)
        #[arg(long)]
        game_dir: Option<PathBuf>,
        /// Path to .wabbajack file for manifest matching
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Import discovered mods into this profile
        #[arg(long)]
        import_to: Option<String>,
        /// Minimum file presence fraction (0.0-1.0)
        #[arg(long, default_value = "0.5")]
        threshold: f32,
        /// Report only, don't write to database
        #[arg(long)]
        dry_run: bool,
        /// Before merging, remove any pre-existing filesystem-scanner
        /// rows (cet/*, reds/*, tweak/*, archive/*, redmod/*) from the
        /// target profile that the manifest already covers. Requires
        /// `--manifest` and `--import-to`. See `modde profile dedup`
        /// for the standalone equivalent.
        #[arg(long)]
        prune_duplicates: bool,
    },
    /// Analyse mod collisions and suggest optimisations
    Collisions {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        /// Show all collisions including cosmetic ones
        #[arg(long)]
        all: bool,
        /// Suggest hide commands for redundant files
        #[arg(long)]
        suggest_hides: bool,
    },
    /// Run diagnostics to detect common modding issues
    Diagnostics {
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: Option<String>,
    },
    /// Export mod list to CSV
    Export {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        /// Comma-separated columns
        #[arg(long)]
        columns: Option<String>,
        /// Output file (stdout if omitted)
        #[arg(long)]
        output: Option<String>,
    },
    /// Manage mod and plugin order backups
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// Detect installed games across Steam and Heroic launchers
    Detect,
    /// Manage modde instances (multiple data directories)
    Instance {
        #[command(subcommand)]
        action: InstanceAction,
    },
    /// Import existing TOML profiles into the database
    Import,
    /// Launch the graphical user interface
    Gui,
}

#[derive(Subcommand)]
enum InstanceAction {
    /// Create a new instance
    Create {
        name: String,
        #[arg(long)]
        data_dir: PathBuf,
    },
    /// List all instances
    List,
    /// Switch to an instance
    Switch { name: String },
}

#[derive(Subcommand)]
enum BackupAction {
    /// Create a backup of a mod
    Create { mod_id: String },
    /// Restore a mod from its latest backup
    Restore { mod_id: String },
    /// List available backups for a mod
    List { mod_id: String },
    /// Backup current plugin load order
    Plugins {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        game: String,
    },
    /// Restore plugin load order from backup
    RestorePlugins {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        game: String,
    },
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
        /// Fork unlocked: strip the profile-level load order lock AND
        /// all per-mod pins from the new profile. Use this when you
        /// want to diverge from a Wabbajack / Collection install and
        /// freely reorder mods in the fork.
        #[arg(long)]
        unlock: bool,
    },
    /// Apply a manual profile-level load order lock
    Lock {
        name: String,
        #[arg(long)]
        game: Option<String>,
        /// Optional free-text note explaining why
        #[arg(long)]
        note: Option<String>,
    },
    /// Clear the profile-level load order lock
    Unlock {
        name: String,
        #[arg(long)]
        game: Option<String>,
    },
    /// Show lock status for a profile
    LockInfo {
        name: String,
        #[arg(long)]
        game: Option<String>,
    },
    /// Pin an individual mod in place (per-mod lock).
    LockMod {
        /// Profile name
        name: String,
        /// Mod ID to pin
        mod_id: String,
        #[arg(long)]
        game: Option<String>,
        /// Optional free-text note explaining why
        #[arg(long)]
        note: Option<String>,
    },
    /// Release an individual mod's per-mod pin.
    UnlockMod {
        /// Profile name
        name: String,
        /// Mod ID to unpin
        mod_id: String,
        #[arg(long)]
        game: Option<String>,
    },
    /// Detect (and optionally remove) filesystem-scanner rows in a
    /// profile that duplicate mods the Wabbajack manifest already
    /// installs. See `plans/greedy-shimmying-pine.md` for background.
    ///
    /// Two modes:
    ///   * Without `--manifest`: layer-1 heuristic — list any
    ///     filesystem-scanner rows (cet/*, reds/*, tweak/*, archive/*,
    ///     redmod/*) on a locked profile. Read-only; a no-`--apply`
    ///     report of "suspects".
    ///   * With `--manifest <path>`: layer-2 classification — use the
    ///     manifest's install directives to classify each suspect as
    ///     LEAKED (safe to delete) or GENUINE (user addition, keep).
    ///     Pass `--apply` to delete the LEAKED rows.
    Dedup {
        /// Profile name
        name: String,
        #[arg(long)]
        game: Option<String>,
        /// Path to a .wabbajack file to use as the authoritative
        /// reference for classification. If omitted, only the
        /// layer-1 heuristic runs.
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Actually delete the rows classified as LEAKED. Without this
        /// flag the command is a dry-run report.
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand)]
enum ModAction {
    /// Remove an installed mod from a profile and unlink its staged
    /// files. Uses the V8 `installed_mod_files` manifest so removal is
    /// precise — no orphaned files, no collateral damage.
    Remove {
        /// Mod id as stored in the profile (usually
        /// `<domain>_<mod_id>_<file_id>` for Nexus installs).
        mod_id: String,
        /// Profile to remove from. Defaults to the active / unambiguous
        /// one.
        #[arg(long)]
        profile: Option<String>,
    },
    /// Print the skill dossier path and inline prompt for a mod whose
    /// install type could not be detected. Handy for piping into
    /// `claude` or pasting into a chat manually.
    Diagnose { mod_id: String },
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
    /// Show status of all gaming tools/overlays for a game
    Status {
        #[arg(long)]
        game: String,
    },
    /// Enable a gaming tool/overlay for a game
    Enable {
        /// Tool ID (mangohud, vkbasalt, gamemode, reshade, optiscaler)
        tool_id: String,
        #[arg(long)]
        game: String,
    },
    /// Disable a gaming tool/overlay for a game
    Disable {
        /// Tool ID
        tool_id: String,
        #[arg(long)]
        game: String,
    },
    /// Configure a gaming tool's settings
    Configure {
        /// Tool ID
        tool_id: String,
        #[arg(long)]
        game: String,
        /// Setting key=value pairs
        #[arg(last = true)]
        settings: Vec<String>,
    },
    /// Apply tool patches to the game directory (ReShade DLLs, OptiScaler, etc.)
    Apply {
        /// Tool ID
        tool_id: String,
        #[arg(long)]
        game: String,
    },
    /// Revert tool patches from the game directory
    Revert {
        /// Tool ID
        tool_id: String,
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
        Commands::Scan {
            game,
            game_dir,
            manifest,
            import_to,
            threshold,
            dry_run,
            prune_duplicates,
        } => {
            return commands::scan::handle(
                game,
                game_dir,
                manifest,
                import_to,
                threshold,
                dry_run,
                prune_duplicates,
            );
        }
        Commands::Diagnostics { game, profile } => {
            return commands::diagnostics::handle(&game, profile);
        }
        Commands::Export {
            profile,
            game,
            columns,
            output,
        } => {
            return commands::export::handle(profile, game, columns, output);
        }
        Commands::Backup { action } => {
            return commands::backup::handle(action);
        }
        Commands::Detect => return commands::detect::handle(),
        Commands::Instance { action } => {
            return match action {
                InstanceAction::Create { name, data_dir } => {
                    commands::instance::handle_create(&name, data_dir)
                }
                InstanceAction::List => commands::instance::handle_list(),
                InstanceAction::Switch { name } => commands::instance::handle_switch(&name),
            };
        }
        Commands::Import => return commands::import::handle(),
        Commands::Fomod { action } => return commands::fomod::handle(action),
        Commands::Loot { action } => {
            return match action {
                LootAction::Sort { game, data_dir } => commands::loot::handle_sort(&game, data_dir),
                LootAction::Validate { game } => commands::loot::handle_validate(&game),
            };
        }
        Commands::Tool {
            action: ToolAction::List { game },
        } => {
            return commands::tool::handle_list(&game);
        }
        Commands::Tool {
            action: ToolAction::Status { game },
        } => {
            return commands::tool::handle_status(&game);
        }
        Commands::Tool {
            action: ToolAction::Enable { tool_id, game },
        } => {
            return commands::tool::handle_enable(&tool_id, &game);
        }
        Commands::Tool {
            action: ToolAction::Disable { tool_id, game },
        } => {
            return commands::tool::handle_disable(&tool_id, &game);
        }
        Commands::Tool {
            action:
                ToolAction::Configure {
                    tool_id,
                    game,
                    settings,
                },
        } => {
            return commands::tool::handle_configure(&tool_id, &game, &settings);
        }
        Commands::Tool {
            action: ToolAction::Apply { tool_id, game },
        } => {
            return commands::tool::handle_apply(&tool_id, &game);
        }
        Commands::Tool {
            action: ToolAction::Revert { tool_id, game },
        } => {
            return commands::tool::handle_revert(&tool_id, &game);
        }
        Commands::Nxm {
            action: NxmAction::Install,
        } => {
            commands::nxm::install_handler()?;
            return Ok(());
        }
        _ => {}
    }

    tokio::runtime::Runtime::new()?.block_on(async {
        match cli.command {
            Commands::Play {
                profile,
                game,
                no_deploy,
                no_switch,
                no_capture,
            } => commands::play::handle(profile, game, no_deploy, no_switch, no_capture).await?,
            Commands::Deploy { profile, game } => commands::deploy::handle(profile, game).await?,
            Commands::Collisions {
                profile,
                game,
                all,
                suggest_hides,
            } => commands::collisions::handle(profile, game, all, suggest_hides).await?,
            Commands::Rollback { profile, game } => {
                commands::rollback::handle(profile, game).await?
            }
            Commands::Install { source } => commands::install::handle(source).await?,
            Commands::Mod { action } => match action {
                ModAction::Remove { mod_id, profile } => {
                    commands::uninstall::handle(mod_id, profile).await?
                }
                ModAction::Diagnose { mod_id } => {
                    commands::uninstall::handle_diagnose(mod_id).await?
                }
            },
            Commands::Verify { profile, game } => commands::verify::handle(profile, game).await?,
            Commands::Nexus { action } => commands::nexus::handle(action).await?,
            Commands::Stock { action } => commands::stock::handle(action).await?,
            Commands::Save { action } => commands::save::handle(action).await?,
            Commands::Update { action } => match action {
                UpdateAction::Check {
                    profile,
                    game,
                    period,
                } => commands::update::handle_check(profile, game, period).await?,
            },
            Commands::Tool { action } => match action {
                ToolAction::Run {
                    executable,
                    profile,
                    game,
                    args,
                } => commands::tool::handle_run(executable, args, profile, game).await?,
                ToolAction::List { .. }
                | ToolAction::Status { .. }
                | ToolAction::Enable { .. }
                | ToolAction::Disable { .. }
                | ToolAction::Configure { .. }
                | ToolAction::Apply { .. }
                | ToolAction::Revert { .. } => unreachable!(),
            },
            Commands::Nxm { action } => match action {
                NxmAction::Handle { uri, profile } => commands::nxm::handle(uri, profile).await?,
                NxmAction::Install => unreachable!(),
            },
            // Already handled above
            Commands::Profile { .. }
            | Commands::Scan { .. }
            | Commands::Detect
            | Commands::Import
            | Commands::Instance { .. }
            | Commands::Backup { .. }
            | Commands::Diagnostics { .. }
            | Commands::Export { .. }
            | Commands::Fomod { .. }
            | Commands::Loot { .. }
            | Commands::Gui => unreachable!(),
        }
        Ok(())
    })
}
