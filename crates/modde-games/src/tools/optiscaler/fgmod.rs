#![allow(clippy::wildcard_imports)]
use super::*;

/// Build fgmod DLL restore commands for the launch wrapper.
///
/// Scans the staging mods directory for DLLs that fgmod will delete at launch,
/// and returns `(source, destination)` pairs for the wrapper to restore them.
#[must_use]
pub fn fgmod_restore_commands(game_dir: &Path, staging_dir: &Path) -> Vec<(String, String)> {
    let exe_dir = crate::resolve_game_plugin("cyberpunk2077")
        .map(|plugin| plugin.executable_dir(game_dir))
        .unwrap_or_else(|| game_dir.join("bin/x64"));
    fgmod_restore_commands_for_executable_dir(game_dir, staging_dir, &exe_dir)
}

/// Build fgmod DLL restore commands for a known executable directory.
#[must_use]
pub fn fgmod_restore_commands_for_executable_dir(
    _game_dir: &Path,
    staging_dir: &Path,
    exe_dir: &Path,
) -> Vec<(String, String)> {
    let mut restore = Vec::new();

    let mods_dir = staging_dir.join("mods");
    if !mods_dir.exists() {
        return restore;
    }

    for entry in std::fs::read_dir(&mods_dir).into_iter().flatten().flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }

        let mod_bin_x64 = entry.path().join("bin/x64");
        if !mod_bin_x64.exists() {
            continue;
        }

        for dll_entry in std::fs::read_dir(&mod_bin_x64)
            .into_iter()
            .flatten()
            .flatten()
        {
            let dll_name = dll_entry.file_name().to_string_lossy().to_lowercase();
            if FGMOD_DELETED_DLLS
                .iter()
                .any(|d| d.to_lowercase() == dll_name)
            {
                let src = dll_entry.path();
                let dest = exe_dir.join(&*dll_name);
                restore.push((
                    src.to_string_lossy().to_string(),
                    dest.to_string_lossy().to_string(),
                ));
            }
        }
    }

    restore
}
