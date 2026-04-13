pub mod collision;
pub mod manifest;
pub mod redmod;
pub mod saves;
pub mod scanner;

use std::path::{Path, PathBuf};

use anyhow::Result;

use smallvec::SmallVec;

use crate::traits::{GamePlugin, ModClassifyConfig, ModSafety, classify_mod_by_content};

pub struct Cyberpunk2077;

pub static CYBERPUNK2077: Cyberpunk2077 = Cyberpunk2077;

/// File extensions that indicate a mod alters game logic.
const CYBERPUNK_SAVE_BREAKING_EXT: &[&str] = &[
    "reds", "lua", "tweak", "xl", "yaml", "yls",
];

/// Directories within a mod that signal save-breaking content.
const CYBERPUNK_SAVE_BREAKING_DIRS: &[&str] = &[
    "r6/scripts", "r6/tweaks", "bin/x64/plugins/cyber_engine_tweaks/mods",
];

/// Extensions that are purely cosmetic.
const CYBERPUNK_COSMETIC_EXT: &[&str] = &[
    "archive", "xl", "png", "jpg", "dds", "tga", "ini",
];

const CYBERPUNK_CLASSIFY_CONFIG: ModClassifyConfig = ModClassifyConfig {
    save_breaking_ext: CYBERPUNK_SAVE_BREAKING_EXT,
    cosmetic_ext: CYBERPUNK_COSMETIC_EXT,
    save_breaking_dirs: CYBERPUNK_SAVE_BREAKING_DIRS,
};

/// Windows system DLLs commonly hijacked by mod frameworks as proxy/hook DLLs.
/// When present in the game's executable directory, these need Wine `n,b` overrides
/// so Wine loads the native (mod) version instead of its built-in stub.
const KNOWN_PROXY_DLLS: &[&str] = &[
    "version",    // CET (Cyber Engine Tweaks), ASI loaders
    "winmm",      // ASI loader, some mod frameworks
    "dinput8",    // Various mod frameworks
    "d3d11",      // ReShade, ENB
    "dxgi",       // OptiScaler, ReShade (often handled by fgmod)
    "winhttp",    // Some mod loaders
    "xinput1_3",  // Controller hook mods
];

impl GamePlugin for Cyberpunk2077 {
    fn game_id(&self) -> &str {
        "cyberpunk2077"
    }

    fn display_name(&self) -> &str {
        "Cyberpunk 2077"
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("mods")
    }

    fn deploy(&self, staging: &Path, target: &Path) -> Result<()> {
        if !target.exists() {
            std::fs::create_dir_all(target)?;
        }
        // Symlink each mod directory from staging into game mods dir
        for entry in std::fs::read_dir(staging)? {
            let entry = entry?;
            let dst = target.join(entry.file_name());
            if dst.exists() || dst.symlink_metadata().is_ok() {
                if dst.is_dir() {
                    std::fs::remove_dir_all(&dst)?;
                } else {
                    std::fs::remove_file(&dst)?;
                }
            }
            modde_core::fs::symlink(&entry.path(), &dst)?;
        }
        Ok(())
    }

    fn post_deploy(&self, install: &Path) -> Result<()> {
        // Run REDmod deploy if available
        redmod::deploy_if_available(install)
    }

    fn save_directory(&self) -> Option<PathBuf> {
        let save_suffix = "pfx/drive_c/users/steamuser/Saved Games/CD Projekt Red/Cyberpunk 2077";

        // Check Heroic prefixes (GOG / sideload)
        let heroic_prefixes = modde_core::paths::home_dir()
            .join("Games/Heroic/Prefixes/default/Cyberpunk 2077");
        let heroic_path = heroic_prefixes.join(save_suffix);
        if heroic_path.exists() {
            return Some(heroic_path);
        }

        // Steam Proton prefix
        let compat = modde_core::paths::steam_common()
            .parent()?  // steamapps/
            .join("compatdata/1091500")
            .join(save_suffix);
        if compat.exists() {
            return Some(compat);
        }
        None
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        classify_mod_by_content(mod_dir, &CYBERPUNK_CLASSIFY_CONFIG)
    }

    fn wine_dll_overrides(&self, game_dir: &Path) -> SmallVec<[String; 4]> {
        let exe_dir = self.executable_dir(game_dir);
        let mut overrides = SmallVec::new();

        for &dll_name in KNOWN_PROXY_DLLS {
            let dll_path = exe_dir.join(format!("{dll_name}.dll"));
            if dll_path.exists() {
                overrides.push(dll_name.to_string());
            }
        }

        overrides
    }

    fn wine_dll_overrides_from_staging(&self, staging: &Path) -> SmallVec<[String; 4]> {
        let mut overrides = SmallVec::new();
        let mods_dir = staging.join("mods");
        if !mods_dir.is_dir() {
            return overrides;
        }

        for entry in std::fs::read_dir(&mods_dir).into_iter().flatten().flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }

            let mod_bin_x64 = entry.path().join("bin/x64");
            if !mod_bin_x64.is_dir() {
                continue;
            }

            for dll_entry in std::fs::read_dir(&mod_bin_x64).into_iter().flatten().flatten() {
                let name = dll_entry.file_name().to_string_lossy().to_lowercase();
                if let Some(stem) = name.strip_suffix(".dll") {
                    if KNOWN_PROXY_DLLS.contains(&stem) && !overrides.contains(&stem.to_string()) {
                        overrides.push(stem.to_string());
                    }
                }
            }
        }

        overrides
    }

    fn executable_dir(&self, install: &Path) -> PathBuf {
        install.join("bin").join("x64")
    }

    fn archive_extensions(&self) -> &[&str] {
        &["archive"]
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        Some(1091500)
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        Some("cyberpunk2077")
    }

    fn nexus_game_id_u32(&self) -> Option<u32> {
        // Nexus Mods v2 GraphQL game ID for Cyberpunk 2077.
        // Source: https://api.nexusmods.com/v1/games.json
        Some(3333)
    }

    fn analyze_mod_archive(
        &self,
        extracted_dir: &Path,
    ) -> Option<modde_core::installer::InstallMethod> {
        // REDmod signature: top-level `info.json` + `archives/` subdir.
        // The `archives/` dir may also be spelled `archive/` on some
        // mods; check both.
        let info_json = extracted_dir.join("info.json");
        if !info_json.is_file() {
            return None;
        }
        let archives = extracted_dir.join("archives");
        let archive = extracted_dir.join("archive");
        if archives.is_dir() || archive.is_dir() {
            return Some(modde_core::installer::InstallMethod::REDmod {
                manifest: PathBuf::from("info.json"),
            });
        }
        None
    }

    fn recognizes_bare_layout(&self, extracted_dir: &Path) -> bool {
        // Cyberpunk mods drop loose into one of these top-level dirs.
        // If any of them exist at the extraction root, treat the archive
        // as a bare extract — the deploy step will symlink into
        // `<install>/mods/<name>/` via the REDmod loader.
        for name in ["r6", "archive", "archives", "bin", "engine", "mods", "red4ext"] {
            if extracted_dir.join(name).is_dir() {
                return true;
            }
        }
        false
    }
}

