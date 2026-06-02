use anyhow::{Context, Result};

use modde_core::backup::BackupManager;
use modde_core::resolver::{GameId, ModId};

use crate::BackupAction;

pub async fn handle(action: BackupAction) -> Result<()> {
    let mgr = BackupManager::new().context("failed to initialise backup manager")?;

    match action {
        BackupAction::Create { mod_id } => {
            let store_mod = modde_core::paths::store_dir().join(&mod_id);
            if !store_mod.exists() {
                anyhow::bail!(
                    "mod directory not found at '{}'. Is the mod installed?",
                    store_mod.display()
                );
            }

            let entry = mgr
                .create_mod_backup(&ModId::from(mod_id.as_str()), &store_mod)
                .context("failed to create backup")?;

            println!("Backup created: {}", entry.name);
            println!("  Path: {}", entry.path.display());
        }
        BackupAction::Restore { mod_id } => {
            let store_mod = modde_core::paths::store_dir().join(&mod_id);
            let entry = mgr
                .restore_mod_backup(&ModId::from(mod_id.as_str()), &store_mod)
                .context("failed to restore backup")?;

            println!("Restored mod '{}' from backup: {}", mod_id, entry.name);
        }
        BackupAction::List { mod_id } => {
            let entries = mgr
                .list_mod_backups(&ModId::from(mod_id.as_str()))
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
            let pm = modde_core::profile::ProfileManager::open().await?;
            let prof = super::load_profile_or_default(&pm, Some(&profile), Some(&game)).await?;
            let plugins = super::load_plugin_order(&pm, &prof).await?;
            if plugins.is_empty() {
                anyhow::bail!("no real plugin order found in the database or plugins.txt");
            }

            let path = mgr
                .backup_plugin_order(&profile, &GameId::from(game.as_str()), &plugins)
                .context("failed to backup plugin order")?;

            println!("Plugin order backed up ({} plugins)", plugins.len());
            println!("  Path: {}", path.display());
        }
        BackupAction::RestorePlugins { profile, game } => {
            let plugins = mgr
                .restore_plugin_order(&profile, &GameId::from(game.as_str()))
                .context("failed to restore plugin order")?;

            let pm = modde_core::profile::ProfileManager::open().await?;
            let prof = super::load_profile_or_default(&pm, Some(&profile), Some(&game)).await?;
            super::persist_plugin_order(&pm, &prof, &plugins)
                .await
                .context("failed to apply restored plugin order")?;

            println!("Restored plugin order ({} plugins):", plugins.len());
            for (i, plugin) in plugins.iter().enumerate() {
                let enabled = if plugin.enabled {
                    "enabled"
                } else {
                    "disabled"
                };
                println!("  {}: {} ({enabled})", i + 1, plugin.plugin_name);
            }
        }
    }

    Ok(())
}
