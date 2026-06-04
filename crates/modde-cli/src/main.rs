use std::path::PathBuf;
#[cfg(feature = "remote-telemetry")]
use std::{env, time::Duration};

#[cfg(feature = "remote-telemetry")]
use anyhow::Context;
use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;
#[cfg(feature = "remote-telemetry")]
use tracing_subscriber::prelude::*;

mod commands;
#[cfg(feature = "remote-telemetry")]
mod telemetry;

#[derive(Parser)]
#[command(name = "modde", version, about = "NixOS-native game mod manager")]
struct Cli {
    /// Override data directory (default: ~/.local/share/modde or $`MODDE_DATA_DIR`)
    #[arg(long, global = true, env = "MODDE_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Write a DHAT heap profile. Requires the `heap-profile` cargo feature.
    #[arg(long, global = true, env = "MODDE_HEAP_PROFILE")]
    heap_profile: Option<PathBuf>,

    /// Panic after startup to smoke test remote telemetry crash capture.
    #[cfg(feature = "remote-telemetry")]
    #[arg(long, global = true, hide = true)]
    debug_panic: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(hide = true)]
    Dev {
        #[command(subcommand)]
        action: DevAction,
    },
    /// Manage profiles
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Inspect and set modde configuration (including the database backend)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
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
    /// Browse, download, and generate Home Manager snippets for Wabbajack modlists
    Wabbajack {
        #[command(subcommand)]
        action: WabbajackAction,
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
    /// Manage user-defined games
    Game {
        #[command(subcommand)]
        action: GameAction,
    },
    /// Manage modde instances (multiple data directories)
    Instance {
        #[command(subcommand)]
        action: InstanceAction,
    },
    /// Manage named launch targets (`xEdit`, `BodySlide`, `FNIS`, etc.)
    ///
    /// Thin alias for the executable subset of `modde tool`. The
    /// underlying storage is shared, so `modde exec list` and
    /// `modde tool list-executables` print the same rows.
    Exec {
        #[command(subcommand)]
        action: ExecAction,
    },
    /// Install and update modde-maintained Codex skills
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Import existing TOML profiles into the database
    Import,
    /// Launch the graphical user interface
    Gui,
}

#[derive(Subcommand)]
enum ExecAction {
    /// Save (or update) a named launch target.
    ///
    /// Re-running with the same name overwrites the existing entry,
    /// so `add` doubles as `edit`.
    Add {
        name: String,
        executable: PathBuf,
        #[arg(long)]
        game: String,
        #[arg(long)]
        working_dir: Option<PathBuf>,
        #[arg(long, default_value = "__overwrite__")]
        output_mod: String,
        #[arg(long)]
        wine_dll_overrides: Option<String>,
        #[arg(long = "env")]
        environment: Vec<String>,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// List configured launch targets for a game.
    List {
        #[arg(long)]
        game: String,
    },
    /// Remove a launch target.
    Remove {
        name: String,
        #[arg(long)]
        game: String,
    },
    /// Run a saved launch target with overwrite capture.
    Run {
        name: String,
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(last = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum DevAction {
    #[command(hide = true)]
    ExportToolSchema {
        #[arg(long, default_value = "nix/tool-schema.nix")]
        out: PathBuf,
    },
    /// Render a GUI screen headlessly (offscreen, no GPU/display) to a PNG.
    ///
    /// Used to generate Flathub / website assets and for future CI
    /// automation. Run with `ICED_BACKEND=tiny-skia` for determinism.
    #[cfg(feature = "screenshot")]
    #[command(hide = true)]
    Screenshot {
        /// Which screen to render (ignored when `--all` is set).
        #[arg(long, value_enum, default_value_t = ScreenArg::ModList)]
        screen: ScreenArg,
        /// Output PNG path. With `--all`, this is the output *directory*
        /// (each screen written as `<dir>/<screen>.png`).
        #[arg(long, default_value = "screenshot.png")]
        out: PathBuf,
        /// Render every screen into the `--out` directory.
        #[arg(long)]
        all: bool,
        /// Logical window width in points.
        #[arg(long, default_value_t = 1280.0)]
        width: f32,
        /// Logical window height in points.
        #[arg(long, default_value_t = 800.0)]
        height: f32,
        /// HiDPI scale factor (2.0 = crisp).
        #[arg(long, default_value_t = 2.0)]
        scale: f32,
        /// modde theme name (Dark, Light, Dracula, Nord, ...).
        #[arg(long, default_value = "Dark")]
        theme: String,
    },
}

#[cfg(feature = "screenshot")]
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum ScreenArg {
    ModList,
    Downloads,
    FomodWizard,
    Tools,
    BrowseNexus,
}

#[cfg(feature = "screenshot")]
impl From<ScreenArg> for modde_ui::screenshot::Screen {
    fn from(value: ScreenArg) -> Self {
        match value {
            ScreenArg::ModList => modde_ui::screenshot::Screen::ModList,
            ScreenArg::Downloads => modde_ui::screenshot::Screen::Downloads,
            ScreenArg::FomodWizard => modde_ui::screenshot::Screen::FomodWizard,
            ScreenArg::Tools => modde_ui::screenshot::Screen::Tools,
            ScreenArg::BrowseNexus => modde_ui::screenshot::Screen::BrowseNexus,
        }
    }
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show the resolved configuration (database backend and its source)
    Show,
    /// Open the resolved database and run a connection probe
    Test,
    /// Set the database backend and `PostgreSQL` connection parameters
    SetDatabase {
        /// Backend to use: `sqlite` (default) or `postgres`
        #[arg(long)]
        backend: String,
        /// Full `PostgreSQL` connection URL (overrides the discrete fields)
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        /// `PostgreSQL` database name
        #[arg(long = "name")]
        dbname: Option<String>,
        #[arg(long)]
        user: Option<String>,
        /// Path to a file containing the `PostgreSQL` password
        #[arg(long)]
        password_file: Option<PathBuf>,
        /// Clear a stored database field from settings.toml
        #[arg(long = "clear")]
        clear: Vec<commands::config::ClearDatabaseField>,
    },
    /// Reset the stored database backend to `SQLite` and clear `PostgreSQL` fields
    ResetDatabase,
}

#[derive(Subcommand)]
enum SkillAction {
    /// List built-in modde skills and install status
    List,
    /// Print the target skill directory
    Path,
    /// Install or update one built-in skill, or `all`
    Install {
        /// Skill name, or `all`
        name: String,
        /// Replace installed skills even when the installed version is newer
        #[arg(long)]
        force: bool,
    },
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
enum GameAction {
    /// Add or overwrite a user-defined game registration
    Add {
        id: String,
        #[arg(long)]
        display_name: String,
        #[arg(long)]
        executable_dir: PathBuf,
        #[arg(long)]
        steam_app_id: Option<String>,
        #[arg(long)]
        install_dir_name: Option<String>,
        #[arg(long)]
        mod_dir: Option<PathBuf>,
        #[arg(long)]
        nexus_domain: Option<String>,
        #[arg(long = "proxy-dll")]
        proxy_dlls: Vec<String>,
        /// Overwrite an existing user-defined game TOML
        #[arg(long)]
        force: bool,
    },
    /// List user-defined games
    List,
    /// Remove a user-defined game registration
    Remove {
        id: String,
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Detect executable-bearing directories under a game install
    Detect { install_path: PathBuf },
    /// Show a resolved game registration
    Show { id: String },
    /// Export a game registration to TOML
    Export {
        id: String,
        #[arg(long)]
        with_optiscaler: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Import a game registration from TOML
    Import {
        path: PathBuf,
        #[arg(long)]
        force: bool,
    },
    #[allow(clippy::doc_markdown)]
    /// Import OptiScaler profiles from TOML for a game
    ImportProfile {
        path: PathBuf,
        #[arg(long = "for")]
        game: String,
        #[arg(long)]
        force: bool,
    },
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
        /// Stage the modlist into modde's data directory but skip the final copy
        /// into `--game-dir`. Useful for Stock-Game lists that should not write
        /// into the live game install.
        #[arg(long, default_value_t = false)]
        no_deploy: bool,
        /// Log per-archive failures (downloads, missing files, broken upstream
        /// links) instead of aborting. The install proceeds as far as possible
        /// and the operator can drop manually-fetched archives into the store
        /// before re-running.
        #[arg(long, default_value_t = false)]
        continue_on_error: bool,
        /// Explicitly discard existing Wabbajack staging before installing.
        #[arg(long, default_value_t = false)]
        reset_staging: bool,
        /// Skip staging validation before deploy.
        #[arg(long, default_value_t = false)]
        skip_validate: bool,
        /// Write Wabbajack apply diagnostics JSONL to this directory.
        #[arg(long)]
        diagnostics_dir: Option<PathBuf>,
        /// Diagnostics heartbeat interval in seconds.
        #[arg(long, default_value_t = 30)]
        diagnostics_interval: u64,
        /// Warn when apply makes no batch/sentinel progress for this many seconds.
        #[arg(long, default_value_t = 600)]
        stall_warn_seconds: u64,
        /// Abort when stalled this long and cgroup memory/swap are saturated.
        #[arg(long, default_value_t = 1800)]
        stall_abort_seconds: u64,
        /// Source archive retention after successful archive-batch integration.
        #[arg(long, value_enum, default_value_t = WabbajackArchiveRetentionArg::Keep)]
        archive_retention: WabbajackArchiveRetentionArg,
        /// Behavior when optional manual/Nexus Wabbajack archives are missing.
        #[arg(long, value_enum, default_value_t = WabbajackMissingArchivePolicyArg::Fail)]
        missing_archive_policy: WabbajackMissingArchivePolicyArg,
        /// Frontload assisted manual archive acquisition before applying.
        #[arg(long, default_value_t = false)]
        acquire_missing: bool,
        /// Browser download directory to watch during assisted acquisition.
        #[arg(long)]
        acquire_download_dir: Option<PathBuf>,
        /// Per-archive assisted acquisition timeout in seconds.
        #[arg(long, default_value_t = 900)]
        acquire_timeout: u64,
        /// Include Nexus archives in frontloaded acquisition.
        #[arg(long, default_value_t = false)]
        acquire_include_nexus: bool,
        /// Use controlled Chromium tabs for frontloaded acquisition.
        #[arg(long, default_value_t = false)]
        acquire_browser_controller: bool,
        /// Disable automatic frontloaded manual archive acquisition.
        #[arg(long, default_value_t = false)]
        no_acquire_missing: bool,
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

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum WabbajackArchiveRetentionArg {
    Keep,
    PruneApplied,
    Auto,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum WabbajackMissingArchivePolicyArg {
    Fail,
    OmitFiles,
    OmitMods,
}

impl From<WabbajackMissingArchivePolicyArg>
    for modde_sources::wabbajack::impact::MissingArchivePolicy
{
    fn from(value: WabbajackMissingArchivePolicyArg) -> Self {
        match value {
            WabbajackMissingArchivePolicyArg::Fail => Self::Fail,
            WabbajackMissingArchivePolicyArg::OmitFiles => Self::OmitFiles,
            WabbajackMissingArchivePolicyArg::OmitMods => Self::OmitMods,
        }
    }
}

impl From<WabbajackArchiveRetentionArg>
    for modde_sources::wabbajack::installer::ArchiveRetentionPolicy
{
    fn from(value: WabbajackArchiveRetentionArg) -> Self {
        match value {
            WabbajackArchiveRetentionArg::Keep => Self::Keep,
            WabbajackArchiveRetentionArg::PruneApplied => Self::PruneApplied,
            WabbajackArchiveRetentionArg::Auto => Self::Auto,
        }
    }
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
        /// Check Nexus-tracked profile mods instead of modde itself.
        #[arg(long)]
        mods: bool,
    },
    /// Download and install the latest MAIN file for any tracked mod
    /// in the profile that has a newer version on Nexus.
    ///
    /// Refuses to run on locked profiles (Wabbajack / Collection /
    /// TOML import) unless `--confirm-locked` is passed — the lock
    /// exists precisely to prevent the load order drifting away from
    /// its authoritative source. Mods whose semver major bumps are
    /// flagged "breaking" and require `--accept-breaking` plus a
    /// secondary y/N confirmation per mod.
    Apply {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        /// Time period to scan: "1d", "1w", or "1m"
        #[arg(long, default_value = "1w")]
        period: String,
        /// Print the mods that would be updated without downloading.
        #[arg(long)]
        dry_run: bool,
        /// Acknowledge that the profile is locked (Wabbajack / Collection
        /// / TOML import) and that updating drifts it away from the
        /// authoritative source. Required for any locked profile.
        #[arg(long)]
        confirm_locked: bool,
        /// Permit applying updates that look like breaking semver bumps
        /// (major version change). Each breaking mod still requires an
        /// interactive y/N confirmation.
        #[arg(long)]
        accept_breaking: bool,
        /// Skip interactive prompts (assume "yes" to per-mod breaking
        /// confirmations). Refuses any breaking update unless
        /// `--accept-breaking` is also set.
        #[arg(long)]
        yes: bool,
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
    /// Save a named executable launch target for a game
    AddExecutable {
        /// Display name, e.g. `xEdit` or `BodySlide`
        name: String,
        /// Path to executable
        executable: PathBuf,
        #[arg(long)]
        game: String,
        /// Working directory. Defaults to the detected game install directory.
        #[arg(long)]
        working_dir: Option<PathBuf>,
        /// Output mod for captured files. Defaults to __overwrite__.
        #[arg(long, default_value = "__overwrite__")]
        output_mod: String,
        /// Wine DLL overrides, e.g. dinput8=n,b;winmm=n,b
        #[arg(long)]
        wine_dll_overrides: Option<String>,
        /// Environment variable assignments, KEY=VALUE
        #[arg(long = "env")]
        environment: Vec<String>,
        /// Default arguments to pass when running this executable
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// List saved executable launch targets for a game
    ListExecutables {
        #[arg(long)]
        game: String,
    },
    /// Remove a saved executable launch target
    RemoveExecutable {
        name: String,
        #[arg(long)]
        game: String,
    },
    /// Run a saved executable launch target with overwrite capture
    RunExecutable {
        name: String,
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: Option<String>,
        /// Additional arguments appended after the saved defaults
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
        /// Tool ID (mangohud, vkbasalt, gamemode, reshade, optiscaler, proton)
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
    /// Apply tool patches to the game directory (`ReShade` DLLs, `OptiScaler`, etc.)
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
    /// List releases for a release-backed tool
    Releases {
        /// Tool ID
        tool_id: String,
        #[arg(long)]
        game: String,
    },
    /// Install a specific release asset for a release-backed tool
    InstallRelease {
        /// Tool ID
        tool_id: String,
        #[arg(long)]
        game: String,
        #[arg(long)]
        tag: String,
        #[arg(long)]
        asset: String,
    },
    /// Install a specific release asset from a local path for a release-backed tool
    InstallReleaseFromPath {
        /// Tool ID
        tool_id: String,
        #[arg(long)]
        game: String,
        #[arg(long)]
        tag: String,
        #[arg(long)]
        asset: String,
        /// Local path to the already-downloaded asset
        path: PathBuf,
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
pub enum WabbajackAction {
    /// Search public Wabbajack modlist catalogs
    Search {
        query: Option<String>,
        #[arg(long)]
        game: Option<String>,
        /// Catalog source: official, authored, or both
        #[arg(long, default_value = "both")]
        source: String,
        #[arg(long)]
        json: bool,
    },
    /// Download a .wabbajack file by URL, machine URL, or title
    Download {
        url_or_machine_url: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Generate a Home Manager profile snippet for a .wabbajack source
    HmSnippet {
        url_or_file: String,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        game: String,
        #[arg(long)]
        game_dir: Option<PathBuf>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Import local archives into the modde store by matching Wabbajack hashes
    ImportArchive {
        manifest: PathBuf,
        archives: Vec<PathBuf>,
    },
    /// Open manual Wabbajack archive pages and import matching browser downloads
    AcquireMissing {
        manifest: PathBuf,
        #[arg(long)]
        download_dir: Option<PathBuf>,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        browser_profile: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        include_nexus: bool,
        #[arg(long, default_value_t = false)]
        browser_controller: bool,
        #[arg(long, default_value_t = 900)]
        timeout: u64,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Report missing manual/Nexus archives and their install impact
    MissingImpact {
        manifest: PathBuf,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        /// Print Home Manager manualArchives entries for missing archives.
        #[arg(long)]
        nix_snippet: bool,
    },
    /// Print missing manual archive URLs that require operator visits
    ManualLinks {
        manifest: PathBuf,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Assess readiness for a large Wabbajack install without mutating state
    Assess {
        manifest: PathBuf,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Summarize Wabbajack diagnostics JSONL from a previous install run
    AnalyzeDiagnostics {
        diagnostics_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
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
    #[cfg(not(feature = "remote-telemetry"))]
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    #[cfg(feature = "remote-telemetry")]
    let telemetry_runtime = tokio::runtime::Runtime::new()?;
    #[cfg(feature = "remote-telemetry")]
    let _telemetry_runtime_guard = telemetry_runtime.enter();
    #[cfg(feature = "remote-telemetry")]
    init_tracing()?;
    #[cfg(feature = "remote-telemetry")]
    init_remote_telemetry(&telemetry_runtime)?;

    let cli = Cli::parse();
    #[cfg(feature = "remote-telemetry")]
    {
        assert!(!cli.debug_panic, "remote telemetry debug panic");
    }

    let _heap_profiler = start_heap_profiler(cli.heap_profile.as_deref())?;

    if let Some(dir) = cli.data_dir.clone() {
        modde_core::paths::set_data_dir(dir);
    }

    // GUI launches its own runtime (iced), so handle it outside tokio.
    if matches!(cli.command, Commands::Gui) {
        modde_ui::app::run().map_err(|e| anyhow::anyhow!("GUI error: {e}"))?;
        return Ok(());
    }

    // Whether this command may have mutated the profile DB / store.
    // Used after the dispatch below to push a refresh signal to any
    // running GUI(s). Read-only commands skip the notify so we don't
    // spam GUIs on `modde profile show` or `modde update check`.
    let mutates_state = command_mutates_state(&cli.command);
    let lazy_update_check = !mutates_state && command_runs_lazy_product_update_check(&cli.command);

    let result = run_command(cli);
    if mutates_state && result.is_ok() {
        let _ = modde_core::ipc::notify_refresh();
    }
    if lazy_update_check && result.is_ok() {
        maybe_print_product_update_notice();
    }
    result
}

#[cfg(feature = "remote-telemetry")]
fn init_tracing() -> Result<()> {
    let fmt_layer = tracing_subscriber::fmt::layer();
    let filter = EnvFilter::from_default_env();
    let registry = tracing_subscriber::registry().with(filter).with(fmt_layer);

    if let Some(config) = remote_telemetry_config()? {
        let layer = detritus::Layer::builder()
            .endpoint(config.endpoint.clone())
            .token(config.token.clone())
            .source(config.source.clone())
            .queue_dir(config.logs_dir.clone())
            .build()
            .context("failed to initialize remote telemetry tracing layer")?;

        registry
            .with(layer)
            .try_init()
            .context("failed to initialize tracing subscriber")?;
    } else {
        registry
            .try_init()
            .context("failed to initialize tracing subscriber")?;
    }

    Ok(())
}

#[cfg(feature = "remote-telemetry")]
#[derive(Clone)]
struct RemoteTelemetryConfig {
    endpoint: url::Url,
    token: secrecy::SecretString,
    source: detritus::SourceId,
    logs_dir: PathBuf,
    crashes_dir: PathBuf,
}

#[cfg(feature = "remote-telemetry")]
fn remote_telemetry_config() -> Result<Option<RemoteTelemetryConfig>> {
    let Some(endpoint) = env::var("RS_MODDE_TELEMETRY_ENDPOINT").ok() else {
        return Ok(None);
    };
    let Some(token) = env::var("RS_MODDE_TELEMETRY_TOKEN").ok() else {
        return Ok(None);
    };

    let endpoint = url::Url::parse(&endpoint).context("invalid RS_MODDE_TELEMETRY_ENDPOINT")?;
    let source = detritus::SourceId {
        project: "rs-modde".to_owned(),
        platform: target_platform(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        install_id: telemetry::persistent_install_id()?,
    };
    let telemetry_dir = telemetry::telemetry_dir()?;

    Ok(Some(RemoteTelemetryConfig {
        endpoint,
        token: secrecy::SecretString::from(token),
        source,
        logs_dir: telemetry_dir.join("logs"),
        crashes_dir: telemetry_dir.join("crashes"),
    }))
}

#[cfg(feature = "remote-telemetry")]
fn init_remote_telemetry(runtime: &tokio::runtime::Runtime) -> Result<()> {
    let Some(config) = remote_telemetry_config()? else {
        return Ok(());
    };

    detritus::install_panic_hook(detritus::PanicHookConfig {
        endpoint: config.endpoint.clone(),
        token: config.token.clone(),
        source: config.source.clone(),
        spool_dir: config.crashes_dir.clone(),
        kind: detritus::PanicKind::PanicTarball,
        build: detritus_protocol::BuildInfo {
            git_sha: option_env!("MODDE_GIT_SHA").unwrap_or("unknown").to_owned(),
            profile: option_env!("PROFILE").unwrap_or("unknown").to_owned(),
            target_triple: target_platform(),
        },
        context: serde_json::json!({}),
        sent_retention_days: 90,
    })
    .context("failed to install remote telemetry panic hook")?;

    let ship_result = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(3),
            detritus::ship_pending_crashes(
                &config.crashes_dir,
                config.endpoint.clone(),
                config.token.clone(),
            ),
        )
        .await
    });

    match ship_result {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => tracing::warn!(%error, "failed to ship pending remote telemetry crashes"),
        Err(_) => tracing::warn!("timed out shipping pending remote telemetry crashes"),
    }

    Ok(())
}

#[cfg(feature = "remote-telemetry")]
fn target_platform() -> String {
    format!("{}-{}", env::consts::ARCH, env::consts::OS)
}

#[cfg(feature = "heap-profile")]
fn start_heap_profiler(path: Option<&std::path::Path>) -> Result<Option<()>> {
    if let Some(path) = path {
        anyhow::bail!(
            "--heap-profile={} is not available in this build because the `turso` dependency already defines the process global allocator; use `--diagnostics-dir` for bounded-memory telemetry",
            path.display()
        );
    }
    Ok(None)
}

#[cfg(not(feature = "heap-profile"))]
fn start_heap_profiler(path: Option<&std::path::Path>) -> Result<Option<()>> {
    if let Some(path) = path {
        anyhow::bail!(
            "--heap-profile={} requires building modde-cli with `--features heap-profile`",
            path.display()
        );
    }
    Ok(None)
}

fn run_command(cli: Cli) -> Result<()> {
    // Commands that touch the (async) database are dispatched through a tokio
    // runtime via `block_on`. Purely-synchronous commands (no DB, no network)
    // still run directly without a runtime. Only the single matched arm runs,
    // so at most one runtime is created per invocation.
    macro_rules! db_sync {
        ($fut:expr) => {
            return tokio::runtime::Runtime::new()?.block_on($fut)
        };
    }

    match cli.command {
        Commands::Dev {
            action: DevAction::ExportToolSchema { out },
        } => return commands::nix_schema::handle_export(&out),
        #[cfg(feature = "screenshot")]
        Commands::Dev {
            action:
                DevAction::Screenshot {
                    screen,
                    out,
                    all,
                    width,
                    height,
                    scale,
                    theme,
                },
        } => {
            let opts = modde_ui::screenshot::ShotOptions {
                width,
                height,
                scale,
                theme,
            };
            if all {
                for shot in modde_ui::screenshot::all_screens() {
                    let path = out.join(format!("{}.png", shot.as_str()));
                    modde_ui::screenshot::capture_to_png(*shot, &opts, &path)?;
                    println!("wrote {}", path.display());
                }
            } else {
                modde_ui::screenshot::capture_to_png(screen.into(), &opts, &out)?;
                println!("wrote {}", out.display());
            }
            return Ok(());
        }
        Commands::Config { action } => {
            return match action {
                ConfigAction::Show => commands::config::handle_show(),
                ConfigAction::Test => db_sync!(commands::config::handle_test()),
                ConfigAction::SetDatabase {
                    backend,
                    url,
                    host,
                    port,
                    dbname,
                    user,
                    password_file,
                    clear,
                } => commands::config::handle_set_database(
                    &backend,
                    url,
                    host,
                    port,
                    dbname,
                    user,
                    password_file,
                    &clear,
                ),
                ConfigAction::ResetDatabase => commands::config::handle_reset_database(),
            };
        }
        Commands::Profile { action } => db_sync!(commands::profile::handle(action)),
        Commands::Scan {
            game,
            game_dir,
            manifest,
            import_to,
            threshold,
            dry_run,
            prune_duplicates,
        } => {
            db_sync!(commands::scan::handle(
                game,
                game_dir,
                manifest,
                import_to,
                threshold,
                dry_run,
                prune_duplicates,
            ));
        }
        Commands::Diagnostics { game, profile } => {
            db_sync!(commands::diagnostics::handle(&game, profile));
        }
        Commands::Export {
            profile,
            game,
            columns,
            output,
        } => {
            db_sync!(commands::export::handle(profile, game, columns, output));
        }
        Commands::Backup { action } => {
            db_sync!(commands::backup::handle(action));
        }
        Commands::Detect => return commands::detect::handle(),
        Commands::Game {
            action: GameAction::List,
        } => return commands::game::handle_list(),
        Commands::Game {
            action: GameAction::Remove { id, yes },
        } => return commands::game::handle_remove(&id, yes),
        Commands::Game {
            action: GameAction::Detect { install_path },
        } => return commands::game::handle_detect(install_path),
        Commands::Game {
            action: GameAction::Show { id },
        } => return commands::game::handle_show(&id),
        Commands::Game {
            action:
                GameAction::Export {
                    id,
                    with_optiscaler,
                    output,
                },
        } => return commands::game::handle_export(&id, with_optiscaler, output),
        Commands::Game {
            action: GameAction::Import { path, force },
        } => return commands::game::handle_import(path, force),
        Commands::Game {
            action: GameAction::ImportProfile { path, game, force },
        } => return commands::game::handle_import_profile(path, game, force),
        Commands::Instance { action } => {
            return match action {
                InstanceAction::Create { name, data_dir } => {
                    commands::instance::handle_create(&name, data_dir)
                }
                InstanceAction::List => commands::instance::handle_list(),
                InstanceAction::Switch { name } => commands::instance::handle_switch(&name),
            };
        }
        Commands::Import => db_sync!(commands::import::handle()),
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
            db_sync!(commands::tool::handle_list(&game));
        }
        Commands::Tool {
            action: ToolAction::Status { game },
        } => {
            db_sync!(commands::tool::handle_status(&game));
        }
        Commands::Tool {
            action: ToolAction::Enable { tool_id, game },
        } => {
            db_sync!(commands::tool::handle_enable(&tool_id, &game));
        }
        Commands::Tool {
            action: ToolAction::Disable { tool_id, game },
        } => {
            db_sync!(commands::tool::handle_disable(&tool_id, &game));
        }
        Commands::Tool {
            action:
                ToolAction::Configure {
                    tool_id,
                    game,
                    settings,
                },
        } => {
            db_sync!(commands::tool::handle_configure(&tool_id, &game, &settings));
        }
        Commands::Tool {
            action: ToolAction::Apply { tool_id, game },
        } => {
            db_sync!(commands::tool::handle_apply(&tool_id, &game));
        }
        Commands::Tool {
            action: ToolAction::Revert { tool_id, game },
        } => {
            db_sync!(commands::tool::handle_revert(&tool_id, &game));
        }
        Commands::Nxm {
            action: NxmAction::Install,
        } => {
            commands::nxm::install_handler()?;
            return Ok(());
        }
        Commands::Exec {
            action:
                ExecAction::Add {
                    name,
                    executable,
                    game,
                    working_dir,
                    output_mod,
                    wine_dll_overrides,
                    environment,
                    args,
                },
        } => {
            db_sync!(commands::tool::handle_add_executable(
                &name,
                executable,
                &game,
                working_dir,
                &output_mod,
                wine_dll_overrides,
                &environment,
                &args,
            ));
        }
        Commands::Exec {
            action: ExecAction::List { game },
        } => {
            db_sync!(commands::tool::handle_list_executables(&game));
        }
        Commands::Exec {
            action: ExecAction::Remove { name, game },
        } => {
            db_sync!(commands::tool::handle_remove_executable(&name, &game));
        }
        Commands::Skill { action } => {
            return commands::skill::handle(action);
        }
        Commands::Wabbajack { .. } => {}
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
                commands::rollback::handle(profile, game).await?;
            }
            Commands::Install { source } => commands::install::handle(source).await?,
            Commands::Mod { action } => match action {
                ModAction::Remove { mod_id, profile } => {
                    commands::uninstall::handle(mod_id, profile).await?;
                }
                ModAction::Diagnose { mod_id } => {
                    commands::uninstall::handle_diagnose(mod_id).await?;
                }
            },
            Commands::Verify { profile, game } => commands::verify::handle(profile, game).await?,
            Commands::Nexus { action } => commands::nexus::handle(action).await?,
            Commands::Stock { action } => commands::stock::handle(action).await?,
            Commands::Save { action } => commands::save::handle(action).await?,
            Commands::Game {
                action:
                    GameAction::Add {
                        id,
                        display_name,
                        executable_dir,
                        steam_app_id,
                        install_dir_name,
                        mod_dir,
                        nexus_domain,
                        proxy_dlls,
                        force,
                    },
            } => {
                commands::game::handle_add(commands::game::AddGameArgs {
                    id,
                    display_name,
                    executable_dir,
                    steam_app_id,
                    install_dir_name,
                    mod_dir,
                    nexus_domain,
                    proxy_dlls,
                    force,
                })
                .await?;
            }
            Commands::Update { action } => match action {
                UpdateAction::Check {
                    profile,
                    game,
                    period,
                    mods,
                } => {
                    if mods || profile.is_some() || game.is_some() {
                        commands::update::handle_check(profile, game, period).await?;
                    } else {
                        commands::update::handle_product_check().await?;
                    }
                }
                UpdateAction::Apply {
                    profile,
                    game,
                    period,
                    dry_run,
                    confirm_locked,
                    accept_breaking,
                    yes,
                } => {
                    commands::update::handle_apply(commands::update::ApplyOptions {
                        profile_name: profile,
                        game_id: game,
                        period,
                        dry_run,
                        safety: commands::update::ApplySafety {
                            confirm_locked,
                            accept_breaking,
                            yes,
                        },
                    })
                    .await?;
                }
            },
            Commands::Tool { action } => match action {
                ToolAction::Run {
                    executable,
                    profile,
                    game,
                    args,
                } => commands::tool::handle_run(executable, args, profile, game).await?,
                ToolAction::AddExecutable {
                    name,
                    executable,
                    game,
                    working_dir,
                    output_mod,
                    wine_dll_overrides,
                    environment,
                    args,
                } => {
                    commands::tool::handle_add_executable(
                        &name,
                        executable,
                        &game,
                        working_dir,
                        &output_mod,
                        wine_dll_overrides,
                        &environment,
                        &args,
                    )
                    .await?;
                }
                ToolAction::ListExecutables { game } => {
                    commands::tool::handle_list_executables(&game).await?;
                }
                ToolAction::RemoveExecutable { name, game } => {
                    commands::tool::handle_remove_executable(&name, &game).await?;
                }
                ToolAction::RunExecutable {
                    name,
                    game,
                    profile,
                    args,
                } => {
                    commands::tool::handle_run_executable(&name, &game, profile, args).await?;
                }
                ToolAction::Releases { tool_id, game } => {
                    commands::tool::handle_releases(&tool_id, &game).await?;
                }
                ToolAction::InstallRelease {
                    tool_id,
                    game,
                    tag,
                    asset,
                } => {
                    commands::tool::handle_install_release(&tool_id, &game, &tag, &asset).await?;
                }
                ToolAction::InstallReleaseFromPath {
                    tool_id,
                    game,
                    tag,
                    asset,
                    path,
                } => {
                    commands::tool::handle_install_release_from_path(
                        &tool_id, &game, &tag, &asset, path,
                    )
                    .await?;
                }
                ToolAction::List { .. }
                | ToolAction::Status { .. }
                | ToolAction::Enable { .. }
                | ToolAction::Disable { .. }
                | ToolAction::Configure { .. }
                | ToolAction::Apply { .. }
                | ToolAction::Revert { .. } => {
                    unreachable!(
                        "these ToolAction variants are dispatched in the pre-tokio sync block"
                    )
                }
            },
            Commands::Nxm { action } => match action {
                NxmAction::Handle { uri, profile } => commands::nxm::handle(uri, profile).await?,
                NxmAction::Install => {
                    unreachable!("NxmAction::Install is dispatched in the pre-tokio sync block")
                }
            },
            Commands::Exec { action } => match action {
                ExecAction::Run {
                    name,
                    game,
                    profile,
                    args,
                } => {
                    commands::tool::handle_run_executable(&name, &game, profile, args).await?;
                }
                // Sync arms handled in the pre-tokio block above.
                ExecAction::Add { .. } | ExecAction::List { .. } | ExecAction::Remove { .. } => {
                    unreachable!("Exec sync arms are dispatched in the pre-tokio block above")
                }
            },
            Commands::Wabbajack { action } => commands::wabbajack::handle(action).await?,
            // Already handled above
            Commands::Profile { .. }
            | Commands::Config { .. }
            | Commands::Dev { .. }
            | Commands::Scan { .. }
            | Commands::Detect
            | Commands::Game { .. }
            | Commands::Import
            | Commands::Instance { .. }
            | Commands::Backup { .. }
            | Commands::Diagnostics { .. }
            | Commands::Export { .. }
            | Commands::Fomod { .. }
            | Commands::Loot { .. }
            | Commands::Skill { .. }
            | Commands::Gui => {
                unreachable!("these commands are dispatched before the async runtime block")
            }
        }
        Ok(())
    })
}

/// `true` when the command mutates persistent profile / store / DB
/// state and a running GUI should re-read the DB after it finishes.
///
/// Centralized so we have one place to audit. Read-only commands
/// (`Detect`, `Diagnostics`, `Export`, `Verify`, `update check`, …)
/// return `false` to avoid spamming GUIs with no-op refreshes.
fn command_mutates_state(cmd: &Commands) -> bool {
    match cmd {
        // Pure read paths.
        Commands::Dev { .. }
        | Commands::Detect
        | Commands::Diagnostics { .. }
        | Commands::Export { .. }
        | Commands::Verify { .. }
        | Commands::Collisions { .. }
        | Commands::Gui => false,

        Commands::Config { action } => matches!(
            action,
            ConfigAction::SetDatabase { .. } | ConfigAction::ResetDatabase
        ),

        Commands::Game { action } => {
            matches!(
                action,
                GameAction::Add { .. }
                    | GameAction::Remove { .. }
                    | GameAction::Import { .. }
                    | GameAction::ImportProfile { .. }
            )
        }

        // `update check` is read-only; `update apply` mutates.
        Commands::Update { action } => matches!(action, UpdateAction::Apply { .. }),

        // `instance list` is read-only; create/switch flip the active
        // data dir.
        Commands::Instance { action } => !matches!(action, InstanceAction::List),

        // Loot validate just reports, sort rewrites the load order.
        Commands::Loot { action } => matches!(action, LootAction::Sort { .. }),

        // Tool subcommands: queries are read-only, everything else
        // mutates per-game tool config rows.
        Commands::Tool { action } => !matches!(
            action,
            ToolAction::List { .. }
                | ToolAction::Status { .. }
                | ToolAction::Releases { .. }
                | ToolAction::ListExecutables { .. }
        ),

        // Exec is the alias for tool's executable subset. List is a
        // read-only query; the others mutate or run a child process
        // whose overwrite-capture writes to the store.
        Commands::Exec { action } => !matches!(action, ExecAction::List { .. }),

        // Skill list/path are read-only; install writes files under the
        // user-global `.agents/skills` tree.
        Commands::Skill { action } => matches!(action, SkillAction::Install { .. }),

        // Nexus: `auth` writes the API key, `status` prints validity.
        // The status arm doesn't mutate, but the GUI surfaces auth
        // state — a refresh is harmless and cheap. Notify on either.
        Commands::Nexus { .. } => true,

        // `mod diagnose` only prints the dossier; remove mutates.
        Commands::Mod { action } => matches!(action, ModAction::Remove { .. }),

        // `wabbajack search` / `download` / `hm-snippet` / `assess` /
        // `missing-impact` / `manual-links` are read-only;
        // import/acquire commands write into the store.
        Commands::Wabbajack { action } => matches!(
            action,
            WabbajackAction::ImportArchive { .. } | WabbajackAction::AcquireMissing { .. }
        ),

        // Save commands cover read-only listing AND mutating
        // capture/restore/adopt; conservatively notify on all of
        // them — the cost is one socket round-trip per active GUI.
        Commands::Save { .. } => true,

        // `nxm install` registers the URI handler (system-side). We
        // still notify so a GUI showing nxm settings can reflect it.
        Commands::Nxm { .. } => true,

        // Stock snapshots affect deploy decisions; treat as mutating.
        Commands::Stock { .. } => true,

        // Backup capture/restore mutates the data dir.
        Commands::Backup { .. } => true,

        // Everything below is unambiguously mutating.
        Commands::Profile { .. }
        | Commands::Scan { .. }
        | Commands::Import
        | Commands::Fomod { .. }
        | Commands::Install { .. }
        | Commands::Deploy { .. }
        | Commands::Rollback { .. }
        | Commands::Play { .. } => true,
    }
}

fn command_runs_lazy_product_update_check(cmd: &Commands) -> bool {
    !matches!(
        cmd,
        Commands::Gui
            | Commands::Config { .. }
            | Commands::Dev { .. }
            | Commands::Update {
                action: UpdateAction::Check { .. }
            }
    )
}

fn maybe_print_product_update_notice() {
    let settings = modde_core::settings::AppSettings::load();
    if !modde_core::update_check::update_checks_enabled(&settings) {
        return;
    }

    let Ok(runtime) = tokio::runtime::Runtime::new() else {
        return;
    };
    match runtime.block_on(modde_core::update_check::check_latest_with_settings(
        &settings,
    )) {
        Ok(Some(update)) => {
            eprintln!(
                "modde update available: {} (current: {}) - {}",
                update.latest_version, update.current_version, update.release_url
            );
        }
        Ok(None) => {}
        Err(error) => tracing::debug!(%error, "product update check failed"),
    }
}

#[cfg(test)]
mod mutation_classification_tests {
    //! Tests for [`command_mutates_state`].
    //!
    //! Goal: lock in which commands push a refresh signal to running
    //! GUIs. A wrong classification has user-visible consequences:
    //!
    //! * False negative (mutating cmd marked read-only) → GUI misses
    //!   an update; user sees stale state until they click around.
    //! * False positive (read-only cmd marked mutating) → harmless
    //!   socket round-trip per active GUI, but adds noise.
    //!
    //! When a new top-level [`Commands`] variant is added, the
    //! exhaustive `match` in `command_mutates_state` will not compile
    //! until you classify it — at which point a test in this module
    //! should pin the answer.
    use super::*;
    use std::path::PathBuf;

    fn list_profiles() -> Commands {
        Commands::Profile {
            action: ProfileAction::List { game: None },
        }
    }

    fn create_profile() -> Commands {
        Commands::Profile {
            action: ProfileAction::Create {
                name: "p".into(),
                game: "skyrim-se".into(),
            },
        }
    }

    fn install_mod() -> Commands {
        Commands::Install {
            source: InstallSource::Mod {
                url: "https://www.nexusmods.com/skyrimspecialedition/mods/1".into(),
                profile: None,
                fomod_config: None,
            },
        }
    }

    fn update_check() -> Commands {
        Commands::Update {
            action: UpdateAction::Check {
                profile: None,
                game: None,
                period: "1w".into(),
                mods: false,
            },
        }
    }

    fn update_apply() -> Commands {
        Commands::Update {
            action: UpdateAction::Apply {
                profile: None,
                game: None,
                period: "1w".into(),
                dry_run: false,
                confirm_locked: false,
                accept_breaking: false,
                yes: false,
            },
        }
    }

    fn instance_list() -> Commands {
        Commands::Instance {
            action: InstanceAction::List,
        }
    }

    fn instance_create() -> Commands {
        Commands::Instance {
            action: InstanceAction::Create {
                name: "x".into(),
                data_dir: PathBuf::from("/tmp/x"),
            },
        }
    }

    fn loot_validate() -> Commands {
        Commands::Loot {
            action: LootAction::Validate {
                game: "skyrim-se".into(),
            },
        }
    }

    fn loot_sort() -> Commands {
        Commands::Loot {
            action: LootAction::Sort {
                game: "skyrim-se".into(),
                data_dir: None,
            },
        }
    }

    fn tool_list() -> Commands {
        Commands::Tool {
            action: ToolAction::List {
                game: "skyrim-se".into(),
            },
        }
    }

    fn tool_apply() -> Commands {
        Commands::Tool {
            action: ToolAction::Apply {
                tool_id: "mangohud".into(),
                game: "skyrim-se".into(),
            },
        }
    }

    fn mod_remove() -> Commands {
        Commands::Mod {
            action: ModAction::Remove {
                mod_id: "x".into(),
                profile: None,
            },
        }
    }

    fn mod_diagnose() -> Commands {
        Commands::Mod {
            action: ModAction::Diagnose { mod_id: "x".into() },
        }
    }

    fn wabbajack_search() -> Commands {
        Commands::Wabbajack {
            action: WabbajackAction::Search {
                query: None,
                game: None,
                source: "both".into(),
                json: false,
            },
        }
    }

    fn wabbajack_import() -> Commands {
        Commands::Wabbajack {
            action: WabbajackAction::ImportArchive {
                manifest: PathBuf::from("m.json"),
                archives: vec![],
            },
        }
    }

    fn wabbajack_acquire_missing() -> Commands {
        Commands::Wabbajack {
            action: WabbajackAction::AcquireMissing {
                manifest: PathBuf::from("m.json"),
                download_dir: None,
                data_dir: None,
                browser_profile: None,
                include_nexus: false,
                browser_controller: false,
                timeout: 1,
                json: false,
            },
        }
    }

    fn wabbajack_assess() -> Commands {
        Commands::Wabbajack {
            action: WabbajackAction::Assess {
                manifest: PathBuf::from("list.wabbajack"),
                profile: None,
                game_dir: None,
                json: false,
            },
        }
    }

    fn skill_list() -> Commands {
        Commands::Skill {
            action: SkillAction::List,
        }
    }

    fn skill_install() -> Commands {
        Commands::Skill {
            action: SkillAction::Install {
                name: "all".into(),
                force: false,
            },
        }
    }

    fn game_add() -> Commands {
        Commands::Game {
            action: GameAction::Add {
                id: "custom-game".into(),
                display_name: "Custom Game".into(),
                executable_dir: PathBuf::from("bin"),
                steam_app_id: None,
                install_dir_name: None,
                mod_dir: None,
                nexus_domain: None,
                proxy_dlls: vec![],
                force: false,
            },
        }
    }

    fn game_remove() -> Commands {
        Commands::Game {
            action: GameAction::Remove {
                id: "custom-game".into(),
                yes: false,
            },
        }
    }

    // ── read-only ────────────────────────────────────────────────

    #[test]
    fn detect_is_read_only() {
        assert!(!command_mutates_state(&Commands::Detect));
    }

    #[test]
    fn diagnostics_is_read_only() {
        assert!(!command_mutates_state(&Commands::Diagnostics {
            game: "skyrim-se".into(),
            profile: None,
        }));
    }

    #[test]
    fn export_is_read_only() {
        assert!(!command_mutates_state(&Commands::Export {
            profile: None,
            game: None,
            columns: None,
            output: None,
        }));
    }

    #[test]
    fn verify_is_read_only() {
        assert!(!command_mutates_state(&Commands::Verify {
            profile: None,
            game: None,
        }));
    }

    #[test]
    fn collisions_is_read_only() {
        assert!(!command_mutates_state(&Commands::Collisions {
            profile: None,
            game: None,
            all: false,
            suggest_hides: false,
        }));
    }

    #[test]
    fn gui_is_read_only() {
        // The GUI command launches the GUI itself; pushing to
        // ourselves at startup would be confusing and pointless.
        assert!(!command_mutates_state(&Commands::Gui));
    }

    #[test]
    fn update_check_is_read_only() {
        assert!(!command_mutates_state(&update_check()));
    }

    #[test]
    fn instance_list_is_read_only() {
        assert!(!command_mutates_state(&instance_list()));
    }

    #[test]
    fn loot_validate_is_read_only() {
        assert!(!command_mutates_state(&loot_validate()));
    }

    #[test]
    fn tool_list_is_read_only() {
        assert!(!command_mutates_state(&tool_list()));
    }

    #[test]
    fn mod_diagnose_is_read_only() {
        assert!(!command_mutates_state(&mod_diagnose()));
    }

    #[test]
    fn wabbajack_search_is_read_only() {
        assert!(!command_mutates_state(&wabbajack_search()));
    }

    #[test]
    fn wabbajack_assess_is_read_only() {
        assert!(!command_mutates_state(&wabbajack_assess()));
    }

    #[test]
    fn skill_list_is_read_only() {
        assert!(!command_mutates_state(&skill_list()));
    }

    #[test]
    fn game_list_is_read_only() {
        assert!(!command_mutates_state(&Commands::Game {
            action: GameAction::List,
        }));
    }

    #[test]
    fn game_detect_is_read_only() {
        assert!(!command_mutates_state(&Commands::Game {
            action: GameAction::Detect {
                install_path: PathBuf::from("/games"),
            },
        }));
    }

    #[test]
    fn game_show_is_read_only() {
        assert!(!command_mutates_state(&Commands::Game {
            action: GameAction::Show {
                id: "custom-game".into(),
            },
        }));
    }

    #[test]
    fn game_export_is_read_only() {
        assert!(!command_mutates_state(&Commands::Game {
            action: GameAction::Export {
                id: "custom-game".into(),
                with_optiscaler: false,
                output: None,
            },
        }));
    }

    // ── mutating ─────────────────────────────────────────────────

    #[test]
    fn profile_list_is_mutating() {
        // Pinned as `mutating` in `command_mutates_state` because
        // the dispatcher routes both List and create/delete through
        // the same `commands::profile::handle` and we don't peek
        // inside the action enum. Document that here; if we later
        // refine the classification, flip this assertion.
        assert!(command_mutates_state(&list_profiles()));
    }

    #[test]
    fn profile_create_is_mutating() {
        assert!(command_mutates_state(&create_profile()));
    }

    #[test]
    fn install_is_mutating() {
        assert!(command_mutates_state(&install_mod()));
    }

    #[test]
    fn update_apply_is_mutating() {
        assert!(command_mutates_state(&update_apply()));
    }

    #[test]
    fn instance_create_is_mutating() {
        assert!(command_mutates_state(&instance_create()));
    }

    #[test]
    fn loot_sort_is_mutating() {
        assert!(command_mutates_state(&loot_sort()));
    }

    #[test]
    fn tool_apply_is_mutating() {
        assert!(command_mutates_state(&tool_apply()));
    }

    #[test]
    fn mod_remove_is_mutating() {
        assert!(command_mutates_state(&mod_remove()));
    }

    #[test]
    fn wabbajack_import_archive_is_mutating() {
        assert!(command_mutates_state(&wabbajack_import()));
    }

    #[test]
    fn wabbajack_acquire_missing_is_mutating() {
        assert!(command_mutates_state(&wabbajack_acquire_missing()));
    }

    #[test]
    fn skill_install_is_mutating() {
        assert!(command_mutates_state(&skill_install()));
    }

    #[test]
    fn game_add_is_mutating() {
        assert!(command_mutates_state(&game_add()));
    }

    #[test]
    fn game_remove_is_mutating() {
        assert!(command_mutates_state(&game_remove()));
    }

    #[test]
    fn deploy_is_mutating() {
        assert!(command_mutates_state(&Commands::Deploy {
            profile: None,
            game: None,
        }));
    }

    #[test]
    fn rollback_is_mutating() {
        assert!(command_mutates_state(&Commands::Rollback {
            profile: None,
            game: None,
        }));
    }

    #[test]
    fn import_is_mutating() {
        assert!(command_mutates_state(&Commands::Import));
    }

    #[test]
    fn stock_is_mutating() {
        assert!(command_mutates_state(&Commands::Stock {
            action: StockAction::Snapshot {
                game_id: "skyrim-se".into(),
            },
        }));
    }
}
