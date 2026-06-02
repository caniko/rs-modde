use anyhow::{Context, Result};

use modde_core::profile::ProfileManager;
use modde_core::vfs;

use super::load_profile_or_default;

pub async fn handle(profile_name: Option<String>, game_id: Option<String>) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let name = profile.name;
    vfs::rollback(&name).await?;
    println!("Rolled back profile: {name}");
    Ok(())
}
