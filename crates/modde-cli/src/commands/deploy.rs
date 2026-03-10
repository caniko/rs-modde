use anyhow::Result;
use tracing::info;

use modde_core::profile::ProfileManager;

pub async fn handle(profile_name: Option<String>) -> Result<()> {
    let pm = ProfileManager::new(ProfileManager::default_dir());

    let name = match profile_name {
        Some(n) => n,
        None => {
            let profiles = pm.list()?;
            profiles
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("no profiles found"))?
        }
    };

    let profile = pm.load(&name)?;
    info!(profile = %name, game = %profile.game_id, "deploying profile");

    // TODO: resolve load order, build symlink farm, deploy to game directory
    println!("Deployed profile: {name}");

    Ok(())
}
