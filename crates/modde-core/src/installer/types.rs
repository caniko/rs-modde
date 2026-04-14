//! Core data types for the mod installer pipeline.
//!
//! An install is a three-stage process:
//!
//! 1. **Analyze** — inspect an extracted archive and decide how to stage it.
//!    Produces an [`InstallPlan`].
//! 2. **Execute** — move files from the staging dir into the mod's store dir
//!    per the plan, producing the concrete [`StagedFile`] list.
//! 3. **Record** — persist the method + file manifest into the database so
//!    uninstalls can be precise.
//!
//! The [`InstallMethod`] enum is the extensibility point: when detection
//! cannot classify a mod, callers dump a dossier and a Claude Code skill
//! extends this enum with a new variant + detection rule.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How a mod's files should be laid out in the store.
///
/// Variants are ordered by detection specificity — game-specific layouts
/// (REDmod) take precedence over generic ones (BareExtract) so that e.g.
/// a Cyberpunk mod containing both `info.json` and a `Data/` folder is
/// identified as REDmod rather than a Bethesda bare-extract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallMethod {
    /// Archive contents map 1:1 to the game's mod dir (e.g. loose files
    /// that drop straight into Skyrim's `Data/`).
    BareExtract,

    /// FOMOD installer. `module_config` points at the archive-relative
    /// `fomod/ModuleConfig.xml`. `config_toml` is a TOML-serialized
    /// `fomod_oxide::DeclarativeConfig` describing the chosen options —
    /// `None` means the UI wizard still has to run before execution.
    Fomod {
        module_config: PathBuf,
        config_toml: Option<String>,
    },

    /// REDmod package (Cyberpunk 2077). `manifest` is the archive-relative
    /// `info.json` that REDmod ships with.
    REDmod { manifest: PathBuf },

    /// BAIN (Wrye Bash) layout: numbered option subdirs like `00 Core`,
    /// `01 Option`. `selected_subdirs` lists which options to stage —
    /// `Vec::new()` means the UI wizard still has to run.
    Bain { selected_subdirs: Vec<String> },

    /// Proxy DLL / overlay (e.g. dxvk, ENB). Files go into the game's
    /// `executable_dir`. `target_dir_hint` is a human-readable hint like
    /// `"game root"` used by the UI.
    DllOverlay { target_dir_hint: String },

    /// Placeholder for future script-merge support. `merge_group` is an
    /// identifier shared by every mod that participates in the same merge;
    /// `base` is the install method that would apply without the merge.
    /// Execution today just stages files per `base` and tags them with
    /// the merge group — actual merging will be wired in later.
    ScriptMerge {
        merge_group: String,
        base: Box<InstallMethod>,
    },

    /// Detection failed. `reason` is a short human-readable string and a
    /// dossier has been (or should be) written so a skill can extend the
    /// installer to handle this layout.
    Unknown { reason: String },
}

impl InstallMethod {
    /// Short label used in logs and the UI.
    pub fn label(&self) -> &'static str {
        match self {
            InstallMethod::BareExtract => "bare",
            InstallMethod::Fomod { .. } => "fomod",
            InstallMethod::REDmod { .. } => "redmod",
            InstallMethod::Bain { .. } => "bain",
            InstallMethod::DllOverlay { .. } => "dll-overlay",
            InstallMethod::ScriptMerge { .. } => "script-merge",
            InstallMethod::Unknown { .. } => "unknown",
        }
    }

    /// `true` if `execute` can proceed without any further user input.
    pub fn is_ready(&self) -> bool {
        match self {
            InstallMethod::BareExtract
            | InstallMethod::REDmod { .. }
            | InstallMethod::DllOverlay { .. } => true,
            InstallMethod::Fomod { config_toml, .. } => config_toml.is_some(),
            InstallMethod::Bain { selected_subdirs, .. } => !selected_subdirs.is_empty(),
            InstallMethod::ScriptMerge { base, .. } => base.is_ready(),
            InstallMethod::Unknown { .. } => false,
        }
    }
}

/// A complete install description produced by [`analyze`](super::analyze) and
/// consumed by [`execute`](super::execute).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
    pub method: InstallMethod,

    /// If `Some`, the analyzer detected a wrapper directory (e.g.
    /// `ModName-1.0/...`) and everything in [`method`](Self::method) and
    /// [`staged_files`](Self::staged_files) is relative to
    /// `extracted_dir.join(strip_prefix)` rather than `extracted_dir`.
    /// Executes by recursing into that subdir.
    #[serde(default)]
    pub strip_prefix: Option<PathBuf>,

    /// xxh64 of the original downloaded archive. Used by the dossier dump
    /// and for future dedup / verification.
    pub source_archive_hash: String,

    /// The concrete file manifest for this mod, populated by `execute`.
    /// Empty until execution completes.
    #[serde(default)]
    pub staged_files: Vec<StagedFile>,
}

/// A single file staged into a mod's store directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StagedFile {
    /// Path relative to the mod's store directory — the canonical key
    /// used by uninstall.
    pub rel_path: String,

    /// Path inside the original archive (before any `strip_prefix` or
    /// FOMOD selection). Useful for reproducing the install, and for the
    /// dossier dump when something goes wrong later.
    pub origin_rel_path: String,

    pub size: u64,

    /// Reserved for future script-merge work. When `Some`, this file
    /// participated in a named merge group; uninstall still removes it
    /// normally but a future re-merge command can replay the group.
    #[serde(default)]
    pub merge_group: Option<String>,
}

/// Persistent install state for a mod row.
///
/// Stored in `profile_mods.install_status` as a lowercase string. Used by
/// the UI to show the correct action button (install / retry / resume).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    /// Files are staged and tracked in `installed_mod_files`.
    Installed,
    /// Detection failed; dossier written. User/skill must extend the
    /// installer before retry is possible.
    Unknown,
    /// Detection succeeded but execution needs user input (FOMOD wizard,
    /// BAIN option picker). The archive is extracted to a staging dir.
    PendingUserInput,
    /// Execution started but failed partway. Store dir may hold partial
    /// files; uninstall should clean them up before retry.
    Failed,
}

impl InstallStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            InstallStatus::Installed => "installed",
            InstallStatus::Unknown => "unknown",
            InstallStatus::PendingUserInput => "pending_user_input",
            InstallStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "installed" => Some(Self::Installed),
            "unknown" => Some(Self::Unknown),
            "pending_user_input" => Some(Self::PendingUserInput),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Errors raised by [`analyze`](super::analyze) and
/// [`execute`](super::execute).
#[derive(Debug, thiserror::Error)]
pub enum InstallerError {
    #[error("I/O error during install: {0}")]
    Io(#[from] std::io::Error),

    #[error("archive extraction failed: {0}")]
    Extract(String),

    #[error("installer requires user input ({method}) — run the wizard first")]
    RequiresUserInput { method: &'static str },

    #[error("unknown install method: {reason}")]
    UnknownMethod { reason: String },

    #[error("plan references a file that is not present in the staging dir: {0}")]
    MissingFile(String),

    #[error("FOMOD installer error: {0}")]
    FomodError(String),
}

pub type InstallerResult<T> = std::result::Result<T, InstallerError>;
