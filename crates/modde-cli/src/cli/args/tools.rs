//! Patcher and external tool CLI actions.

use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum PatcherAction {
    /// List patcher stages for a profile.
    List {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
    /// Add or update a Synthesis CLI stage.
    AddSynthesis {
        name: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        #[arg(long)]
        executable: PathBuf,
        #[arg(long)]
        pipeline_settings: PathBuf,
        #[arg(long)]
        synthesis_profile: String,
        #[arg(long)]
        output_mod: String,
        #[arg(long)]
        order: i64,
        #[arg(long, default_value_t = modde_core::patcher::DEFAULT_PATCHER_TIMEOUT_SECONDS)]
        timeout_seconds: u64,
    },
    /// Add or update a generic command stage.
    AddCommand {
        name: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        #[arg(long)]
        executable: PathBuf,
        #[arg(long)]
        working_dir: Option<PathBuf>,
        #[arg(long = "arg", allow_hyphen_values = true)]
        args: Vec<String>,
        #[arg(long = "env")]
        environment: Vec<String>,
        #[arg(long)]
        output_mod: String,
        #[arg(long, default_value_t = -1)]
        order: i64,
        #[arg(long, default_value_t = modde_core::patcher::DEFAULT_PATCHER_TIMEOUT_SECONDS)]
        timeout_seconds: u64,
    },
    /// Remove a patcher stage.
    Remove {
        name: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
    /// Enable a patcher stage.
    Enable {
        name: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
    /// Disable a patcher stage.
    Disable {
        name: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
    /// Replace patcher stage order with the supplied stage names.
    Reorder {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        names: Vec<String>,
    },
    /// Validate enabled patcher stages without running them.
    Validate {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
    /// Run one stage by name, or all enabled stages when no name is supplied.
    Run {
        name: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum ToolAction {
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
