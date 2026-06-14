//! Lockfile, installer source, Nexus, FOMOD, and Wabbajack CLI actions.

use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum LockAction {
    /// Export one profile to a portable JSON lockfile.
    Export {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        game: String,
        #[arg(long, default_value = "modde.lock")]
        output: PathBuf,
        /// Emit a diagnostic lock even when required provenance is missing.
        #[arg(long)]
        allow_incomplete: bool,
    },
    /// Verify lock signatures and tracked files against local disk.
    Verify {
        path: PathBuf,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
    /// Append an Ed25519 signature to a lockfile.
    Sign {
        path: PathBuf,
        #[arg(long)]
        secret_key: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Generate an Ed25519 key pair for lock signing.
    Keygen {
        #[arg(long)]
        public: PathBuf,
        #[arg(long)]
        secret: PathBuf,
    },
    /// Validate and import lock metadata into a profile.
    Import {
        path: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        game: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum InstallSource {
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
pub(crate) enum NexusAction {
    /// Save API key to keyring
    Auth,
    /// Show API key validity and premium status
    Status,
}

#[derive(Subcommand)]
pub(crate) enum FomodAction {
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
