//! Profile, backup, and save CLI actions.

use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum BackupAction {
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
pub(crate) enum ProfileAction {
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
pub(crate) enum SaveAction {
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
