use anyhow::{Context, Result};
use smallvec::SmallVec;

pub mod bethesda;
pub mod cyberpunk;
pub mod detection;
pub mod generic;
pub mod launcher;
pub mod tools;
pub mod traits;
pub mod ue4;

pub use detection::{DetectedGame, LauncherSource, find_detected_game, scan_installed_games};
pub use traits::{
    DiscoveredFile, DiscoveredMod, GamePlugin, ModClassifyConfig, ModSafety, ModScanner, ModSource,
    SaveTracker, ScanContext, classify_mod_by_content, slug, walk_files_relative,
};

/// Build an [`modde_core::installer::InstallProbe`] that delegates to a
/// game plugin's [`GamePlugin::analyze_mod_archive`] and
/// [`GamePlugin::recognizes_bare_layout`] hooks.
///
/// Only `&'static dyn GamePlugin` is accepted because all registered
/// plugins are `static` (see [`resolve_game_plugin`]), and the probe's
/// closures need `'static` captures to cross async task boundaries.
///
/// ```ignore
/// let plugin = resolve_game_plugin("skyrim-se").unwrap();
/// let probe = game_probe(plugin);
/// let plan = modde_core::installer::analyze(&extracted, &probe, hash)?;
/// ```
pub fn game_probe(plugin: &'static dyn GamePlugin) -> modde_core::installer::InstallProbe {
    modde_core::installer::InstallProbe::new(
        move |dir: &std::path::Path| plugin.analyze_mod_archive(dir),
        move |dir: &std::path::Path| plugin.recognizes_bare_layout(dir),
    )
}

/// All recognized game IDs, in the order they appear in the match table.
pub const SUPPORTED_GAME_IDS: &[&str] = &[
    "skyrim-se",
    "skyrim-ae",
    "fallout4",
    "fallout76",
    "starfield",
    "cyberpunk2077",
    "stellar-blade",
];

/// (game_id, display_name) for every supported game, derived from the plugin registry.
pub fn supported_games() -> SmallVec<[(&'static str, &'static str); 8]> {
    SUPPORTED_GAME_IDS
        .iter()
        .filter_map(|&id| resolve_game_plugin(id).map(|p| (p.game_id(), p.display_name())))
        .collect()
}

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
        "stellar-blade" => Some(&ue4::STELLAR_BLADE),
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
        "stellar-blade" => Some(&ue4::scanner::STELLAR_BLADE_SCANNER),
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
        // Starfield save tracking is intentionally not exposed until it has a
        // real tracker instead of silently borrowing Skyrim semantics.
        "starfield" => None,
        "cyberpunk2077" => Some(&cyberpunk::saves::CYBERPUNK_SAVE_TRACKER),
        _ => None,
    }
}

/// Read the native plugin order for a game from `plugins.txt`, when the game uses one.
pub fn read_native_plugin_order(game_id: &str) -> Result<Vec<modde_core::PluginEntry>> {
    let plugin = resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game '{game_id}'"))?;
    let app_id = plugin
        .steam_app_id_u32()
        .ok_or_else(|| anyhow::anyhow!("game '{game_id}' does not expose plugins.txt"))?;
    let folder = plugin
        .plugins_txt_folder()
        .ok_or_else(|| anyhow::anyhow!("game '{game_id}' does not expose plugins.txt"))?;

    let entries = bethesda::plugins_txt::read_plugins_txt(app_id, folder)
        .with_context(|| format!("failed to read plugins.txt for '{game_id}'"))?;

    Ok(entries
        .into_iter()
        .enumerate()
        .map(|(sort_index, entry)| modde_core::PluginEntry {
            plugin_name: entry.name,
            sort_index: sort_index as i64,
            enabled: entry.enabled,
        })
        .collect())
}

/// Persist plugin order back to the game's native `plugins.txt`, when supported.
pub fn write_native_plugin_order(game_id: &str, plugins: &[modde_core::PluginEntry]) -> Result<()> {
    let plugin = resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game '{game_id}'"))?;
    let app_id = plugin
        .steam_app_id_u32()
        .ok_or_else(|| anyhow::anyhow!("game '{game_id}' does not expose plugins.txt"))?;
    let folder = plugin
        .plugins_txt_folder()
        .ok_or_else(|| anyhow::anyhow!("game '{game_id}' does not expose plugins.txt"))?;

    let entries: Vec<bethesda::plugins_txt::PluginEntry> = plugins
        .iter()
        .map(|plugin| bethesda::plugins_txt::PluginEntry {
            name: plugin.plugin_name.clone(),
            enabled: plugin.enabled,
        })
        .collect();

    bethesda::plugins_txt::write_plugins_txt(app_id, folder, &entries)
        .with_context(|| format!("failed to write plugins.txt for '{game_id}'"))
}
