use anyhow::Result;

use modde_core::stock::StockGameManager;

use crate::StockAction;

pub async fn handle(action: StockAction) -> Result<()> {
    let mgr = StockGameManager::new(StockGameManager::default_dir());

    match action {
        StockAction::Snapshot { game_id } => {
            // TODO: detect game install, create snapshot
            println!("Creating stock snapshot for: {game_id}");
            println!("(Game detection not yet implemented)");
        }
        StockAction::Verify { game_id } => {
            let ok = mgr.verify(&game_id).await?;
            if ok {
                println!("Stock snapshot for {game_id}: OK");
            } else {
                println!("Stock snapshot for {game_id}: MISMATCH (re-snapshot recommended)");
            }
        }
    }
    Ok(())
}
