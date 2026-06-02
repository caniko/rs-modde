use anyhow::{Context, Result};
use tracing::info;

use modde_core::profile::{ActivateResult, ProfileManager};
use modde_core::resolver::GameId;
use modde_core::save::SaveManager;

use super::{compute_fingerprint, resolve_save_dir, supports_save_profiles};

pub async fn handle(
    profile_name: Option<String>,
    game_id: String,
    no_deploy: bool,
    no_switch: bool,
    no_capture: bool,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;

    // 1. Resolve which profile to use
    let target_profile = match profile_name {
        Some(name) => name,
        None => pm
            .active(&GameId::from(game_id.as_str()))
            .await?
            .map(|info| info.profile.name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no active profile for game '{game_id}'. \
                     Specify one: modde play <profile> --game {game_id}"
                )
            })?,
    };

    // 2. Switch profile (swaps saves automatically)
    if !no_switch {
        let already_active = pm
            .active(&GameId::from(game_id.as_str()))
            .await?
            .is_some_and(|info| info.profile.name == target_profile);

        if already_active {
            info!(profile = %target_profile, "profile already active, skipping switch");
        } else {
            let save_dir = resolve_save_dir(&game_id);
            let fp = compute_fingerprint(&pm, &target_profile, &game_id).await;
            match pm
                .activate_with_fingerprint(
                    &target_profile,
                    &GameId::from(game_id.as_str()),
                    save_dir.as_deref(),
                    fp.as_ref(),
                )
                .await?
            {
                ActivateResult::Activated => {
                    if save_dir.is_some() {
                        println!("Switched to profile: {target_profile} (saves swapped)");
                    } else if matches!(supports_save_profiles(&game_id), Ok(false)) {
                        println!(
                            "Switched to profile: {target_profile} (save profiles unsupported)"
                        );
                    } else {
                        println!("Switched to profile: {target_profile}");
                    }
                }
                ActivateResult::AdoptionRequired { save_count } => {
                    anyhow::bail!(
                        "Found {save_count} unadopted save(s) in the game directory.\n\
                         Run `modde save adopt --game {game_id} --profile {target_profile}` first."
                    );
                }
            }
        }
    }

    // 3. Deploy mods
    if !no_deploy {
        super::deploy::handle(Some(target_profile.clone()), Some(game_id.clone())).await?;
    }

    // 4. Detect launcher and launch
    let detected =
        modde_games::find_detected_game(&GameId::from(game_id.as_str())).ok_or_else(|| {
            anyhow::anyhow!("could not detect launcher for '{game_id}'. Is the game installed?")
        })?;

    println!("Launching via {}...", detected.source);
    let exit_status = detected.source.launch()?;

    // 5. Auto-capture saves after exit
    if !no_capture {
        match exit_status {
            Some(status) => {
                println!("Game exited (status: {status}).");
                let save_dir = resolve_save_dir(&game_id);
                if let Some(ref save_dir) = save_dir {
                    let sm = SaveManager::new(pm.db());
                    let fp = compute_fingerprint(&pm, &target_profile, &game_id).await;
                    match sm.capture_with_fingerprint(
                        &GameId::from(game_id.as_str()),
                        &target_profile,
                        save_dir,
                        fp.as_ref(),
                    ) {
                        Ok(_) => println!("Saves auto-captured for profile '{target_profile}'."),
                        Err(e) => eprintln!("Warning: save auto-capture failed: {e}"),
                    }
                } else if matches!(supports_save_profiles(&game_id), Ok(false)) {
                    info!(game = %game_id, "save profiles unsupported, skipping capture");
                }
            }
            None => {
                if matches!(supports_save_profiles(&game_id), Ok(false)) {
                    println!("Game launched via Steam (fire-and-forget).");
                } else {
                    println!(
                        "Game launched via Steam (fire-and-forget).\n\
                         Saves will be captured by the launch wrapper on exit, or run:\n  \
                         modde save auto-capture --game {game_id}"
                    );
                }
            }
        }
    }

    Ok(())
}
