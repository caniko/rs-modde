use anyhow::Result;
use tracing::info;

use modde_core::profile::{Profile, ProfileManager, ProfileSource};

use crate::ProfileAction;

pub async fn handle(action: ProfileAction) -> Result<()> {
    let pm = ProfileManager::new(ProfileManager::default_dir());

    match action {
        ProfileAction::List => {
            let profiles = pm.list()?;
            if profiles.is_empty() {
                println!("No profiles found.");
            } else {
                for name in profiles {
                    println!("  {name}");
                }
            }
        }
        ProfileAction::Switch { name } => {
            let _profile = pm.load(&name)?;
            info!(profile = %name, "switched to profile");
            println!("Switched to profile: {name}");
        }
        ProfileAction::Create { name, game } => {
            let profile = Profile {
                name: name.clone(),
                game_id: game.clone(),
                source: ProfileSource::Manual,
                mods: vec![],
                overrides: ProfileManager::default_dir()
                    .join(&name)
                    .join("overrides"),
                load_order_rules: vec![],
            };
            pm.create(&profile)?;
            println!("Created profile: {name} (game: {game})");
        }
        ProfileAction::Delete { name } => {
            pm.delete(&name)?;
            println!("Deleted profile: {name}");
        }
    }

    Ok(())
}
