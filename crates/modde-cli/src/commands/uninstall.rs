//! `modde mod remove` — the inverse of `modde install mod`.
//!
//! Uses the V8 installer pipeline's `installed_mod_files` manifest to
//! remove precisely the files that were staged by a given mod. Unknown
//! and pending-user-input rows can still be removed (their `rel_path`
//! list is just the raw extracted tree for pending, or empty for
//! unknown — both cases are safe).

use anyhow::{Context, Result, bail};
use tracing::{info, warn};

use modde_core::ModdeDb;
use modde_core::installer::dossiers_dir;
use modde_core::paths;
use modde_core::profile::ProfileManager;
use modde_core::resolver::{GameId, ModId};
use modde_games::{ModSafety, SaveDependencyKind, SaveRemovalGateReport};

/// Remove `mod_id` from `profile_name`. If `profile_name` is `None`,
/// the unambiguous default profile is used.
pub async fn handle(
    mod_id: String,
    profile_name: Option<String>,
    dry_run: bool,
    force_contaminate: bool,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let mut profile = super::load_profile_or_default(&pm, profile_name.as_deref(), None).await?;
    let profile_id = profile
        .id
        .ok_or_else(|| anyhow::anyhow!("loaded profile has no database id"))?;

    // Refuse to remove from a locked profile. The lock exists to prevent
    // drift from an authoritative source (Wabbajack manifest / Nexus
    // collection); removing a mod from under the lock would break that
    // invariant. The user must unlock first.
    if let Some(lock) = profile.load_order_lock.as_ref() {
        bail!(
            "profile '{}' is locked ({:?}). Unlock with `modde profile unlock` first.",
            profile.name,
            lock.reason
        );
    }

    if let Some(report) =
        analyze_save_removal_gate(&profile.name, profile.game_id.as_str(), &mod_id)
            .context("failed to analyze save-game removal safety")?
    {
        print_gate_report(&report, dry_run || report.is_blocked() || force_contaminate);
        if dry_run {
            return Ok(());
        }
        if report.is_blocked() && !force_contaminate {
            bail!(
                "refusing to remove '{mod_id}' because saved playthroughs depend on it. \
                 Fork the profile or rerun with --force-contaminate to accept permanent save contamination."
            );
        }
        if report.is_blocked() {
            eprintln!(
                "WARNING: forcing removal of '{mod_id}' even though saves depend on it; affected saves may be permanently contaminated."
            );
        }
    } else if dry_run {
        println!(
            "No record-level save dependency analyzer is available for game '{}'. No removal performed.",
            profile.game_id
        );
        return Ok(());
    }

    // Pull the file manifest before we touch anything, then drop the
    // rows + the profile_mods entry in one transaction.
    let db = ModdeDb::open().await.context("failed to open mod db")?;
    let staged_files = db
        .remove_installed_mod(profile_id, &ModId::from(mod_id.as_str()))
        .await
        .context("failed to clear installed_mod_files rows")?;

    // Wipe the mod's store directory. The store dir name convention
    // matches the ids we use at install time: `<domain>_<mod>_<file>`.
    // We stored each file under `store/<mod_id>/<rel_path>`, so the
    // top-level store dir is `store/<mod_id>`.
    let store_mod_dir = paths::store_dir().join(&mod_id);
    if store_mod_dir.exists()
        && let Err(e) = std::fs::remove_dir_all(&store_mod_dir)
    {
        warn!(
            path = %store_mod_dir.display(),
            error = %e,
            "failed to delete store dir; leaving orphaned files behind"
        );
    }

    // Finally, strip the mod row from the in-memory profile and persist
    // the slimmer version. `remove_installed_mod` already dropped the
    // row from `profile_mods`, but the profile we loaded is stale —
    // `pm.update()` below rewrites the mods table from the in-memory
    // list so we need to drop it there too.
    profile.mods.retain(|m| m.mod_id != mod_id);
    pm.update(&profile)
        .await
        .context("failed to persist profile after remove")?;

    info!(%mod_id, files = staged_files.len(), "mod removed");
    println!(
        "Removed '{mod_id}' from profile '{}' ({} staged files tracked).",
        profile.name,
        staged_files.len()
    );

    Ok(())
}

fn analyze_save_removal_gate(
    profile_name: &str,
    game_id: &str,
    mod_id: &str,
) -> Result<Option<SaveRemovalGateReport>> {
    let Some(analyzer) = modde_games::resolve_save_dependency_analyzer(game_id) else {
        return Ok(None);
    };

    let mod_dir = ProfileManager::staging_dir(profile_name).join(mod_id);
    let mut save_roots = Vec::new();
    if let Some(save_dir) = super::resolve_save_dir(game_id) {
        save_roots.push(save_dir);
    }
    let vault_dir = paths::save_vault_dir(&GameId::from(game_id));
    if vault_dir.exists() {
        save_roots.push(vault_dir);
    }

    let report = analyzer.analyze_mod_removal(mod_id, &mod_dir, &save_roots)?;
    Ok(Some(report))
}

fn print_gate_report(report: &SaveRemovalGateReport, verbose: bool) {
    let safety = match report.safety {
        ModSafety::SaveBreaking => "save-breaking",
        ModSafety::SaveSafe => "save-safe",
        ModSafety::Unknown => "unknown",
    };
    println!(
        "Save removal gate for '{}': {safety}, {} save file(s) analyzed.",
        report.mod_id, report.analyzed_saves
    );

    if !verbose {
        return;
    }

    for warning in &report.warnings {
        eprintln!("  warning: {warning}");
    }

    if report.blocking_findings.is_empty() {
        println!("  No save records tied to this mod were found.");
        return;
    }

    eprintln!("  Blocking save dependencies:");
    for finding in &report.blocking_findings {
        let source = finding
            .source_file
            .as_deref()
            .map(|s| format!(" via {s}"))
            .unwrap_or_default();
        let kind = match finding.dependency_kind {
            SaveDependencyKind::PluginRecord => "plugin record",
            SaveDependencyKind::PapyrusScript => "Papyrus script",
            SaveDependencyKind::ActiveScript => "active script",
            SaveDependencyKind::UnattachedInstance => "unattached instance",
            SaveDependencyKind::UndefinedElement => "undefined element",
            SaveDependencyKind::ParseIncomplete => "parse incomplete",
        };
        eprintln!(
            "    {}: {kind} '{}'{} (confidence {:.2})",
            finding.save_path.display(),
            finding.symbol,
            source,
            finding.confidence
        );
    }
}

/// `modde mod diagnose <mod_id>` — locate the skill dossier for an
/// unknown-install mod and dump its `PROMPT.md` to stdout so it can be
/// piped into `claude` or pasted into a chat manually.
pub async fn handle_diagnose(mod_id: String) -> Result<()> {
    // The dossier slug is usually `<domain>_<mod>_<file>` which is the
    // same as the profile mod_id for Nexus installs; just look for a
    // dir with that name under the dossiers root. Users can also pass
    // a plain Nexus `<domain>_<mod_id>` prefix.
    let root = dossiers_dir();
    if !root.exists() {
        bail!(
            "no dossiers directory at {} — nothing to diagnose",
            root.display()
        );
    }

    let mut best: Option<std::path::PathBuf> = None;
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == mod_id || name.starts_with(&mod_id) {
            best = Some(entry.path());
            break;
        }
    }

    let dossier = best
        .ok_or_else(|| anyhow::anyhow!("no dossier matching '{mod_id}' in {}", root.display()))?;

    let prompt_path = dossier.join("PROMPT.md");
    if !prompt_path.exists() {
        bail!("dossier at {} is missing PROMPT.md", dossier.display());
    }
    println!("Dossier: {}", dossier.display());
    println!("----- PROMPT.md -----");
    let body = std::fs::read_to_string(&prompt_path)?;
    print!("{body}");
    Ok(())
}
