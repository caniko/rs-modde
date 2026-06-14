use std::collections::HashSet;

use super::strip_mo2_prefix;
use crate::manifest::wabbajack::{InstallDirective, WabbajackManifest};
use crate::profile::Profile;

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
/// Cyberpunk 2077 this is `modde_games::cyberpunk::scanner::mod_id_footprint`.
/// Profiles
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
