use anyhow::{Context, Result};
use reqwest::Client;
use tracing::info;

use modde_core::profile::ProfileManager;
use modde_sources::nexus::api::NexusApi;
use modde_sources::nexus::auth::load_api_key;
use modde_sources::nexus::updates::{TrackedMod, check_updates};

use super::load_profile_or_default;

pub async fn handle_check(
    profile_name: Option<String>,
    game_id: Option<String>,
    period: String,
) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref())?;

    info!(profile = %profile.name, game = %profile.game_id, "checking for mod updates");

    // Collect tracked mods with Nexus metadata
    let tracked: Vec<TrackedMod> = profile
        .mods
        .iter()
        .filter(|m| m.enabled && m.nexus_mod_id.is_some())
        .filter_map(|m| {
            Some(TrackedMod {
                mod_id: m.mod_id.clone(),
                nexus_mod_id: m.nexus_mod_id? as u64,
                nexus_game_domain: m.nexus_game_domain.clone()?,
                installed_version: m.version.clone(),
                installed_timestamp: m.installed_timestamp?,
            })
        })
        .collect();

    if tracked.is_empty() {
        println!(
            "No mods with Nexus metadata found in profile '{}'.",
            profile.name
        );
        println!("Hint: Mods installed via 'modde install mod' or Nexus Collections");
        println!("      automatically track their Nexus source for update checking.");
        return Ok(());
    }

    println!(
        "Checking updates for {} tracked mods (period: {period})...",
        tracked.len()
    );

    let api_key = load_api_key()?;
    let client = Client::new();
    let api = NexusApi::new(client, api_key);

    let updates = check_updates(&api, &tracked, &period).await?;

    if updates.is_empty() {
        println!("All {} tracked mods are up to date.", tracked.len());
    } else {
        println!("\n{} mod(s) have updates available:\n", updates.len());
        for u in &updates {
            let installed = u.installed_version.as_deref().unwrap_or("unknown");
            println!(
                "  {} (installed: {}, updated: {})",
                u.mod_id, installed, u.latest_file_update
            );
        }
    }

    Ok(())
}
