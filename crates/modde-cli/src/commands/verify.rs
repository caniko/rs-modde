use anyhow::{Context, Result};
use tracing::{error, info, warn};

use modde_core::fs::walk_files;
use modde_core::hash;
use modde_core::paths;
use modde_core::profile::ProfileManager;

use super::load_profile_or_default;

pub async fn handle(profile_name: Option<String>, game_id: Option<String>) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;

    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref())?;

    let name = &profile.name;
    info!(profile = %name, game = %profile.game_id, "verifying installed files");

    let store_dir = paths::store_dir();
    let mut total_files: u64 = 0;
    let mut mismatches: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for enabled_mod in &profile.mods {
        if !enabled_mod.enabled {
            continue;
        }

        let mod_dir = store_dir.join(&enabled_mod.mod_id);
        if !mod_dir.exists() {
            warn!(mod_id = %enabled_mod.mod_id, "mod directory not found in store");
            missing.push(enabled_mod.mod_id.clone());
            continue;
        }

        let files = walk_files(&mod_dir)?;
        for file_path in &files {
            total_files += 1;

            if !file_path.exists() {
                missing.push(format!("{}:{}", enabled_mod.mod_id, file_path.display()));
                continue;
            }

            match hash::hash_file_xxhash(file_path).await {
                Ok(_) => {
                    if file_path.symlink_metadata().is_ok() && file_path.is_symlink() {
                        match tokio::fs::read_link(file_path).await {
                            Ok(target) => {
                                if !target.exists() {
                                    warn!(
                                        file = %file_path.display(),
                                        target = %target.display(),
                                        "broken symlink detected"
                                    );
                                    mismatches.push(format!(
                                        "{}:{} (broken symlink -> {})",
                                        enabled_mod.mod_id,
                                        file_path.display(),
                                        target.display()
                                    ));
                                }
                            }
                            Err(e) => {
                                error!(file = %file_path.display(), error = %e, "failed to read symlink");
                                mismatches.push(format!(
                                    "{}:{} (symlink read error: {e})",
                                    enabled_mod.mod_id,
                                    file_path.display()
                                ));
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(file = %file_path.display(), error = %e, "failed to hash file");
                    mismatches.push(format!(
                        "{}:{} (hash error: {e})",
                        enabled_mod.mod_id,
                        file_path.display()
                    ));
                }
            }
        }
    }

    println!("Verification results for profile '{name}':");
    println!("  Total files checked: {total_files}");
    println!("  Missing mods/files:  {}", missing.len());
    println!("  Mismatches/errors:   {}", mismatches.len());

    if !missing.is_empty() {
        println!("\nMissing:");
        for m in &missing {
            println!("  - {m}");
        }
    }

    if !mismatches.is_empty() {
        println!("\nMismatches:");
        for m in &mismatches {
            println!("  - {m}");
        }
    }

    if missing.is_empty() && mismatches.is_empty() {
        println!("\nAll files OK.");
    }

    Ok(())
}
