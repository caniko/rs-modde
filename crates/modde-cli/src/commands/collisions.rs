use std::collections::HashSet;

use anyhow::{Context, Result};
use tracing::info;

use modde_core::collision::{self, CollisionSeverity};
use modde_core::paths;
use modde_core::profile::ProfileManager;
use modde_core::resolver::{self, ModId};

use super::load_profile_or_default;

pub async fn handle(
    profile_name: Option<String>,
    game_id: Option<String>,
    show_all: bool,
    suggest_hides: bool,
) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref())?;

    let classifier = modde_games::resolve_collision_classifier(&profile.game_id)
        .ok_or_else(|| anyhow::anyhow!(
            "no collision classifier for game '{}'", profile.game_id
        ))?;

    let resolved = resolver::resolve(&profile)
        .context("failed to resolve load order")?;

    let store = paths::store_dir();

    let (conflict_map, origins) = collision::build_full_conflict_map(
        &store,
        &resolved.order,
        classifier.as_ref(),
    )
    .context("failed to build conflict map")?;

    // Load hidden files.
    let hidden: HashSet<(String, String)> = profile
        .id
        .and_then(|pid| {
            let h = pm.db().list_hidden_files(pid).ok()?;
            Some(h.into_iter().map(|f| (f.mod_id, f.rel_path)).collect())
        })
        .unwrap_or_default();

    let report = collision::analyze_collisions(
        &conflict_map,
        &resolved.order,
        &hidden,
        &origins,
        classifier.as_ref(),
    );

    // ── Print report ────────────────────────────────────────────────

    println!(
        "Collision Report for profile \"{}\" ({})",
        profile.name, profile.game_id
    );
    println!("{}", "=".repeat(60));
    println!(
        "{} file collisions across {} mod pairs\n",
        report.total_collisions,
        report.pairs.len()
    );

    if report.pairs.is_empty() {
        println!("No collisions detected.");
        return Ok(());
    }

    for pair in &report.pairs {
        // Skip cosmetic collisions unless --all
        if !show_all && pair.max_severity == CollisionSeverity::Cosmetic {
            continue;
        }

        println!(
            "[{}] {} vs {} ({} files)",
            pair.max_severity,
            pair.loser,
            pair.winner,
            pair.files.len()
        );

        for fc in &pair.files {
            let origin_tag = match (&fc.winner_origin, &fc.loser_origin) {
                (collision::FileOrigin::Loose, collision::FileOrigin::Archive { .. }) => {
                    " [loose > archive]"
                }
                (collision::FileOrigin::Archive { .. }, collision::FileOrigin::Loose) => {
                    " [archive > loose]"
                }
                _ => "",
            };
            let hidden_tag = if fc.is_loser_hidden { " (hidden)" } else { "" };
            println!(
                "  {}  ->  winner: {}{}{}",
                fc.file_path, fc.winner, origin_tag, hidden_tag
            );
        }
        println!();
    }

    // Shadowed mods.
    if !report.shadowed_mods.is_empty() {
        println!("Shadowed mods (all files overridden):");
        for sm in &report.shadowed_mods {
            let by: Vec<&str> = sm.shadowed_by.iter().map(ModId::as_str).collect();
            println!(
                "  - \"{}\" ({} files, all overridden by {})",
                sm.mod_id,
                sm.file_count,
                by.join(", ")
            );
        }
        println!();
    }

    // Loose vs archive.
    if !report.loose_vs_archive.is_empty() {
        println!(
            "Loose vs archive conflicts: {} files",
            report.loose_vs_archive.len()
        );
        info!(
            count = report.loose_vs_archive.len(),
            "loose files overriding archive contents — may indicate unintended overrides"
        );
        println!();
    }

    // Redundant files.
    if !report.redundant_files.is_empty() {
        println!(
            "Redundant files (never win): {}",
            report.redundant_files.len()
        );
        if suggest_hides {
            println!("Suggested hide commands:");
            for (mod_id, file_path) in &report.redundant_files {
                println!("  modde profile hide \"{}\" \"{}\"", mod_id, file_path);
            }
        } else {
            println!("  Run with --suggest-hides to get hide commands");
        }
        println!();
    }

    Ok(())
}
