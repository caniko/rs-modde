#![allow(clippy::wildcard_imports)]
use super::*;

pub fn backup_optiscaler_install(
    game_id: Option<&str>,
    state: &OptiScalerInstallState,
) -> Result<Option<PathBuf>> {
    if state.recognized_files.is_empty() {
        return Ok(None);
    }
    let backup_dir = optiscaler_backup_root(game_id).join(timestamp_slug());
    for file in &state.recognized_files {
        let src = state.executable_dir.join(&file.rel_path);
        let dst = backup_dir.join(&file.rel_path);
        if src.is_file() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::copy(&src, &dst)
                .with_context(|| format!("failed to copy {}", dst.display()))?;
        }
    }
    let manifest = serde_json::json!({
        "status": state.status.to_string(),
        "version": state.version.to_string(),
        "executable_dir": state.executable_dir.display().to_string(),
        "files": state.recognized_files.iter().map(|file| {
            serde_json::json!({
                "path": file.rel_path.to_string_lossy().replace('\\', "/"),
                "hash": file.hash,
                "managed": file.managed,
            })
        }).collect::<Vec<_>>(),
    });
    std::fs::create_dir_all(&backup_dir)
        .with_context(|| format!("failed to create {}", backup_dir.display()))?;
    let manifest_path = backup_dir.join("modde-optiscaler-backup.json");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;
    Ok(Some(backup_dir))
}

pub fn latest_optiscaler_backup(game_id: Option<&str>) -> Option<PathBuf> {
    let root = optiscaler_backup_root(game_id);
    let mut entries = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            entry.file_type().ok()?.is_dir().then_some(path)
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.pop()
}

pub fn restore_latest_optiscaler_backup(game_id: &str, game_dir: &Path) -> Result<PathBuf> {
    let backup = latest_optiscaler_backup(Some(game_id))
        .ok_or_else(|| anyhow::anyhow!("no OptiScaler backup found for {game_id}"))?;
    let executable_dir = crate::resolve_game_plugin(game_id)
        .map(|plugin| plugin.executable_dir(game_dir))
        .unwrap_or_else(|| game_dir.to_path_buf());
    restore_dir_contents(&backup, &executable_dir)?;
    Ok(backup)
}

pub(super) fn optiscaler_backup_root(game_id: Option<&str>) -> PathBuf {
    modde_core::paths::modde_data_dir()
        .join("tool-backups")
        .join("optiscaler")
        .join(game_id.unwrap_or("_unknown"))
}

pub(super) fn timestamp_slug() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format!("{secs}-{}", std::process::id())
}
