use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::error::{CoreError, Result};

/// Manages vanilla game snapshots for stock game preservation.
pub struct StockGameManager {
    store_dir: PathBuf,
}

/// A snapshot of a vanilla game installation.
#[derive(Debug)]
pub struct StockSnapshot {
    pub game_id: String,
    pub path: PathBuf,
    pub hash: String,
}

impl StockGameManager {
    pub fn new(store_dir: PathBuf) -> Self {
        Self { store_dir }
    }

    /// Default store directory: `~/.local/share/modde/stock/`.
    pub fn default_dir() -> PathBuf {
        let data_dir = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                PathBuf::from(home).join(".local/share")
            });
        data_dir.join("modde").join("stock")
    }

    /// Detect Steam install path for a given game.
    pub fn detect_steam_install(&self, game_dir_name: &str) -> Option<PathBuf> {
        let steam_common = steam_common_path();
        let game_path = steam_common.join(game_dir_name);
        if game_path.exists() {
            Some(game_path)
        } else {
            None
        }
    }

    /// Create a hardlink snapshot of a game installation.
    ///
    /// Falls back to file copy if source and destination are on different filesystems.
    pub async fn snapshot(&self, game_id: &str, source_dir: &Path) -> Result<StockSnapshot> {
        if !source_dir.exists() {
            return Err(CoreError::GameNotDetected(game_id.to_string()));
        }

        let snapshot_dir = self.store_dir.join(game_id);
        tokio::fs::create_dir_all(&snapshot_dir).await?;

        // Walk source and hardlink/copy files
        snapshot_recursive(source_dir, &snapshot_dir).await?;

        info!(game_id, path = %snapshot_dir.display(), "stock snapshot created");

        Ok(StockSnapshot {
            game_id: game_id.to_string(),
            path: snapshot_dir,
            hash: String::new(), // TODO: compute tree hash
        })
    }

    /// Verify an existing snapshot still matches the game installation.
    pub async fn verify(&self, game_id: &str) -> Result<bool> {
        let snapshot_dir = self.store_dir.join(game_id);
        if !snapshot_dir.exists() {
            return Err(CoreError::Other(format!(
                "no snapshot found for game '{game_id}'"
            )));
        }
        // TODO: compare tree hashes
        Ok(true)
    }
}

async fn snapshot_recursive(src: &Path, dst: &Path) -> Result<()> {
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            tokio::fs::create_dir_all(&dst_path).await?;
            Box::pin(snapshot_recursive(&src_path, &dst_path)).await?;
        } else if file_type.is_file() {
            match tokio::fs::hard_link(&src_path, &dst_path).await {
                Ok(()) => {}
                Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                    warn!(
                        src = %src_path.display(),
                        "cross-device hardlink; falling back to copy"
                    );
                    tokio::fs::copy(&src_path, &dst_path).await?;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(())
}

fn steam_common_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local/share/Steam/steamapps/common")
}
