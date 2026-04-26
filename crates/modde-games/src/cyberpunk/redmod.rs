use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use tracing::{info, warn};

/// Locate the `REDmod` binary.
fn find_redmod(game_dir: &Path) -> Option<PathBuf> {
    // Check within game directory (platform-aware binary name)
    let bin_name = if cfg!(windows) {
        "redmod.exe"
    } else {
        "redmod"
    };
    let in_game = game_dir.join("tools/redmod/bin").join(bin_name);
    if in_game.exists() {
        return Some(in_game);
    }

    // Check PATH using the `which` crate (cross-platform)
    which::which("redmod").ok()
}

/// Run `redmod deploy` for the given mod directories.
pub fn deploy(mod_dirs: &[PathBuf], game_dir: &Path) -> Result<()> {
    let redmod_bin =
        find_redmod(game_dir).ok_or_else(|| anyhow::anyhow!("REDmod binary not found"))?;

    let mut cmd = Command::new(&redmod_bin);
    cmd.arg("deploy");

    for dir in mod_dirs {
        cmd.arg("-mod").arg(dir);
    }

    cmd.current_dir(game_dir);

    info!(bin = %redmod_bin.display(), "running REDmod deploy");

    let output = cmd
        .output()
        .with_context(|| "failed to run REDmod deploy")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("REDmod deploy failed: {stderr}");
    }

    Ok(())
}

/// Run `REDmod` deploy if the binary is available; warn and skip if not.
pub fn deploy_if_available(game_dir: &Path) -> Result<()> {
    if find_redmod(game_dir).is_none() {
        warn!("REDmod not found; skipping post-deploy step");
        return Ok(());
    }

    let mods_dir = game_dir.join("mods");
    if !mods_dir.exists() {
        return Ok(());
    }

    let mod_dirs: Vec<PathBuf> = std::fs::read_dir(&mods_dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();

    if mod_dirs.is_empty() {
        return Ok(());
    }

    deploy(&mod_dirs, game_dir)
}
