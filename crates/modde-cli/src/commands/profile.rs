use anyhow::{Context, Result};
use smallvec::SmallVec;
use tracing::info;

use modde_core::error::CoreError;
use modde_core::profile::{
    ActivateResult, LoadOrderLock, LockReason, Profile, ProfileManager, ProfileSource,
};
use modde_core::save::SaveFingerprint;

use super::{compute_fingerprint, resolve_save_dir};
use crate::ProfileAction;

/// Human-readable byte size (KB/MB/GB) for `lock-info` output.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Render a `LockReason` as a short, human-readable phrase for CLI output.
fn format_lock_reason(reason: &LockReason) -> String {
    match reason {
        LockReason::Wabbajack { manifest_hash } => format!("Wabbajack (hash {manifest_hash})"),
        LockReason::NexusCollection { slug, version } => {
            format!("Nexus Collection '{slug}' v{version}")
        }
        LockReason::TomlImport { source_path } => format!("TOML import from {source_path}"),
        LockReason::Manual { note: Some(n) } => format!("manual ({n})"),
        LockReason::Manual { note: None } => "manual".to_string(),
    }
}

/// Look up a mod in a profile by `mod_id` and return its index, or construct
/// a typed [`CoreError::ModNotFound`] whose `candidates` field lists the
/// first 5 mod ids in the profile as a hint.
fn find_mod_or_bail(profile: &Profile, mod_id: &str) -> Result<usize> {
    if let Some(idx) = profile.mods.iter().position(|m| m.mod_id == mod_id) {
        return Ok(idx);
    }
    let candidates: SmallVec<[String; 5]> = profile
        .mods
        .iter()
        .take(5)
        .map(|m| m.mod_id.clone())
        .collect();
    Err(CoreError::ModNotFound {
        profile: profile.name.clone(),
        mod_id: mod_id.to_string(),
        candidates,
    }
    .into())
}

pub fn handle(action: ProfileAction) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;

    match action {
        ProfileAction::List { game } => {
            let profiles = match game {
                Some(ref g) => pm.list_for_game(g)?,
                None => pm.list()?,
            };
            if profiles.is_empty() {
                println!("No profiles found.");
            } else {
                for p in profiles {
                    println!(
                        "  {} (game: {}, {} mods, source: {})",
                        p.name, p.game_id, p.mod_count, p.source_type
                    );
                }
            }
        }
        ProfileAction::Switch { name, game } => {
            let save_dir = resolve_save_dir(&game);
            let fp = compute_fingerprint(&pm, &name, &game);
            match pm.activate_with_fingerprint(&name, &game, save_dir.as_deref(), fp.as_ref())? {
                ActivateResult::Activated => {
                    info!(profile = %name, "switched to profile");
                    if save_dir.is_some() {
                        println!("Switched to profile: {name} (saves swapped)");
                    } else {
                        println!("Switched to profile: {name} (no save directory detected)");
                    }
                }
                ActivateResult::AdoptionRequired { save_count } => {
                    println!(
                        "Found {save_count} existing save(s) in the game directory.\n\
                         Run `modde save adopt --game {game} --profile {name}` to adopt them first,\n\
                         or use a different profile."
                    );
                }
            }
        }
        ProfileAction::Create { name, game } => {
            let profile = Profile {
                id: None,
                name: name.clone(),
                game_id: modde_core::GameId::from(game.clone()),
                source: ProfileSource::Manual,
                mods: vec![],
                overrides: ProfileManager::default_overrides(&name),
                load_order_rules: smallvec::SmallVec::new(),
                load_order_lock: None,
            };
            pm.create(&profile)?;
            println!("Created profile: {name} (game: {game})");
        }
        ProfileAction::Delete { name, game } => {
            pm.delete(&name, game.as_deref())?;
            println!("Deleted profile: {name}");
        }
        ProfileAction::Try { name, game } => {
            let save_dir = resolve_save_dir(&game);
            let fp = compute_fingerprint(&pm, &name, &game);
            pm.try_profile_with_fingerprint(&name, &game, save_dir.as_deref(), fp.as_ref())?;
            let depth = pm.active(&game)?.map(|a| a.experiment_depth).unwrap_or(0);
            println!("Experimenting with profile: {name} (stack depth: {depth})");
            println!(
                "Use `modde profile rollback --game {game}` to undo, or `modde profile commit --game {game}` to accept."
            );
        }
        ProfileAction::Rollback { game } => {
            let save_dir = resolve_save_dir(&game);

            // Compute fingerprint for the current (about-to-be-rolled-back) profile
            let fp = pm.active(&game)?.and_then(|info| {
                let game_plugin = modde_games::resolve_game_plugin(&game)?;
                let staging_dir = ProfileManager::staging_dir(&info.profile.name);
                Some(SaveFingerprint::compute(&info.profile.mods, |mod_id| {
                    let mod_path = staging_dir.join(mod_id);
                    game_plugin.classify_mod(&mod_path).affects_saves()
                }))
            });

            let restored = pm.rollback_with_fingerprint(&game, save_dir.as_deref(), fp.as_ref())?;
            println!("Rolled back to profile: {restored}");
        }
        ProfileAction::Commit { game } => {
            pm.commit(&game)?;
            println!("Experiment accepted. Rollback stack cleared for game: {game}");
        }
        ProfileAction::Active { game } => {
            match pm.active(&game)? {
                Some(info) => {
                    println!(
                        "Active profile: {} (game: {})",
                        info.profile.name, info.profile.game_id
                    );
                    println!("  Mods: {}", info.profile.mods.len());
                    if info.experiment_depth > 0 {
                        println!(
                            "  Experiment depth: {} (use `rollback` to undo or `commit` to accept)",
                            info.experiment_depth
                        );
                    }

                    // Show fingerprint info
                    if let Some(fp) =
                        compute_fingerprint(&pm, &info.profile.name, info.profile.game_id.as_str())
                    {
                        if !fp.is_empty() {
                            println!(
                                "  Save fingerprint: {} ({} save-breaking mod(s))",
                                fp.short_hash(),
                                fp.mod_ids.len()
                            );
                        } else {
                            println!("  Save fingerprint: none (no save-breaking mods)");
                        }
                    }
                }
                None => {
                    println!("No active profile for game: {game}");
                }
            }
        }
        ProfileAction::Fork {
            source,
            name,
            game,
            unlock,
        } => {
            let id = pm.fork_with_options(
                &source,
                &name,
                &game,
                modde_core::profile::ForkOptions { unlock },
            )?;
            if unlock {
                println!(
                    "Forked profile '{source}' -> '{name}' (id: {id}, mods + saves cloned, locks stripped)"
                );
            } else {
                println!("Forked profile '{source}' -> '{name}' (id: {id}, mods + saves cloned)");
            }
        }
        ProfileAction::Lock { name, game, note } => {
            let mut profile = pm.load(&name, game.as_deref())?;
            if let Some(existing) = profile.load_order_lock.as_ref() {
                anyhow::bail!(
                    "profile '{name}' is already locked by {} — unlock first to re-lock",
                    format_lock_reason(&existing.reason)
                );
            }
            profile.load_order_lock = Some(LoadOrderLock::now(LockReason::Manual {
                note: note.clone(),
            }));
            pm.update(&profile)?;
            match note {
                Some(n) => println!("Locked profile '{name}' (manual: {n})"),
                None => println!("Locked profile '{name}' (manual)"),
            }
        }
        ProfileAction::Unlock { name, game } => {
            let mut profile = pm.load(&name, game.as_deref())?;
            match profile.load_order_lock.take() {
                None => println!("Profile '{name}' was not locked."),
                Some(prior) => {
                    pm.update(&profile)?;
                    println!(
                        "Unlocked profile '{name}' (was {}, locked at {})",
                        format_lock_reason(&prior.reason),
                        prior.locked_at
                    );
                }
            }
        }
        ProfileAction::LockInfo { name, game } => {
            let profile = pm.load(&name, game.as_deref())?;
            match profile.load_order_lock.as_ref() {
                None => println!("Profile '{name}' is not locked."),
                Some(lock) => {
                    println!("Profile '{name}' lock:");
                    println!("  Reason:    {}", format_lock_reason(&lock.reason));
                    println!("  Locked at: {}", lock.locked_at);
                    // For Wabbajack locks, also show the cached source file
                    // status so the user can re-verify / re-import from it.
                    if let LockReason::Wabbajack { manifest_hash } = &lock.reason {
                        let cache_path = modde_core::paths::wabbajack_cache_path(manifest_hash);
                        match std::fs::metadata(&cache_path) {
                            Ok(meta) => println!(
                                "  Source:    {} ({})",
                                cache_path.display(),
                                format_bytes(meta.len())
                            ),
                            Err(_) => println!(
                                "  Source:    {} (missing — re-run \
                                 `modde scan --manifest <file> --import-to {name}`)",
                                cache_path.display()
                            ),
                        }
                    }
                }
            }
            let pinned: Vec<&str> = profile
                .mods
                .iter()
                .filter(|m| m.lock.is_some())
                .map(|m| m.mod_id.as_str())
                .collect();
            if !pinned.is_empty() {
                println!("  Per-mod pins ({}):", pinned.len());
                for id in pinned.iter().take(20) {
                    println!("    - {id}");
                }
                if pinned.len() > 20 {
                    println!("    ... and {} more", pinned.len() - 20);
                }
            }
        }
        ProfileAction::LockMod {
            name,
            mod_id,
            game,
            note,
        } => {
            // A per-mod pin is independent of the profile-level lock. If the
            // profile is already Wabbajack-locked, lock-mod still succeeds —
            // the pin takes effect after a later `unlock` or `fork --unlock`.
            let mut profile = pm.load(&name, game.as_deref())?;
            let idx = find_mod_or_bail(&profile, &mod_id)?;
            if let Some(existing) = profile.mods[idx].lock.as_ref() {
                anyhow::bail!(
                    "mod '{mod_id}' is already pinned ({}) — unlock-mod first to re-pin",
                    format_lock_reason(existing)
                );
            }
            profile.mods[idx].lock = Some(LockReason::Manual { note: note.clone() });
            pm.update(&profile)?;
            match note {
                Some(n) => println!("Pinned '{mod_id}' in profile '{name}' (manual: {n})"),
                None => println!("Pinned '{mod_id}' in profile '{name}'"),
            }
        }
        ProfileAction::UnlockMod { name, mod_id, game } => {
            let mut profile = pm.load(&name, game.as_deref())?;
            let idx = find_mod_or_bail(&profile, &mod_id)?;
            match profile.mods[idx].lock.take() {
                None => println!("'{mod_id}' was not pinned"),
                Some(prior) => {
                    pm.update(&profile)?;
                    println!("Unpinned '{mod_id}' (was {})", format_lock_reason(&prior));
                }
            }
        }
        ProfileAction::Dedup {
            name,
            game,
            manifest,
            apply,
        } => {
            dedup(&pm, &name, game.as_deref(), manifest.as_deref(), apply)?;
        }
    }

    Ok(())
}

/// Handle `modde profile dedup`. Splits into layer-1 (heuristic, no
/// manifest) and layer-2 (authoritative, manifest-backed) paths, both
/// with optional `--apply`.
fn dedup(
    pm: &ProfileManager,
    name: &str,
    game: Option<&str>,
    manifest_path: Option<&std::path::Path>,
    apply: bool,
) -> Result<()> {
    let mut profile = pm.load(name, game)?;

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
    let leaked_set: std::collections::HashSet<&str> =
        report.leaked.iter().map(|s| s.as_str()).collect();
    let before = profile.mods.len();
    profile
        .mods
        .retain(|m| !leaked_set.contains(m.mod_id.as_str()));
    let deleted = before - profile.mods.len();

    pm.update(&profile).context("failed to save profile")?;

    info!(%name, deleted, "pruned leaked filesystem-scanner duplicates");
    println!(
        "\nDeleted {deleted} LEAKED row(s). Profile '{name}' now has {} mods.",
        profile.mods.len()
    );

    Ok(())
}
