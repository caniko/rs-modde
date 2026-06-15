#![allow(clippy::wildcard_imports)]
//! Shared Wabbajack readiness assessment for CLI and GUI flows.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use modde_core::manifest::wabbajack::{
    ArchiveEntry, ArchiveState, RawDirective, WabbajackManifest,
};
use serde::Serialize;

use super::runner::parse_wabbajack_manifest;
use super::staging::StagingStore;

/// Inputs for a Wabbajack readiness assessment.
#[derive(Debug, Clone)]
pub struct WabbajackReadinessOptions {
    pub manifest_path: PathBuf,
    pub profile_name: Option<String>,
    pub game_dir: Option<PathBuf>,
    pub store_dir: PathBuf,
    pub staging_root: PathBuf,
    /// Override Nexus credential detection. Tests use this for deterministic
    /// results; production callers leave it as `None`.
    pub nexus_credentials_available: Option<bool>,
}

impl WabbajackReadinessOptions {
    #[must_use]
    pub fn new(manifest_path: impl Into<PathBuf>) -> Self {
        Self {
            manifest_path: manifest_path.into(),
            profile_name: None,
            game_dir: None,
            store_dir: modde_core::paths::store_dir(),
            staging_root: modde_core::paths::staging_dir(),
            nexus_credentials_available: None,
        }
    }
}

/// Readiness report consumed by both the CLI and GUI.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WabbajackReadinessReport {
    pub manifest_path: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub game: String,
    pub normalized_game: String,
    pub profile_name: String,
    pub store_path: String,
    pub archives: usize,
    pub directives: usize,
    pub archive_states: BTreeMap<String, usize>,
    pub directive_types: BTreeMap<String, usize>,
    pub archive_extensions: BTreeMap<String, usize>,
    pub downloadable_archives: usize,
    pub store_present: usize,
    pub store_missing: Vec<WabbajackReadinessArchive>,
    pub manual_downloads: Vec<WabbajackManualArchive>,
    pub missing_nexus_archives: Vec<WabbajackReadinessArchive>,
    pub game_file_sources: WabbajackGameFileSourceReport,
    pub staging: WabbajackStagingReport,
    pub rar_enabled: bool,
    pub nexus_required: bool,
    pub nexus_available: bool,
    pub hard_blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub install_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WabbajackReadinessArchive {
    pub hash: String,
    pub name: String,
    pub state: String,
    pub store_path: String,
    pub source: Option<String>,
    pub remediation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WabbajackManualArchive {
    pub hash: String,
    pub name: String,
    pub url: String,
    pub prompt: String,
    pub store_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WabbajackGameFileSourceReport {
    pub total: usize,
    pub present: usize,
    pub missing: Vec<String>,
    pub mismatched: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WabbajackStagingReport {
    pub path: String,
    pub exists: bool,
    pub compatible_layout: bool,
    pub archive_batch_sentinels: usize,
    pub archive_batch_total: usize,
    pub create_bsa_sentinels: usize,
    pub create_bsa_total: usize,
    pub layout_action: String,
}

/// Assess whether a Wabbajack modlist can be safely installed now.
///
/// This function does not download archives or mutate staging. It parses the
/// manifest, verifies configured game-file sources by hash, checks the local
/// store for manual/Nexus prerequisites, and reports resumable staging status.
pub async fn assess_wabbajack_readiness(
    options: WabbajackReadinessOptions,
) -> Result<WabbajackReadinessReport> {
    let manifest = parse_wabbajack_manifest(&options.manifest_path)?;
    assess_manifest_readiness(options, manifest).await
}

async fn assess_manifest_readiness(
    options: WabbajackReadinessOptions,
    manifest: WabbajackManifest,
) -> Result<WabbajackReadinessReport> {
    let profile_name = options
        .profile_name
        .clone()
        .unwrap_or_else(|| manifest.name.clone());
    let staging_path = options.staging_root.join(&profile_name);
    let store = options.store_dir;
    let mut archive_states = BTreeMap::new();
    let mut archive_extensions = BTreeMap::new();
    let mut downloadable_archives = 0;
    let mut store_present = 0;
    let mut store_missing = Vec::new();
    let mut manual_downloads = Vec::new();
    let mut missing_nexus_archives = Vec::new();
    let mut hard_blockers = Vec::new();
    let mut warnings = Vec::new();

    for archive in &manifest.archives {
        *archive_states
            .entry(archive_state_label(archive.state.as_ref()))
            .or_insert(0) += 1;
        *archive_extensions
            .entry(archive_extension(&archive.name))
            .or_insert(0) += 1;

        if matches!(
            archive.state,
            Some(ArchiveState::GameFileSourceDownloader { .. })
        ) {
            continue;
        }

        downloadable_archives += 1;
        let store_path = archive_store_path(&store, archive.hash);
        if store_path.exists() {
            store_present += 1;
            continue;
        }

        let missing = missing_archive_report(archive, &store);
        match &archive.state {
            Some(ArchiveState::ManualDownloader { url, prompt }) => {
                manual_downloads.push(WabbajackManualArchive {
                    hash: format!("{:016x}", archive.hash),
                    name: archive.name.clone(),
                    url: url.clone(),
                    prompt: prompt.clone(),
                    store_path: store_path.display().to_string(),
                });
            }
            Some(ArchiveState::NexusDownloader { .. }) => {
                missing_nexus_archives.push(missing.clone());
            }
            None => {
                hard_blockers.push(format!(
                    "Archive '{}' ({:016x}) is missing from the store and has no downloader metadata",
                    archive.name, archive.hash
                ));
            }
            _ => {}
        }
        store_missing.push(missing);
    }

    let mut directive_types = BTreeMap::new();
    for directive in &manifest.directives {
        *directive_types
            .entry(directive_label(directive))
            .or_insert(0) += 1;
    }

    let unknown_directives = directive_types.get("Unknown").copied().unwrap_or(0);
    if unknown_directives > 0 {
        hard_blockers.push(format!(
            "{unknown_directives} unsupported Wabbajack directive(s) parsed as Unknown"
        ));
    }

    let rar_enabled = cfg!(feature = "rar");
    if !rar_enabled
        && manifest
            .archives
            .iter()
            .any(|archive| archive_extension(&archive.name) == "rar")
    {
        hard_blockers.push("RAR archives are present but this build does not enable `rar`".into());
    }

    let nexus_required = manifest
        .archives
        .iter()
        .any(|archive| matches!(archive.state, Some(ArchiveState::NexusDownloader { .. })));
    let nexus_available = options
        .nexus_credentials_available
        .unwrap_or_else(|| crate::nexus::auth::load_api_key().is_ok());
    if nexus_required && !nexus_available {
        hard_blockers
            .push("Nexus archives are required, but no Nexus API key is configured".to_string());
    }

    if !manual_downloads.is_empty() {
        warnings.push(format!(
            "{} manual archive(s) must be downloaded and imported before install",
            manual_downloads.len()
        ));
    }
    if !missing_nexus_archives.is_empty() && nexus_available {
        warnings.push(format!(
            "{} Nexus archive(s) are not in the local store yet and will be downloaded during install",
            missing_nexus_archives.len()
        ));
    }

    let game_file_sources = assess_game_file_sources(&manifest, options.game_dir.as_deref()).await;
    if !game_file_sources.missing.is_empty() {
        hard_blockers.push(format!(
            "{} game-file source(s) are missing",
            game_file_sources.missing.len()
        ));
    }
    if !game_file_sources.mismatched.is_empty() {
        hard_blockers.push(format!(
            "{} game-file source(s) failed hash verification",
            game_file_sources.mismatched.len()
        ));
    }

    let compatible_layout = StagingStore::new(&staging_path)
        .has_compatible_layout()
        .await;
    let archive_batch_total = manifest.install_directives_grouped_by_archive().len();
    let create_bsa_total = manifest
        .install_directives()
        .iter()
        .filter(|directive| {
            matches!(
                directive,
                modde_core::manifest::wabbajack::InstallDirective::CreateBSA { .. }
            )
        })
        .count();
    let staging = WabbajackStagingReport {
        path: staging_path.display().to_string(),
        exists: staging_path.exists(),
        compatible_layout,
        archive_batch_sentinels: count_json_files(staging_path.join("_state/archive-batches")),
        archive_batch_total,
        create_bsa_sentinels: count_json_files(staging_path.join("_state/create-bsa")),
        create_bsa_total,
        layout_action: if !staging_path.exists() {
            "create".into()
        } else if compatible_layout {
            "resume".into()
        } else {
            "adopt".into()
        },
    };
    if staging.exists && !staging.compatible_layout {
        warnings.push("existing staging will be adopted instead of deleted".into());
    }

    let install_ready = hard_blockers.is_empty() && manual_downloads.is_empty();

    Ok(WabbajackReadinessReport {
        manifest_path: options.manifest_path.display().to_string(),
        name: manifest.name,
        author: manifest.author,
        version: manifest.version,
        normalized_game: modde_games::normalize_wabbajack_game(&manifest.game)
            .map_or_else(|| manifest.game.to_ascii_lowercase(), str::to_string),
        game: manifest.game,
        profile_name,
        store_path: store.display().to_string(),
        archives: manifest.archives.len(),
        directives: manifest.directives.len(),
        archive_states,
        directive_types,
        archive_extensions,
        downloadable_archives,
        store_present,
        store_missing,
        manual_downloads,
        missing_nexus_archives,
        game_file_sources,
        staging,
        rar_enabled,
        nexus_required,
        nexus_available,
        hard_blockers,
        warnings,
        install_ready,
    })
}

async fn assess_game_file_sources(
    manifest: &WabbajackManifest,
    game_dir: Option<&Path>,
) -> WabbajackGameFileSourceReport {
    let mut total = 0;
    let mut present = 0;
    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    let required = manifest
        .archives
        .iter()
        .filter(|archive| {
            matches!(
                archive.state,
                Some(ArchiveState::GameFileSourceDownloader { .. })
            )
        })
        .count();

    let Some(game_dir) = game_dir else {
        return WabbajackGameFileSourceReport {
            total: required,
            present: 0,
            missing: if required == 0 {
                Vec::new()
            } else {
                vec!["game directory was not provided".into()]
            },
            mismatched,
        };
    };

    if required > 0 && !game_dir.is_dir() {
        return WabbajackGameFileSourceReport {
            total: required,
            present: 0,
            missing: vec![format!(
                "game directory does not exist: {}",
                game_dir.display()
            )],
            mismatched,
        };
    }

    for archive in &manifest.archives {
        let Some(state @ ArchiveState::GameFileSourceDownloader { .. }) = archive.state.as_ref()
        else {
            continue;
        };
        total += 1;
        let Some(rel) = state.game_file_path() else {
            missing.push(format!("{} has no recognized game-file path", archive.name));
            continue;
        };
        let Some(normalized) = normalize_relative_path(rel) else {
            missing.push(format!("{} has unsafe game-file path: {rel}", archive.name));
            continue;
        };

        let exact_path = game_dir.join(&normalized);
        let path = if exact_path.exists() {
            exact_path
        } else if let Ok(path) = find_path_case_insensitive(game_dir, &normalized) {
            path
        } else {
            missing.push(rel.to_string());
            continue;
        };

        match modde_core::hash::hash_file_xxh64(&path).await {
            Ok(actual) if actual == archive.hash => {
                present += 1;
            }
            Ok(actual) => {
                mismatched.push(format!(
                    "{} expected xxh64 {:016x}, got {:016x}",
                    rel, archive.hash, actual
                ));
            }
            Err(error) => {
                mismatched.push(format!("{} could not be hashed: {error}", path.display()));
            }
        }
    }

    WabbajackGameFileSourceReport {
        total,
        present,
        missing,
        mismatched,
    }
}

mod helpers;
use helpers::*;

#[cfg(test)]
mod tests;
