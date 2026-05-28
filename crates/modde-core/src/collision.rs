//! Mod collision detection and analysis.
//!
//! Builds on the existing [`ConflictMap`] to provide:
//! - Archive-aware conflict detection (BSA/BA2 contents, not just loose files)
//! - Collision severity classification (cosmetic vs dangerous)
//! - Per-mod-pair collision grouping
//! - Pre-deploy optimisation: shadowed mods, redundant files

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::fs::walk_files_relative;
use crate::resolver::{ConflictMap, ModId};

// ── Types ───────────────────────────────────────────────────────────

/// How a file is provided by a mod.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileOrigin {
    /// A loose file in the mod's staging directory.
    Loose,
    /// A file listed inside an archive.
    Archive { archive_rel: String },
}

/// Risk level of a file collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CollisionSeverity {
    /// Texture/mesh/sound — cosmetic, low risk.
    Cosmetic,
    /// INI/config — medium risk, may change behaviour.
    Config,
    /// Script/plugin/DLL — high risk, potential crashes or save corruption.
    Dangerous,
    /// Cannot classify automatically.
    Unknown,
}

impl std::fmt::Display for CollisionSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cosmetic => f.write_str("COSMETIC"),
            Self::Config => f.write_str("CONFIG"),
            Self::Dangerous => f.write_str("DANGEROUS"),
            Self::Unknown => f.write_str("UNKNOWN"),
        }
    }
}

/// Detail about a single file that collides between two mods.
#[derive(Debug, Clone)]
pub struct FileCollision {
    pub file_path: String,
    pub severity: CollisionSeverity,
    pub winner: ModId,
    pub loser: ModId,
    pub winner_origin: FileOrigin,
    pub loser_origin: FileOrigin,
    pub is_loser_hidden: bool,
}

/// Aggregated collision info between a specific pair of mods.
#[derive(Debug, Clone)]
pub struct ModPairCollision {
    /// The lower-priority mod (the one that loses files).
    pub loser: ModId,
    /// The higher-priority mod (the one that wins files).
    pub winner: ModId,
    /// Individual file collisions in this pair.
    pub files: Vec<FileCollision>,
    /// Worst severity among all collisions in this pair.
    pub max_severity: CollisionSeverity,
}

/// A mod whose files are all overridden by higher-priority mods.
#[derive(Debug, Clone)]
pub struct ShadowedMod {
    pub mod_id: ModId,
    /// Which mods override this one's files.
    pub shadowed_by: Vec<ModId>,
    /// Total file count that is overridden.
    pub file_count: usize,
}

/// Full collision analysis report for a profile.
#[derive(Debug, Clone, Default)]
pub struct CollisionReport {
    /// Collision details grouped by (loser, winner) mod pair.
    pub pairs: Vec<ModPairCollision>,
    /// Files that are provided by a mod but always overridden (`mod_id`, `file_path`).
    pub redundant_files: Vec<(ModId, String)>,
    /// Mods whose files are all overridden.
    pub shadowed_mods: Vec<ShadowedMod>,
    /// Loose files that override files inside archives.
    pub loose_vs_archive: Vec<FileCollision>,
    /// Total number of file-level collisions.
    pub total_collisions: usize,
}

// ── Classifier trait ────────────────────────────────────────────────

/// Game-specific behaviour for collision detection.
///
/// Each game provides archive indexing and file severity classification.
pub trait CollisionClassifier: Send + Sync {
    /// List files inside an archive at `archive_path`.
    /// Returns `(normalised_relative_path, size)` pairs.
    fn index_archive(&self, archive_path: &Path) -> Result<Vec<(String, u64)>>;

    /// Classify the collision severity of a file based on its path.
    fn classify_severity(&self, file_path: &str) -> CollisionSeverity;

    /// File extensions (lowercase, no dot) that are archives for this game.
    fn archive_extensions(&self) -> &[&str];
}

// ── Conflict map builder ────────────────────────────────────────────

/// Tracks per-file origin information alongside the conflict map.
pub type OriginMap = HashMap<String, HashMap<ModId, FileOrigin>>;

const MISSING_STORE_DIR_SAMPLE_LIMIT: usize = 10;

#[derive(Debug, PartialEq, Eq)]
pub struct MissingStoreDirsSummary {
    pub missing_mod_count: usize,
    pub missing_mod_sample: Vec<String>,
    pub omitted_missing_mod_count: usize,
}

#[derive(Debug)]
pub struct FullConflictMap {
    pub conflict_map: ConflictMap,
    pub origins: OriginMap,
    pub missing_mods: Vec<ModId>,
}

pub fn summarize_missing_store_dirs(missing_mods: &[ModId]) -> Option<MissingStoreDirsSummary> {
    if missing_mods.is_empty() {
        return None;
    }

    let missing_mod_sample = missing_mods
        .iter()
        .take(MISSING_STORE_DIR_SAMPLE_LIMIT)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let missing_mod_count = missing_mods.len();
    let omitted_missing_mod_count =
        missing_mod_count.saturating_sub(MISSING_STORE_DIR_SAMPLE_LIMIT);

    Some(MissingStoreDirsSummary {
        missing_mod_count,
        missing_mod_sample,
        omitted_missing_mod_count,
    })
}

/// Build a [`ConflictMap`] that includes both loose files and archive contents.
///
/// For each mod in `resolved_order`, walks its store directory for loose files,
/// then indexes any archives via `classifier`.
///
/// Returns the conflict map and a parallel map tracking how each mod provides
/// each file ([`FileOrigin`]).
pub fn build_full_conflict_map(
    store: &Path,
    resolved_order: &[ModId],
    classifier: &dyn CollisionClassifier,
) -> Result<FullConflictMap> {
    let archive_exts: HashSet<&str> = classifier.archive_extensions().iter().copied().collect();
    let mut conflict_map = ConflictMap::default();
    let mut origins: OriginMap = HashMap::new();
    let mut missing_mods = Vec::new();

    for mod_id in resolved_order {
        let mod_dir = store.join(mod_id.as_str());
        if !mod_dir.exists() {
            missing_mods.push(mod_id.clone());
            continue;
        }

        let files = walk_files_relative(&mod_dir)?;

        for (rel_path, abs_path) in &files {
            // Register the loose file.
            conflict_map.register(rel_path.clone(), mod_id.clone());
            origins
                .entry(rel_path.clone())
                .or_default()
                .insert(mod_id.clone(), FileOrigin::Loose);

            // If this file is an archive, index its contents.
            let ext = abs_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if archive_exts.contains(ext.as_str()) {
                let archive_files = match classifier.index_archive(abs_path) {
                    Ok(files) => {
                        debug!(%mod_id, archive = rel_path, count = files.len(), "indexed archive");
                        files
                    }
                    Err(e) => {
                        warn!(%mod_id, archive = rel_path, error = %e, "failed to index archive");
                        Vec::new()
                    }
                };

                for (archive_file_path, _size) in archive_files {
                    conflict_map.register(archive_file_path.clone(), mod_id.clone());
                    origins.entry(archive_file_path).or_default().insert(
                        mod_id.clone(),
                        FileOrigin::Archive {
                            archive_rel: rel_path.clone(),
                        },
                    );
                }
            }
        }
    }

    if let Some(summary) = summarize_missing_store_dirs(&missing_mods) {
        debug!(
            store = %store.display(),
            missing_mod_count = summary.missing_mod_count,
            missing_mod_sample = ?summary.missing_mod_sample,
            omitted_missing_mod_count = summary.omitted_missing_mod_count,
            "mod directories not found in store, skipping"
        );
    }

    Ok(FullConflictMap {
        conflict_map,
        origins,
        missing_mods,
    })
}

// ── Collision analyser ──────────────────────────────────────────────

/// Analyse a conflict map to produce a full collision report.
///
/// `priority_order` lists mods from lowest to highest priority (same as
/// [`crate::resolver::ResolvedLoadOrder::order`]). `hidden` contains `(mod_id, rel_path)`
/// pairs that have been hidden by the user.
pub fn analyze_collisions(
    conflict_map: &ConflictMap,
    priority_order: &[ModId],
    hidden: &HashSet<(String, String)>,
    origins: &OriginMap,
    classifier: &dyn CollisionClassifier,
) -> CollisionReport {
    let priority_rank: HashMap<&ModId, usize> = priority_order
        .iter()
        .enumerate()
        .map(|(i, m)| (m, i))
        .collect();

    // Collect all file-level collisions.
    let mut pair_map: HashMap<(ModId, ModId), Vec<FileCollision>> = HashMap::new();
    let mut all_loose_vs_archive = Vec::new();
    let mut total_collisions: usize = 0;

    // Track how many files each mod provides and how many are overridden.
    let mut mod_file_count: HashMap<ModId, usize> = HashMap::new();
    let mut mod_overridden_count: HashMap<ModId, usize> = HashMap::new();
    let mut mod_overridden_by: HashMap<ModId, HashSet<ModId>> = HashMap::new();

    // Count total files per mod (including non-conflicting).
    for providers in conflict_map.files.values() {
        for mod_id in providers {
            *mod_file_count.entry(mod_id.clone()).or_default() += 1;
        }
    }

    for (file_path, providers) in &conflict_map.files {
        if providers.len() < 2 {
            continue;
        }

        total_collisions += providers.len() - 1;

        let winner = conflict_map.winner_for(file_path, priority_order, hidden);
        let severity = classifier.classify_severity(file_path);

        let winner_id = match &winner {
            Some(w) => w,
            None => continue,
        };

        let winner_origin = origins
            .get(file_path)
            .and_then(|m| m.get(winner_id))
            .cloned()
            .unwrap_or(FileOrigin::Loose);

        for loser_id in providers {
            if loser_id == winner_id {
                continue;
            }

            let loser_origin = origins
                .get(file_path)
                .and_then(|m| m.get(loser_id))
                .cloned()
                .unwrap_or(FileOrigin::Loose);

            let is_hidden = hidden.contains(&(loser_id.0.clone(), file_path.clone()));

            let collision = FileCollision {
                file_path: file_path.clone(),
                severity,
                winner: winner_id.clone(),
                loser: loser_id.clone(),
                winner_origin: winner_origin.clone(),
                loser_origin: loser_origin.clone(),
                is_loser_hidden: is_hidden,
            };

            // Detect loose vs archive conflicts.
            if collision.winner_origin != collision.loser_origin {
                let is_loose_vs_archive = matches!(
                    (&collision.winner_origin, &collision.loser_origin),
                    (FileOrigin::Loose, FileOrigin::Archive { .. })
                        | (FileOrigin::Archive { .. }, FileOrigin::Loose)
                );
                if is_loose_vs_archive {
                    all_loose_vs_archive.push(collision.clone());
                }
            }

            // Track overridden files for shadowed mod detection.
            *mod_overridden_count.entry(loser_id.clone()).or_default() += 1;
            mod_overridden_by
                .entry(loser_id.clone())
                .or_default()
                .insert(winner_id.clone());

            // Group by (loser, winner) pair, ordered by priority.
            let key = order_pair(loser_id, winner_id, &priority_rank);
            pair_map.entry(key).or_default().push(collision);
        }
    }

    // Build pair summaries.
    let mut pairs: Vec<ModPairCollision> = pair_map
        .into_iter()
        .map(|((loser, winner), files)| {
            let max_severity = files
                .iter()
                .map(|f| f.severity)
                .max()
                .unwrap_or(CollisionSeverity::Unknown);
            ModPairCollision {
                loser,
                winner,
                files,
                max_severity,
            }
        })
        .collect();

    // Sort pairs: most severe first, then by file count descending.
    pairs.sort_by(|a, b| {
        b.max_severity
            .cmp(&a.max_severity)
            .then_with(|| b.files.len().cmp(&a.files.len()))
    });

    // Identify redundant files: files provided by a mod that always lose.
    let mut redundant_files = Vec::new();
    for (file_path, providers) in &conflict_map.files {
        if providers.len() < 2 {
            continue;
        }
        let winner = conflict_map.winner_for(file_path, priority_order, hidden);
        for provider in providers {
            if winner.as_ref() != Some(provider) {
                redundant_files.push((provider.clone(), file_path.clone()));
            }
        }
    }

    // Identify fully shadowed mods.
    let shadowed_mods: Vec<ShadowedMod> = mod_file_count
        .iter()
        .filter_map(|(mod_id, &total)| {
            let overridden = mod_overridden_count.get(mod_id).copied().unwrap_or(0);
            if overridden >= total && total > 0 {
                let shadowed_by = mod_overridden_by
                    .get(mod_id)
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default();
                Some(ShadowedMod {
                    mod_id: mod_id.clone(),
                    shadowed_by,
                    file_count: total,
                })
            } else {
                None
            }
        })
        .collect();

    CollisionReport {
        pairs,
        redundant_files,
        shadowed_mods,
        loose_vs_archive: all_loose_vs_archive,
        total_collisions,
    }
}

/// Order a (`mod_a`, `mod_b`) pair so the lower-priority mod is first.
fn order_pair(a: &ModId, b: &ModId, priority_rank: &HashMap<&ModId, usize>) -> (ModId, ModId) {
    let rank_a = priority_rank.get(a).copied().unwrap_or(0);
    let rank_b = priority_rank.get(b).copied().unwrap_or(0);
    if rank_a <= rank_b {
        (a.clone(), b.clone())
    } else {
        (b.clone(), a.clone())
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A test classifier that classifies by extension.
    struct TestClassifier;

    impl CollisionClassifier for TestClassifier {
        fn index_archive(&self, _path: &Path) -> Result<Vec<(String, u64)>> {
            Ok(Vec::new())
        }

        fn classify_severity(&self, file_path: &str) -> CollisionSeverity {
            let ext = file_path.rsplit('.').next().unwrap_or("");
            match ext {
                "esp" | "esm" | "dll" => CollisionSeverity::Dangerous,
                "ini" | "cfg" => CollisionSeverity::Config,
                "dds" | "nif" => CollisionSeverity::Cosmetic,
                _ => CollisionSeverity::Unknown,
            }
        }

        fn archive_extensions(&self) -> &[&str] {
            &["bsa"]
        }
    }

    fn mod_id(s: &str) -> ModId {
        ModId::from(s)
    }

    #[test]
    fn missing_store_dirs_summary_is_none_for_empty_input() {
        assert_eq!(summarize_missing_store_dirs(&[]), None);
    }

    #[test]
    fn missing_store_dirs_summary_reports_all_entries_within_sample_limit() {
        let missing_mods = vec![mod_id("mod_a"), mod_id("mod_b")];

        assert_eq!(
            summarize_missing_store_dirs(&missing_mods),
            Some(MissingStoreDirsSummary {
                missing_mod_count: 2,
                missing_mod_sample: vec!["mod_a".to_string(), "mod_b".to_string()],
                omitted_missing_mod_count: 0,
            })
        );
    }

    #[test]
    fn missing_store_dirs_summary_bounds_sample_and_counts_omitted_entries() {
        let missing_mods = (0..12)
            .map(|idx| mod_id(&format!("mod_{idx}")))
            .collect::<Vec<_>>();

        assert_eq!(
            summarize_missing_store_dirs(&missing_mods),
            Some(MissingStoreDirsSummary {
                missing_mod_count: 12,
                missing_mod_sample: (0..10).map(|idx| format!("mod_{idx}")).collect(),
                omitted_missing_mod_count: 2,
            })
        );
    }

    #[test]
    fn build_full_conflict_map_skips_missing_store_dirs() {
        let store = tempfile::tempdir().unwrap();
        let present = store.path().join("mod_present");
        std::fs::create_dir_all(present.join("textures")).unwrap();
        std::fs::write(present.join("textures/sky.dds"), b"sky").unwrap();

        let order = vec![mod_id("mod_missing"), mod_id("mod_present")];
        let result = build_full_conflict_map(store.path(), &order, &TestClassifier).unwrap();

        assert_eq!(result.missing_mods, vec![mod_id("mod_missing")]);
        assert_eq!(result.conflict_map.files.len(), 1);
        assert!(result.conflict_map.files.contains_key("textures/sky.dds"));
        assert!(
            !result
                .conflict_map
                .files
                .values()
                .any(|mods| { mods.iter().any(|mod_id| mod_id.as_str() == "mod_missing") })
        );
        assert!(
            !result
                .origins
                .values()
                .any(|mods| { mods.keys().any(|mod_id| mod_id.as_str() == "mod_missing") })
        );
    }

    #[test]
    fn no_collisions_produces_empty_report() {
        let mut cm = ConflictMap::default();
        cm.register("textures/sky.dds".into(), mod_id("mod_a"));
        cm.register("textures/ground.dds".into(), mod_id("mod_b"));

        let order = vec![mod_id("mod_a"), mod_id("mod_b")];
        let hidden = HashSet::new();
        let origins = HashMap::new();

        let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
        assert!(report.pairs.is_empty());
        assert_eq!(report.total_collisions, 0);
    }

    #[test]
    fn simple_collision_detected() {
        let mut cm = ConflictMap::default();
        cm.register("textures/sky.dds".into(), mod_id("mod_a"));
        cm.register("textures/sky.dds".into(), mod_id("mod_b"));

        let order = vec![mod_id("mod_a"), mod_id("mod_b")];
        let hidden = HashSet::new();
        let origins = HashMap::new();

        let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
        assert_eq!(report.pairs.len(), 1);
        assert_eq!(report.total_collisions, 1);
        assert_eq!(report.pairs[0].winner, mod_id("mod_b"));
        assert_eq!(report.pairs[0].loser, mod_id("mod_a"));
        assert_eq!(report.pairs[0].max_severity, CollisionSeverity::Cosmetic);
    }

    #[test]
    fn dangerous_collision_severity() {
        let mut cm = ConflictMap::default();
        cm.register("scripts/combat.esp".into(), mod_id("mod_a"));
        cm.register("scripts/combat.esp".into(), mod_id("mod_b"));

        let order = vec![mod_id("mod_a"), mod_id("mod_b")];
        let hidden = HashSet::new();
        let origins = HashMap::new();

        let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
        assert_eq!(report.pairs[0].max_severity, CollisionSeverity::Dangerous);
    }

    #[test]
    fn shadowed_mod_detected() {
        let mut cm = ConflictMap::default();
        // mod_a provides two files, both also provided by mod_b
        cm.register("textures/sky.dds".into(), mod_id("mod_a"));
        cm.register("textures/sky.dds".into(), mod_id("mod_b"));
        cm.register("textures/ground.dds".into(), mod_id("mod_a"));
        cm.register("textures/ground.dds".into(), mod_id("mod_b"));

        let order = vec![mod_id("mod_a"), mod_id("mod_b")];
        let hidden = HashSet::new();
        let origins = HashMap::new();

        let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
        assert_eq!(report.shadowed_mods.len(), 1);
        assert_eq!(report.shadowed_mods[0].mod_id, mod_id("mod_a"));
        assert_eq!(report.shadowed_mods[0].file_count, 2);
    }

    #[test]
    fn redundant_files_tracked() {
        let mut cm = ConflictMap::default();
        cm.register("textures/sky.dds".into(), mod_id("mod_a"));
        cm.register("textures/sky.dds".into(), mod_id("mod_b"));

        let order = vec![mod_id("mod_a"), mod_id("mod_b")];
        let hidden = HashSet::new();
        let origins = HashMap::new();

        let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
        assert_eq!(report.redundant_files.len(), 1);
        assert_eq!(report.redundant_files[0].0, mod_id("mod_a"));
    }

    #[test]
    fn loose_vs_archive_detected() {
        let mut cm = ConflictMap::default();
        cm.register("textures/sky.dds".into(), mod_id("mod_a"));
        cm.register("textures/sky.dds".into(), mod_id("mod_b"));

        let order = vec![mod_id("mod_a"), mod_id("mod_b")];
        let hidden = HashSet::new();

        let mut origins: OriginMap = HashMap::new();
        origins
            .entry("textures/sky.dds".into())
            .or_default()
            .insert(
                mod_id("mod_a"),
                FileOrigin::Archive {
                    archive_rel: "mod_a.bsa".into(),
                },
            );
        origins
            .entry("textures/sky.dds".into())
            .or_default()
            .insert(mod_id("mod_b"), FileOrigin::Loose);

        let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
        assert_eq!(report.loose_vs_archive.len(), 1);
    }

    #[test]
    fn three_way_collision() {
        let mut cm = ConflictMap::default();
        cm.register("textures/sky.dds".into(), mod_id("mod_a"));
        cm.register("textures/sky.dds".into(), mod_id("mod_b"));
        cm.register("textures/sky.dds".into(), mod_id("mod_c"));

        let order = vec![mod_id("mod_a"), mod_id("mod_b"), mod_id("mod_c")];
        let hidden = HashSet::new();
        let origins = HashMap::new();

        let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
        // mod_c wins, mod_a and mod_b both lose → 2 collisions, 2 pairs
        assert_eq!(report.total_collisions, 2);
        assert_eq!(report.pairs.len(), 2);
    }

    #[test]
    fn hidden_file_excluded_from_winner() {
        let mut cm = ConflictMap::default();
        cm.register("textures/sky.dds".into(), mod_id("mod_a"));
        cm.register("textures/sky.dds".into(), mod_id("mod_b"));

        let order = vec![mod_id("mod_a"), mod_id("mod_b")];
        let mut hidden = HashSet::new();
        hidden.insert(("mod_b".to_string(), "textures/sky.dds".to_string()));
        let origins = HashMap::new();

        let report = analyze_collisions(&cm, &order, &hidden, &origins, &TestClassifier);
        // mod_b is hidden, so mod_a wins — still a collision pair though
        assert_eq!(report.pairs.len(), 1);
        assert_eq!(report.pairs[0].files[0].winner, mod_id("mod_a"));
    }
}
