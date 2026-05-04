use std::collections::{HashMap, HashSet};

use crate::manifest::wabbajack::{
    ArchiveEntry, ArchiveState, InstallDirective, WabbajackManifest, compute_manifest_hash,
};
use crate::profile::{EnabledMod, LoadOrderLock, LockReason, Profile};

/// Canonical `mod_id` derivation for a Wabbajack archive entry.
///
/// Used by **both** the scanner and the Wabbajack installer so that a
/// profile installed via `modde install wabbajack` and the same modlist
/// re-scanned via `modde scan --manifest` produce identical `mod_id`
/// strings — otherwise retroactive-lock flows would create duplicates
/// rather than matching existing mods.
///
/// - Nexus-sourced archives: `nexus_{game_domain}_{mod_id}_{file_id}`
/// - Everything else:        `wj_{archive_hash}`
#[must_use]
pub fn archive_mod_id(archive: &ArchiveEntry) -> String {
    if let Some(ArchiveState::NexusDownloader {
        game_name,
        mod_id,
        file_id,
    }) = archive.state.as_ref()
    {
        format!("nexus_{game_name}_{mod_id}_{file_id}")
    } else {
        format!("wj_{}", archive.hash)
    }
}

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
#[must_use]
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
                if archive_mod_names.get(archive_hash).is_none()
                    && let Some(name) = extract_mo2_mod_name(&normalized)
                {
                    archive_mod_names.insert(*archive_hash, name);
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
    let archive_map: HashMap<u64, &crate::manifest::wabbajack::ArchiveEntry> =
        manifest.archives.iter().map(|a| (a.hash, a)).collect();

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
        let archive_name = archive.map_or_else(|| format!("unknown_{hash}"), |a| a.name.clone());

        // Display name: prefer cleaned archive filename (unique per archive).
        let display_name = clean_archive_name(&archive_name);

        let (nexus_mod_id, nexus_file_id, nexus_game_domain) = archive
            .and_then(|a| a.state.as_ref())
            .map_or((None, None, None), |state| match state {
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
            });

        // Canonical mod_id — must match `archive_mod_id` exactly so Wabbajack
        // installs + retroactive scans dedup correctly.
        let mod_id = match archive {
            Some(a) => archive_mod_id(a),
            None => format!("wj_{hash}"),
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
#[must_use]
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
    if let Some(rest) = path.strip_prefix("mods/")
        && let Some(idx) = rest.find('/')
    {
        return rest[idx + 1..].to_string();
    }
    path.to_string()
}

/// Clean an archive filename into a display name.
///
/// `ImmersiveHealing-26281-3-1-3-1772288704.zip` → `ImmersiveHealing`.
/// Strips the Nexus suffix pattern (mod_id-version-timestamp.ext).
fn clean_archive_name(name: &str) -> String {
    // Strip extension.
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    // Nexus filenames: "ModName-modid-version-timestamp". Strip from first `-{digits}`.
    if let Some(idx) = stem
        .find('-')
        .filter(|&i| stem[i + 1..].starts_with(|c: char| c.is_ascii_digit()))
    {
        stem[..idx].replace('_', " ")
    } else {
        stem.replace('_', " ")
    }
}

/// Compute the canonical mod order from a Wabbajack manifest's install
/// directives.
///
/// `WabbajackManifest.archives` is an unordered JSON array — not a load
/// order. The *directive* list, however, is the sequence Wabbajack applies
/// on install, so the first-appearance order of each archive in the
/// directives is the closest reproducible approximation of "load order".
///
/// Returns a `Vec<String>` of canonical `mod_id`s (as produced by
/// [`archive_mod_id`]) in the order the corresponding archives first
/// appear in the install directives. Archives that never appear in a
/// [`InstallDirective::FromArchive`] / [`InstallDirective::PatchedFromArchive`]
/// are omitted.
#[must_use]
pub fn manifest_directive_order(manifest: &WabbajackManifest) -> Vec<String> {
    let archive_by_hash: HashMap<u64, &ArchiveEntry> =
        manifest.archives.iter().map(|a| (a.hash, a)).collect();

    let mut seen: HashSet<u64> = HashSet::new();
    let mut order: Vec<String> = Vec::new();
    for d in manifest.install_directives() {
        let hash = match d {
            InstallDirective::FromArchive { archive_hash, .. }
            | InstallDirective::PatchedFromArchive { archive_hash, .. } => archive_hash,
            _ => continue,
        };
        if !seen.insert(hash) {
            continue;
        }
        if let Some(archive) = archive_by_hash.get(&hash) {
            order.push(archive_mod_id(archive));
        }
    }
    order
}

/// Report from [`apply_wabbajack_lock`] — what the in-place reorder did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WabbajackLockApplied {
    /// `manifest_hash` recorded on the new lock. Matches
    /// `ProfileSource::Wabbajack { manifest_hash }` on installs.
    pub manifest_hash: String,
    /// Number of mods whose `mod_id` is present in the manifest order
    /// (these end up at the front of the mod list).
    pub matched: usize,
    /// Number of pre-existing profile mods not mentioned by the
    /// manifest (these are appended after, preserving relative order).
    pub unmatched: usize,
    /// Whether the profile already carried a lock that was overwritten.
    pub replaced_existing_lock: bool,
}

/// Reorder `profile.mods` to follow the manifest's install-directive
/// order and stamp a `LockReason::Wabbajack` lock onto the profile.
///
/// This is the pure helper that powers `modde scan --manifest` and is
/// the recommended way to retroactively lock an existing profile to a
/// Wabbajack modlist. Extracted from `scan.rs` so it can be unit-tested
/// without touching the filesystem scanner.
///
/// Invariants:
///
/// 1. **Mod count is preserved** — no mod is ever dropped. Matched mods
///    move to the front in manifest order; unmatched mods retain their
///    original relative order and are appended after.
/// 2. **Matched mods are sorted by first-appearance in install
///    directives** — see [`manifest_directive_order`] for the semantic.
/// 3. **`profile.load_order_lock` is overwritten** — any prior lock
///    (including a stale Wabbajack or Manual lock) is replaced. The
///    return value's `replaced_existing_lock` field lets callers surface
///    this to the user.
pub fn apply_wabbajack_lock(
    profile: &mut Profile,
    manifest: &WabbajackManifest,
) -> WabbajackLockApplied {
    let manifest_order = manifest_directive_order(manifest);
    let manifest_rank: HashMap<String, usize> = manifest_order
        .iter()
        .enumerate()
        .map(|(i, mid)| (mid.clone(), i))
        .collect();

    // Stable partition: matched first (in manifest order), unmatched
    // after (original relative order preserved).
    let (mut matched, unmatched): (Vec<EnabledMod>, Vec<EnabledMod>) =
        std::mem::take(&mut profile.mods)
            .into_iter()
            .partition(|m| manifest_rank.contains_key(&m.mod_id));

    matched.sort_by_key(|m| manifest_rank.get(&m.mod_id).copied().unwrap_or(usize::MAX));

    let matched_count = matched.len();
    let unmatched_count = unmatched.len();
    profile.mods = matched;
    profile.mods.extend(unmatched);

    let manifest_hash = compute_manifest_hash(manifest);
    let replaced_existing_lock = profile.load_order_lock.is_some();
    profile.load_order_lock = Some(LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: manifest_hash.clone(),
    }));

    WabbajackLockApplied {
        manifest_hash,
        matched: matched_count,
        unmatched: unmatched_count,
        replaced_existing_lock,
    }
}

/// The filesystem footprint of a mod discovered by a game-specific
/// filesystem scanner.
///
/// Game scanners produce `mod_ids` in schemes like `cet/<name>`,
/// `archive/<stem>`, etc. To correlate those rows against a Wabbajack
/// manifest's install directives, we need to know what portion of the
/// game directory each mod owns. That's what this enum expresses.
///
/// - [`ModFootprint::Directory`] — the mod owns everything under a
///   subtree of the game install (e.g. `bin/x64/plugins/cyber_engine_tweaks/mods/<name>/`).
/// - [`ModFootprint::File`] — the mod *is* a single file (e.g. a
///   loose `.archive` under `archive/pc/mod/`).
///
/// Paths are lowercased, use forward slashes, and (for `Directory`)
/// end with a trailing `/`. This matches the conventions used by
/// [`dir_prefixes`](crate::scanner) and the manifest-covered-dirs set
/// built in `modde-cli::commands::scan`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModFootprint {
    /// A directory subtree owned by the mod. Compared against the set of
    /// directories the manifest writes into.
    Directory(String),
    /// A single file owned by the mod. Compared against the set of
    /// `To` paths in the manifest's install directives.
    File(String),
}

/// Result of [`detect_stale_duplicates`] — a partition of a profile's
/// filesystem-scanner rows into "covered by the manifest" (leaked
/// duplicates) and "not covered" (genuine additions).
///
/// `mod_ids` whose footprint cannot be determined by the supplied
/// `mod_id_to_footprint` closure (typically `nexus_*`, `wj_*`, or any
/// non-filesystem-scheme row) are **not** included in either list —
/// they're skipped silently because they aren't candidates for this
/// kind of dedup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DuplicateReport {
    /// Filesystem-scanner `mod_ids` whose footprint is covered by the
    /// manifest. These are safe to delete from the profile: a
    /// manifest-authored row (usually `nexus_*`) already deploys the
    /// same files under a different ID.
    pub leaked: Vec<String>,
    /// Filesystem-scanner `mod_ids` whose footprint is **not** covered
    /// by the manifest. These are genuine additions the user made on
    /// top of the Wabbajack modlist and must be preserved.
    pub genuine: Vec<String>,
}

/// Classify a profile's filesystem-scanner rows against a Wabbajack
/// manifest into "leaked duplicates" and "genuine additions".
///
/// This is the pure helper that powers `modde profile dedup` and the
/// `--prune-duplicates` flag on `modde scan`. See
/// `/home/can/.claude/plans/greedy-shimmying-pine.md` and the companion
/// discussion in `docs/` (if present) for the design rationale.
///
/// The `mod_id_to_footprint` closure is the game-specific bridge: it
/// maps a filesystem-scanner `mod_id` (e.g. `cet/ImmersiveHealing`) back
/// to the directory or file the mod owns in the game install. For
/// Cyberpunk 2077 this is
/// [`modde_games::cyberpunk::scanner::mod_id_footprint`]. Profiles
/// spanning multiple games aren't supported — each profile is tied to
/// a single game via `profile.game_id`, so callers wire up a
/// per-game closure.
///
/// Classification rules:
///
/// 1. If the closure returns `None` for a `mod_id`, the row is **not a
///    candidate** — it's skipped silently. `nexus_*` and `wj_*` rows
///    are manifest-authored and shouldn't be classified as duplicates
///    of themselves.
/// 2. If the footprint is [`ModFootprint::Directory`] and the manifest
///    writes any file under that directory → **LEAKED** (the nexus
///    archive that deployed those files is already tracked under its
///    `nexus_*` ID).
/// 3. If the footprint is [`ModFootprint::File`] and the exact file
///    path appears in the manifest's install directives → **LEAKED**.
/// 4. Otherwise → **GENUINE**: the user added this mod on top of the
///    Wabbajack and it must not be deleted.
///
/// Case and slash-normalization: paths are lowercased and
/// forward-slashed internally, so callers don't need to pre-normalize.
///
/// Complexity: O(D × A + M) where D is manifest directive count,
/// A is average path depth, and M is profile mod count. For a typical
/// CP2077 modlist (≈7k directives, ≈700 mods) this runs in well under
/// a millisecond.
pub fn detect_stale_duplicates<F>(
    profile: &Profile,
    manifest: &WabbajackManifest,
    mod_id_to_footprint: F,
) -> DuplicateReport
where
    F: Fn(&str) -> Option<ModFootprint>,
{
    // Build the manifest's covered file set + covered directory set
    // from its install directives. Only `FromArchive` /
    // `PatchedFromArchive` directives are "physical" file placements
    // we can compare against — `CreateBSA` and `InlineFile` don't map
    // cleanly to a single on-disk file at scan time.
    //
    // Wabbajack `To` paths are MO2-staged: they look like
    // `mods\<MO2 Mod Name>\<game-relative-path>`. We must strip the
    // `mods/<name>/` prefix before comparing against game-relative
    // footprints — this mirrors what `match_wabbajack_manifest` does
    // via `strip_mo2_prefix`. Without this step, every directive path
    // in a CP2077 modlist begins with `mods/<big mod name>/`, which
    // never overlaps with a `bin/x64/...` or `archive/pc/mod/...`
    // footprint, and `detect_stale_duplicates` silently classifies
    // every row as GENUINE. See profile 3077 for the failure mode.
    let mut covered_files: HashSet<String> = HashSet::new();
    for d in manifest.install_directives() {
        let to = match d {
            InstallDirective::FromArchive { to, .. }
            | InstallDirective::PatchedFromArchive { to, .. } => to,
            _ => continue,
        };
        let normalized = to.replace('\\', "/").to_lowercase();
        covered_files.insert(strip_mo2_prefix(&normalized));
    }

    // Expand each covered file into its ancestor-directory prefixes so
    // the Directory footprint check becomes a single O(1) HashSet lookup.
    let mut covered_dirs: HashSet<String> = HashSet::new();
    for f in &covered_files {
        let mut cur = f.as_str();
        while let Some(idx) = cur.rfind('/') {
            cur = &cur[..idx];
            covered_dirs.insert(format!("{cur}/"));
        }
    }

    let mut report = DuplicateReport::default();
    for m in &profile.mods {
        let footprint = match mod_id_to_footprint(&m.mod_id) {
            Some(fp) => fp,
            None => continue, // Not a filesystem-scanner row; skip.
        };
        let covered = match &footprint {
            ModFootprint::Directory(d) => covered_dirs.contains(d),
            ModFootprint::File(f) => covered_files.contains(f),
        };
        if covered {
            report.leaked.push(m.mod_id.clone());
        } else {
            report.genuine.push(m.mod_id.clone());
        }
    }
    report
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
