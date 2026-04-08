pub mod bethesda;
pub mod cyberpunk;
pub mod detection;
pub mod generic;
pub mod launcher;
pub mod tools;
pub mod traits;

pub use detection::{find_detected_game, scan_installed_games, DetectedGame, LauncherSource};
pub use traits::{
    GamePlugin, ModClassifyConfig, ModSafety, SaveTracker, classify_mod_by_content,
    DiscoveredFile, DiscoveredMod, ModScanner, ModSource, ScanContext,
    walk_files_relative, slug,
};

/// All recognized game IDs, in the order they appear in the match table.
pub const SUPPORTED_GAME_IDS: &[&str] = &[
    "skyrim-se",
    "skyrim-ae",
    "fallout4",
    "fallout76",
    "starfield",
    "cyberpunk2077",
];

/// Map a Wabbajack manifest `game` field (e.g. `"Cyberpunk2077"`, `"SkyrimSpecialEdition"`)
/// to the internal game_id (e.g. `"cyberpunk2077"`, `"skyrim-se"`).
///
/// Returns `None` if the name is not recognized.
pub fn normalize_wabbajack_game(wj_game: &str) -> Option<&'static str> {
    match wj_game {
        "Cyberpunk2077" => Some("cyberpunk2077"),
        "SkyrimSpecialEdition" => Some("skyrim-se"),
        "Fallout4" => Some("fallout4"),
        "Fallout76" => Some("fallout76"),
        "Starfield" => Some("starfield"),
        _ => None,
    }
}

/// Resolve a game_id string to the corresponding `GamePlugin` implementation.
pub fn resolve_game_plugin(game_id: &str) -> Option<&'static dyn GamePlugin> {
    match game_id {
        "skyrim-se" => Some(&bethesda::SKYRIM_SE),
        "skyrim-ae" => Some(&bethesda::SKYRIM_AE),
        "fallout4" => Some(&bethesda::FALLOUT4),
        "fallout76" => Some(&bethesda::FALLOUT76),
        "starfield" => Some(&bethesda::STARFIELD),
        "cyberpunk2077" => Some(&cyberpunk::CYBERPUNK2077),
        _ => None,
    }
}

/// Resolve a game_id to its `ModScanner` implementation, if one exists.
pub fn resolve_mod_scanner(game_id: &str) -> Option<&'static dyn ModScanner> {
    match game_id {
        "cyberpunk2077" => Some(&cyberpunk::scanner::CYBERPUNK_SCANNER),
        "skyrim-se" | "skyrim-ae" => Some(&bethesda::scanner::SKYRIM_SCANNER),
        "fallout4" => Some(&bethesda::scanner::FALLOUT4_SCANNER),
        "starfield" => Some(&bethesda::scanner::STARFIELD_SCANNER),
        _ => None,
    }
}

/// Resolve a game_id to its `CollisionClassifier` implementation, if one exists.
pub fn resolve_collision_classifier(
    game_id: &str,
) -> Option<Box<dyn modde_core::collision::CollisionClassifier>> {
    match game_id {
        "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" | "starfield" => {
            Some(Box::new(bethesda::collision::BethesdaCollisionClassifier))
        }
        "cyberpunk2077" => Some(Box::new(cyberpunk::collision::CyberpunkCollisionClassifier)),
        _ => None,
    }
}

/// Resolve a game_id to its `SaveTracker` implementation, if one exists.
pub fn resolve_save_tracker(game_id: &str) -> Option<&'static dyn SaveTracker> {
    match game_id {
        "skyrim-se" | "skyrim-ae" => Some(&bethesda::saves::SKYRIM_SAVE_TRACKER),
        "fallout4" => Some(&bethesda::saves::FALLOUT4_SAVE_TRACKER),
        // FO76 saves are server-side; local cache files are captured with a warning
        "fallout76" => Some(&bethesda::saves::FALLOUT76_SAVE_TRACKER),
        // Starfield: reuse Skyrim tracker as placeholder until save format is confirmed
        "starfield" => Some(&bethesda::saves::SKYRIM_SAVE_TRACKER),
        "cyberpunk2077" => Some(&cyberpunk::saves::CYBERPUNK_SAVE_TRACKER),
        _ => None,
    }
}
