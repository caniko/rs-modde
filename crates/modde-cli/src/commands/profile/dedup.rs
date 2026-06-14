//! Profile duplicate-pruning workflow.

use super::*;

pub(super) async fn dedup(
    pm: &ProfileManager,
    name: &str,
    game: Option<&str>,
    manifest_path: Option<&std::path::Path>,
    apply: bool,
) -> Result<()> {
    let mut profile = pm.load(name, game.map(GameId::from).as_ref()).await?;

    // ── Layer 1: pure-DB heuristic ────────────────────────────────
    //
    // Flag any filesystem-scanner-prefixed rows. On a locked profile
    // these are always suspects; on an unlocked profile they're just
    // informational (the user may have legitimately built a manual
    // profile from a scan without a manifest).
    let suspects: Vec<&str> = profile
        .mods
        .iter()
        .map(|m| m.mod_id.as_str())
        .filter(|id| {
            id.starts_with("cet/")
                || id.starts_with("reds/")
                || id.starts_with("tweak/")
                || id.starts_with("archive/")
                || id.starts_with("redmod/")
        })
        .collect();

    let lock_status = match profile.load_order_lock.as_ref() {
        Some(lock) => format!("locked by {}", format_lock_reason(&lock.reason)),
        None => "unlocked".to_string(),
    };
    println!(
        "Profile '{name}' (game: {}, {lock_status}): {} mods, {} filesystem-scanner suspects",
        profile.game_id,
        profile.mods.len(),
        suspects.len()
    );

    if suspects.is_empty() {
        println!("No filesystem-scanner rows found — nothing to dedup.");
        return Ok(());
    }

    // ── Layer 2: manifest-backed classification ───────────────────
    //
    // Without a manifest we can't tell leaked duplicates from genuine
    // additions — just report the suspects and bail.
    let Some(manifest_path) = manifest_path else {
        println!("\nSuspects (layer-1, heuristic only):");
        for id in suspects.iter().take(40) {
            println!("  - {id}");
        }
        if suspects.len() > 40 {
            println!("  ... and {} more", suspects.len() - 40);
        }
        println!(
            "\nPass --manifest <path.wabbajack> to classify suspects as LEAKED \
             or GENUINE and (with --apply) delete the LEAKED rows."
        );
        if apply {
            anyhow::bail!("--apply requires --manifest to classify rows before deleting");
        }
        return Ok(());
    };

    let wj_manifest = modde_sources::wabbajack::manifest::parse_wabbajack_file(manifest_path)
        .with_context(|| format!("failed to parse manifest: {}", manifest_path.display()))?;

    // Resolve the per-game footprint mapping once. For games we don't
    // yet support, `mod_id_footprint` returns None for every row, which
    // means layer-2 classification is a no-op and we surface it cleanly.
    let scanner = modde_games::resolve_mod_scanner(profile.game_id.as_str()).ok_or_else(|| {
        anyhow::anyhow!("no mod scanner available for game '{}'", profile.game_id)
    })?;

    let report = modde_core::scanner::detect_stale_duplicates(&profile, &wj_manifest, |mod_id| {
        scanner.mod_id_footprint(mod_id)
    });

    println!(
        "\nManifest: {} by {} ({} archives, {} directives)",
        wj_manifest.name,
        wj_manifest.author,
        wj_manifest.archives.len(),
        wj_manifest.directives.len(),
    );
    println!(
        "Classification: {} leaked duplicate(s), {} genuine addition(s)",
        report.leaked.len(),
        report.genuine.len(),
    );

    if !report.leaked.is_empty() {
        println!("\nLEAKED (safe to delete):");
        for id in report.leaked.iter().take(40) {
            println!("  - {id}");
        }
        if report.leaked.len() > 40 {
            println!("  ... and {} more", report.leaked.len() - 40);
        }
    }
    if !report.genuine.is_empty() {
        println!("\nGENUINE (kept):");
        for id in report.genuine.iter().take(40) {
            println!("  - {id}");
        }
        if report.genuine.len() > 40 {
            println!("  ... and {} more", report.genuine.len() - 40);
        }
    }

    if !apply {
        println!("\n[DRY RUN] Pass --apply to delete the LEAKED rows.");
        return Ok(());
    }

    if report.leaked.is_empty() {
        println!("\nNothing to delete.");
        return Ok(());
    }

    // Persist: remove leaked rows from the in-memory profile and save.
    // `update_profile` does DELETE + re-INSERT of all profile_mods, so
    // sort_index is automatically contiguous after the prune.
    let leaked_set: std::collections::HashSet<&str> = report
        .leaked
        .iter()
        .map(std::string::String::as_str)
        .collect();
    let before = profile.mods.len();
    profile
        .mods
        .retain(|m| !leaked_set.contains(m.mod_id.as_str()));
    let deleted = before - profile.mods.len();

    pm.update(&profile)
        .await
        .context("failed to save profile")?;

    info!(%name, deleted, "pruned leaked filesystem-scanner duplicates");
    println!(
        "\nDeleted {deleted} LEAKED row(s). Profile '{name}' now has {} mods.",
        profile.mods.len()
    );

    Ok(())
}
