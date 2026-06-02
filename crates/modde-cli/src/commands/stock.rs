use anyhow::{Context, Result};
use tracing::info;

use modde_core::ModdeDb;
use modde_core::fs::count_files;
use modde_core::resolver::GameId;
use modde_core::stock::StockGameManager;

use crate::StockAction;
use modde_games::resolve_game_plugin;

pub async fn handle(action: StockAction) -> Result<()> {
    let db = ModdeDb::open().await.context("failed to open database")?;
    let mgr = StockGameManager::with_db(StockGameManager::default_dir(), db);

    match action {
        StockAction::Snapshot { game_id } => {
            info!(%game_id, "creating stock snapshot");

            // Detect game install using GamePlugin
            let game = resolve_game_plugin(&game_id).context("unsupported game")?;
            let install_path = game.detect_install().ok_or_else(|| {
                anyhow::anyhow!(
                    "could not detect install path for game '{}' ({})",
                    game_id,
                    game.display_name()
                )
            })?;

            info!(
                game = game.display_name(),
                path = %install_path.display(),
                "game install detected"
            );

            // Create snapshot
            let snapshot = mgr
                .snapshot(&GameId::from(game_id.as_str()), &install_path)
                .await
                .context("failed to create stock snapshot")?;

            // Count files in snapshot
            let file_count = count_files(&snapshot.path)?;

            println!("Stock snapshot created for '{}'", game.display_name());
            println!("  Snapshot path: {}", snapshot.path.display());
            println!("  Files:         {file_count}");
        }
        StockAction::Verify { game_id } => {
            let ok = mgr.verify(&GameId::from(game_id.as_str())).await?;
            if ok {
                println!("Stock snapshot for {game_id}: OK");
            } else {
                println!("Stock snapshot for {game_id}: MISMATCH (re-snapshot recommended)");
            }
        }
    }
    Ok(())
}
