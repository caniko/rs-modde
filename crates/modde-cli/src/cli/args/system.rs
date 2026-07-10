//! System, config, instance, skill, and executable CLI actions.

use std::path::PathBuf;

use clap::Subcommand;

use crate::commands;

#[derive(Subcommand)]
pub(crate) enum ExecAction {
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
pub(crate) enum DevAction {
    /// Generate shell completion scripts (bash, zsh, fish, powershell, nushell)
    Completions {
        /// Shell to generate completions for
        shell: String,
    },
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
        /// `HiDPI` scale factor (2.0 = crisp).
        #[arg(long, default_value_t = 2.0)]
        scale: f32,
        /// modde theme name (Dark, Light, Dracula, Nord, ...).
        #[arg(long, default_value = "Dark")]
        theme: String,
    },
}

#[cfg(feature = "screenshot")]
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum ScreenArg {
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
pub(crate) enum ConfigAction {
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
pub(crate) enum SkillAction {
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
pub(crate) enum InstanceAction {
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
pub(crate) enum NxmAction {
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
