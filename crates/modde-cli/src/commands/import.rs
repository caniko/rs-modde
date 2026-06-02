use anyhow::{Context, Result};

use modde_core::paths;
use modde_core::profile::ProfileManager;

pub async fn handle() -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;

    let profiles_dir = paths::modde_data_dir().join("profiles");
    if !profiles_dir.exists() {
        println!(
            "No legacy profiles directory found at {}",
            profiles_dir.display()
        );
        return Ok(());
    }

    let count = pm.import_toml(&profiles_dir).await?;
    if count == 0 {
        println!("No TOML profiles found to import.");
    } else {
        println!("Imported {count} profile(s) from TOML files.");
    }

    Ok(())
}
