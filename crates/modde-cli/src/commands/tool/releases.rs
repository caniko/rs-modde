//! Release-backed tool selection and installation commands.

use std::path::PathBuf;

use anyhow::{Context, Result};

use modde_core::db::ModdeDb;
use modde_core::resolver::GameId;

pub async fn handle_releases(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown tool: '{tool_id}'\nAvailable: {}",
            modde_games::tools::all_tools()
                .iter()
                .map(|t| t.tool_id())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    if !tool.supports_releases() {
        anyhow::bail!("{} does not support release selection", tool.display_name());
    }

    let db = ModdeDb::open().await.context("failed to open database")?;
    let current = load_tool_config_or_default(&db, game_id, tool_id, tool).await?;
    let current_tag = current.get_str("release_tag").unwrap_or("latest");
    let current_asset = current.get_str("release_asset").unwrap_or("");
    let releases = tool.list_releases().await?;

    if releases.is_empty() {
        println!("No releases returned for {}", tool.display_name());
        return Ok(());
    }

    println!(
        "{} releases for {game_id} (selected: {} / {})",
        tool.display_name(),
        current_tag,
        if current_asset.is_empty() {
            "(no asset)"
        } else {
            current_asset
        }
    );
    for release in &releases {
        let assets = tool.installable_release_assets(release);
        if assets.is_empty() {
            println!("  {}: no installable assets", release.tag);
        } else {
            println!("  {}: {}", release.tag, assets.join(", "));
        }
    }
    Ok(())
}

/// Install a specific release asset for a release-backed tool.
pub async fn handle_install_release(
    tool_id: &str,
    game_id: &str,
    tag: &str,
    asset: &str,
) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;
    if !tool.supports_releases() {
        anyhow::bail!("{} does not support release selection", tool.display_name());
    }

    let db = ModdeDb::open().await.context("failed to open database")?;
    let config = load_tool_config_or_default(&db, game_id, tool_id, tool).await?;
    let config = tool.install_release(game_id, config, tag, asset).await?;
    let settings_json = serde_json::to_string(&config.settings)?;
    db.save_tool_config(
        &GameId::from(game_id),
        tool_id,
        config.enabled,
        &settings_json,
    )
    .await?;

    println!(
        "Installed {} {} ({}) for {}",
        tool.display_name(),
        tag,
        asset,
        game_id
    );
    Ok(())
}

/// Install a specific release asset for a release-backed tool from a local path.
pub async fn handle_install_release_from_path(
    tool_id: &str,
    game_id: &str,
    tag: &str,
    asset: &str,
    path: PathBuf,
) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;
    if !tool.supports_releases() {
        anyhow::bail!("{} does not support release selection", tool.display_name());
    }

    let db = ModdeDb::open().await.context("failed to open database")?;
    let config = load_tool_config_or_default(&db, game_id, tool_id, tool).await?;
    let config = tool
        .install_release_from_path(game_id, config, tag, asset, path)
        .await?;
    let settings_json = serde_json::to_string(&config.settings)?;
    db.save_tool_config(
        &GameId::from(game_id),
        tool_id,
        config.enabled,
        &settings_json,
    )
    .await?;

    println!(
        "Installed {} {} ({}) for {}",
        tool.display_name(),
        tag,
        asset,
        game_id
    );
    Ok(())
}

async fn load_tool_config_or_default(
    db: &ModdeDb,
    game_id: &str,
    tool_id: &str,
    tool: &dyn modde_games::tools::GameTool,
) -> Result<modde_games::tools::ToolConfig> {
    let context = modde_games::resolve_game_plugin(game_id).map(|plugin| {
        modde_games::tools::ToolGameContext::from_parts(game_id, plugin.display_name(), None, None)
    });
    Ok(
        match db.load_tool_config(&GameId::from(game_id), tool_id).await? {
            Some(row) => modde_games::tools::ToolConfig {
                tool_id: row.tool_id,
                enabled: row.enabled,
                settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
            },
            None => tool.default_config_for(context.as_ref()),
        },
    )
}
