//! Nexus Collection install workflow.

use super::*;

pub(super) async fn handle_nexus_collection(
    slug: String,
    version: Option<String>,
    profile_name: Option<String>,
) -> Result<()> {
    info!(%slug, ?version, ?profile_name, "installing Nexus Collection");

    let api_key = load_api_key().context("failed to load Nexus API key")?;
    let client = build_http_client()?;

    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile_name = profile_name.unwrap_or_else(|| slug.clone());

    // Two-step fetch: discover game_domain from slug, then fetch the full manifest.
    let manifest = fetch_collection(&client, &api_key, &slug, version.as_deref())
        .await
        .context("failed to fetch collection manifest")?;
    let transaction = modde_sources::resolution::preflight_collection_transaction(&manifest)
        .context("collection dependency transaction is not solvable")?;

    let game_domain = manifest.game.domain_name.clone();
    let collection_version = manifest.version.version.clone();

    println!(
        "Collection: {} by {} ({} mods, {} solved artifacts)",
        manifest.name,
        manifest.author.name,
        manifest.mods.len(),
        transaction.artifacts.len()
    );

    let store = paths::store_dir();
    let mut enabled_mods = Vec::new();

    // Sort mods by install_order
    let mut mods = manifest.mods.clone();
    mods.sort_by_key(|m| m.install_order);

    for collection_mod in &mods {
        let mod_name = &collection_mod.name;
        let mod_id = collection_mod.mod_id;
        let file_id = collection_mod.file_id;

        println!("  Installing: {mod_name} (mod {mod_id}, file {file_id})");

        let mod_store_dir = store.join(format!("{game_domain}_{mod_id}_{file_id}"));

        if mod_store_dir.exists() {
            println!("    (already downloaded, skipping)");
        } else {
            // Generate download link and download
            let download_url =
                generate_download_link(&client, &api_key, &game_domain, mod_id, file_id)
                    .await
                    .with_context(|| format!("failed to get download link for {mod_name}"))?;

            let archive_path = store.join(format!("{mod_id}_{file_id}.zip"));
            download_file(&client, &download_url, &archive_path)
                .await
                .with_context(|| format!("failed to download {mod_name}"))?;

            // Extract archive
            std::fs::create_dir_all(&mod_store_dir)?;
            extract_archive(&archive_path, &mod_store_dir)
                .with_context(|| format!("failed to extract {mod_name}"))?;

            // Clean up archive after extraction
            let _ = std::fs::remove_file(&archive_path);
        }

        // Detect FOMOD - for automated installs we apply default selections
        if find_fomod_config(&mod_store_dir).is_some() {
            info!(%mod_name, "FOMOD installer detected, applying defaults");
            // FOMOD will be applied during deploy phase with full context
        }

        let mod_id_str = format!("{game_domain}_{mod_id}_{file_id}");
        enabled_mods.push(EnabledMod {
            mod_id: mod_id_str,
            display_name: Some(collection_mod.name.clone()),
            enabled: !collection_mod.optional,
            version: Some(collection_mod.version.clone()),
            fomod_config: None,
            ..Default::default()
        });
    }

    // Create and save profile. Collections define a canonical install
    // order, so we stamp a matching `LockReason::NexusCollection` lock —
    // preventing accidental reorder until the user explicitly unlocks.
    let profile = Profile {
        id: None,
        name: profile_name.clone(),
        game_id: modde_core::GameId::from(game_domain.clone()),
        source: ProfileSource::NexusCollection {
            slug: slug.clone(),
            version: collection_version.clone(),
        },
        mods: enabled_mods,
        overrides: paths::modde_data_dir()
            .join("profiles")
            .join(&profile_name)
            .join("overrides"),
        load_order_rules: smallvec::SmallVec::new(),
        load_order_lock: Some(LoadOrderLock::now(LockReason::NexusCollection {
            slug: slug.clone(),
            version: collection_version,
        })),
    };

    save_profile_and_settings(&pm, &profile, None).await?;

    println!(
        "Collection '{slug}' installed to profile '{profile_name}' ({} mods)",
        profile.mods.len()
    );

    Ok(())
}
