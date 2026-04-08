use std::collections::{HashMap, HashSet};

use crate::manifest::wabbajack::{ArchiveState, InstallDirective, WabbajackManifest};
use crate::profile::EnabledMod;

/// A mod discovered by matching a Wabbajack manifest against files on disk.
pub struct ManifestMatch {
    /// Stable unique ID based on Nexus identity or archive hash.
    pub mod_id: String,
    /// Human-readable name (from archive filename, cleaned).
    pub display_name: String,
    /// Original archive filename.
    pub archive_name: String,
    pub archive_hash: u64,
    pub total_files: usize,
    pub present_files: usize,
    pub confidence: f32,
    pub nexus_mod_id: Option<i64>,
    pub nexus_file_id: Option<i64>,
    pub nexus_game_domain: Option<String>,
    /// Game-relative file paths that this archive covers on disk (lowercased).
    /// Used for correlation with filesystem-discovered mods.
    pub covered_paths: Vec<String>,
}

/// Match files on disk against a Wabbajack manifest.
///
/// Groups directives by their source `archive_hash`, then checks what
/// fraction of each archive's `to` paths exist in `on_disk_files`.
/// Archives where the fraction meets or exceeds `threshold` are returned.
///
/// `on_disk_files` should contain lowercased, forward-slash relative paths
/// from the game install root.
pub fn match_wabbajack_manifest(
    manifest: &WabbajackManifest,
    on_disk_files: &HashSet<String>,
    threshold: f32,
) -> Vec<ManifestMatch> {
    let directives = manifest.install_directives();

    // Group directives by archive_hash → list of game-relative paths.
    // Also extract the MO2 mod name from the `mods/<Name>/...` prefix.
    let mut archive_files: HashMap<u64, Vec<String>> = HashMap::new();
    let mut archive_mod_names: HashMap<u64, String> = HashMap::new();

    for d in &directives {
        match d {
            InstallDirective::FromArchive {
                archive_hash, to, ..
            }
            | InstallDirective::PatchedFromArchive {
                archive_hash, to, ..
            } => {
                let normalized = to.replace('\\', "/");

                // Extract the MO2 mod name before lowercasing (preserves casing).
                if archive_mod_names.get(archive_hash).is_none() {
                    if let Some(name) = extract_mo2_mod_name(&normalized) {
                        archive_mod_names.insert(*archive_hash, name);
                    }
                }

                // Strip prefix and lowercase for matching.
                let game_relative = strip_mo2_prefix(&normalized.to_lowercase());
                archive_files
                    .entry(*archive_hash)
                    .or_default()
                    .push(game_relative);
            }
            _ => {}
        }
    }

    // Build archive hash → ArchiveEntry lookup for metadata.
    let archive_map: HashMap<u64, &crate::manifest::wabbajack::ArchiveEntry> = manifest
        .archives
        .iter()
        .map(|a| (a.hash, a))
        .collect();

    let mut results = Vec::new();

    for (hash, files) in &archive_files {
        let total = files.len();
        if total == 0 {
            continue;
        }

        let present_paths: Vec<String> = files
            .iter()
            .filter(|path| on_disk_files.contains(path.as_str()))
            .cloned()
            .collect();
        let present = present_paths.len();

        let fraction = present as f32 / total as f32;
        if fraction < threshold {
            continue;
        }

        let archive = archive_map.get(hash);
        let archive_name = archive
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("unknown_{hash}"));

        // Display name: prefer cleaned archive filename (unique per archive).
        let display_name = clean_archive_name(&archive_name);

        let (nexus_mod_id, nexus_file_id, nexus_game_domain) = archive
            .and_then(|a| a.state.as_ref())
            .map(|state| match state {
                ArchiveState::NexusDownloader {
                    game_name,
                    mod_id,
                    file_id,
                } => (
                    Some(*mod_id as i64),
                    Some(*file_id as i64),
                    Some(game_name.clone()),
                ),
                _ => (None, None, None),
            })
            .unwrap_or((None, None, None));

        // mod_id: use Nexus identity (stable, unique) or fall back to archive hash.
        let mod_id = if let (Some(domain), Some(nmod_id), Some(nfile_id)) =
            (&nexus_game_domain, nexus_mod_id, nexus_file_id)
        {
            format!("nexus_{domain}_{nmod_id}_{nfile_id}")
        } else {
            format!("wj_{hash}")
        };

        results.push(ManifestMatch {
            mod_id,
            display_name,
            archive_name,
            archive_hash: *hash,
            total_files: total,
            present_files: present,
            confidence: fraction,
            nexus_mod_id,
            nexus_file_id,
            nexus_game_domain,
            covered_paths: present_paths,
        });
    }

    // Sort by display_name for readability.
    results.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
    results
}

/// Convert a `ManifestMatch` into an `EnabledMod` for database storage.
pub fn manifest_match_to_enabled(m: &ManifestMatch) -> EnabledMod {
    EnabledMod {
        mod_id: m.mod_id.clone(),
        display_name: Some(m.display_name.clone()),
        enabled: true,
        version: None,
        fomod_config: None,
        nexus_mod_id: m.nexus_mod_id,
        nexus_file_id: m.nexus_file_id,
        nexus_game_domain: m.nexus_game_domain.clone(),
        installed_timestamp: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        ),
        ..Default::default()
    }
}

/// Extract the MO2 mod name from a directive path.
///
/// Paths like `mods/Immersive Healing/archive/pc/mod/ImmersiveHealing.archive`
/// yield `"Immersive Healing"`.
fn extract_mo2_mod_name(path: &str) -> Option<String> {
    let rest = path.strip_prefix("mods/")?;
    let end = rest.find('/')?;
    let name = &rest[..end];
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Strip MO2 staging prefix from a path.
///
/// `mods/<mod_name>/<game_relative_path>` → `<game_relative_path>`.
/// Non-mod paths (e.g., MO2 executables) are returned as-is.
fn strip_mo2_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("mods/") {
        if let Some(idx) = rest.find('/') {
            return rest[idx + 1..].to_string();
        }
    }
    path.to_string()
}

/// Clean an archive filename into a display name.
///
/// `ImmersiveHealing-26281-3-1-3-1772288704.zip` → `ImmersiveHealing`.
/// Strips the Nexus suffix pattern (mod_id-version-timestamp.ext).
fn clean_archive_name(name: &str) -> String {
    // Strip extension.
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    // Nexus filenames: "ModName-modid-version-timestamp". Strip from first `-{digits}`.
    if let Some(idx) = stem.find(|c: char| c == '-').and_then(|i| {
        if stem[i + 1..].starts_with(|c: char| c.is_ascii_digit()) {
            Some(i)
        } else {
            None
        }
    }) {
        stem[..idx].replace('_', " ")
    } else {
        stem.replace('_', " ")
    }
}

/// Convert a filesystem-discovered mod into an `EnabledMod`.
pub fn discovered_to_enabled(
    mod_id: &str,
    display_name: &str,
    version: Option<&str>,
    _confidence: f32,
) -> EnabledMod {
    EnabledMod {
        mod_id: mod_id.to_string(),
        display_name: Some(display_name.to_string()),
        enabled: true,
        version: version.map(String::from),
        ..Default::default()
    }
}
