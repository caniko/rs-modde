pub mod archives;
pub mod fomod;
pub mod ini;
pub mod plugins_txt;
pub mod saves;

use std::path::{Path, PathBuf};

use modde_core::paths;

use crate::traits::{GamePlugin, ModClassifyConfig, ModSafety, classify_mod_by_content};

/// Data-driven Bethesda game plugin.
///
/// All Bethesda games share the same deploy strategy (symlink into `Data/`)
/// and differ only in metadata. This replaces four near-identical unit structs.
pub struct BethesdaGame {
    game_id: &'static str,
    display_name: &'static str,
    /// Steam App ID (for Proton save path detection).
    steam_app_id: &'static str,
    /// "My Games" subdirectory name where saves are stored.
    my_games_dir: &'static str,
}

impl BethesdaGame {
    pub const fn new(
        game_id: &'static str,
        display_name: &'static str,
        steam_app_id: &'static str,
        my_games_dir: &'static str,
    ) -> Self {
        Self { game_id, display_name, steam_app_id, my_games_dir }
    }
}

/// File extensions that indicate a Bethesda mod alters game logic.
const BETHESDA_SAVE_BREAKING_EXT: &[&str] = &[
    "esp", "esm", "esl", "pex", "dll", "psc",
];

/// Extensions that are purely cosmetic in Bethesda games.
const BETHESDA_COSMETIC_EXT: &[&str] = &[
    "nif", "bsa", "ba2", "dds", "png", "tga", "jpg",
    "hkx", "fuz", "wav", "xwm", "swf", "ini", "json",
];

const BETHESDA_CLASSIFY_CONFIG: ModClassifyConfig = ModClassifyConfig {
    save_breaking_ext: BETHESDA_SAVE_BREAKING_EXT,
    cosmetic_ext: BETHESDA_COSMETIC_EXT,
    save_breaking_dirs: &[],
};

pub const SKYRIM_SE: BethesdaGame = BethesdaGame::new(
    "skyrim-se",
    "The Elder Scrolls V: Skyrim Special Edition",
    "489830",
    "Skyrim Special Edition",
);

pub const SKYRIM_AE: BethesdaGame = BethesdaGame::new(
    "skyrim-ae",
    "The Elder Scrolls V: Skyrim Anniversary Edition",
    "489830",
    "Skyrim Special Edition",
);

pub const FALLOUT4: BethesdaGame = BethesdaGame::new(
    "fallout4",
    "Fallout 4",
    "377160",
    "Fallout4",
);

pub const FALLOUT76: BethesdaGame = BethesdaGame::new(
    "fallout76",
    "Fallout 76",
    "1151340",
    "Fallout 76",
);

impl GamePlugin for BethesdaGame {
    fn game_id(&self) -> &str {
        self.game_id
    }

    fn display_name(&self) -> &str {
        self.display_name
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("Data")
    }

    fn save_directory(&self) -> Option<PathBuf> {
        // Proton prefix: compatdata/<APP_ID>/pfx/drive_c/Users/steamuser/Documents/My Games/<DIR>/Saves
        let compat = paths::steam_common()
            .parent()?  // steamapps/
            .join("compatdata")
            .join(self.steam_app_id)
            .join("pfx/drive_c/Users/steamuser/Documents/My Games")
            .join(self.my_games_dir)
            .join("Saves");
        if compat.exists() {
            return Some(compat);
        }
        None
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        classify_mod_by_content(mod_dir, &BETHESDA_CLASSIFY_CONFIG)
    }
}
