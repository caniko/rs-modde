use anyhow::{Context, Result};

use modde_core::backup::BackupManager;

use crate::BackupAction;

pub fn handle(action: BackupAction) -> Result<()> {
    let mgr = BackupManager::new().context("failed to initialise backup manager")?;

    match action {
        BackupAction::Create { mod_id } => {
            let staging = modde_core::paths::data_dir().join("staging").join(&mod_id);
            if !staging.exists() {
                anyhow::bail!(
                    "mod directory not found at '{}'. Is the mod installed?",
                    staging.display()
                );
            }

            let entry = mgr
                .create_mod_backup(&mod_id, &staging)
                .context("failed to create backup")?;

            println!("Backup created: {}", entry.name);
            println!("  Path: {}", entry.path.display());
        }
        BackupAction::Restore { mod_id } => {
            let staging = modde_core::paths::data_dir().join("staging").join(&mod_id);
            let entry = mgr
                .restore_mod_backup(&mod_id, &staging)
                .context("failed to restore backup")?;

            println!("Restored mod '{}' from backup: {}", mod_id, entry.name);
        }
        BackupAction::List { mod_id } => {
            let entries = mgr
                .list_mod_backups(&mod_id)
                .context("failed to list backups")?;

            if entries.is_empty() {
                println!("No backups found for mod '{mod_id}'.");
            } else {
                println!("Backups for '{mod_id}':");
                for entry in &entries {
                    println!("  {} ({})", entry.name, entry.path.display());
                }
            }
        }
        BackupAction::Plugins { profile, game } => {
            // Read the mod list from the profile and store it as the plugin order
            let pm = modde_core::profile::ProfileManager::open()?;
            let prof = super::load_profile_or_default(&pm, Some(&profile), Some(&game))?;
            let plugins: Vec<String> = prof
                .mods
                .iter()
                .filter(|m| m.enabled)
                .map(|m| m.mod_id.clone())
                .collect();

            let path = mgr
                .backup_plugin_order(&profile, &game, &plugins)
                .context("failed to backup plugin order")?;

            println!("Plugin order backed up ({} plugins)", plugins.len());
            println!("  Path: {}", path.display());
        }
        BackupAction::RestorePlugins { profile, game } => {
            let plugins = mgr
                .restore_plugin_order(&profile, &game)
                .context("failed to restore plugin order")?;

            println!("Restored plugin order ({} plugins):", plugins.len());
            for (i, p) in plugins.iter().enumerate() {
                println!("  {}: {p}", i + 1);
            }
        }
    }

    Ok(())
}
