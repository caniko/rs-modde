use std::sync::{OnceLock, RwLock};

use crate::generic::loader::load_user_games;
use crate::optiscaler::OptiScalerProfile;
use crate::policies::{CollisionPolicy, PolicyCollisionClassifier};
use crate::traits::{GamePlugin, ModScanner, SaveTracker};

pub type CollisionClassifierFactory = fn() -> Box<dyn modde_core::collision::CollisionClassifier>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineFamily {
    Bethesda,
    Bannerlord,
    CyberpunkRedEngine,
    Gamebryo,
    Generic,
    Larian,
    Smapi,
    Unreal4,
    Witcher,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LauncherIds {
    pub steam_app_id: Option<&'static str>,
    pub steam_dir: Option<&'static str>,
    pub heroic_gog_app_id: Option<&'static str>,
    pub heroic_epic_app_id: Option<&'static str>,
}

#[derive(Clone, Copy)]
pub struct GameRegistration {
    pub game_id: &'static str,
    pub display_name: &'static str,
    pub engine: EngineFamily,
    pub launcher: LauncherIds,
    pub wabbajack_names: &'static [&'static str],
    pub nexus_domain: Option<&'static str>,
    pub nexus_game_id: Option<u32>,
    pub supports_save_profiles: bool,
    pub plugin: &'static dyn GamePlugin,
    pub scanner: Option<&'static dyn ModScanner>,
    pub save_tracker: Option<&'static dyn SaveTracker>,
    pub collision_classifier: Option<CollisionClassifierFactory>,
    pub optiscaler_profiles: &'static [OptiScalerProfile],
}

impl GameRegistration {
    pub fn normalized_wabbajack_names(self) -> impl Iterator<Item = String> {
        self.wabbajack_names.iter().map(|name| {
            name.chars()
                .filter(char::is_ascii_alphanumeric)
                .flat_map(char::to_lowercase)
                .collect()
        })
    }
}

fn bethesda_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier> {
    Box::new(crate::bethesda::collision::BethesdaCollisionClassifier)
}

fn cyberpunk_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier> {
    Box::new(crate::cyberpunk::collision::CyberpunkCollisionClassifier)
}

fn ue4_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier> {
    Box::new(crate::policies::PolicyCollisionClassifier {
        policy: crate::ue4::UE4_COLLISION_POLICY,
    })
}

fn policy_collision_classifier(
    archive_extensions: &'static [&'static str],
) -> Box<dyn modde_core::collision::CollisionClassifier> {
    Box::new(PolicyCollisionClassifier {
        policy: CollisionPolicy {
            archive_extensions,
            severities: DEFAULT_SEVERITIES,
        },
    })
}

const DEFAULT_ARCHIVE_EXTENSIONS: &[&str] = &[];
const BSA_ARCHIVE_EXTENSIONS: &[&str] = &["bsa"];
const PAK_ARCHIVE_EXTENSIONS: &[&str] = &["pak", "ucas", "utoc"];
const WITCHER_ARCHIVE_EXTENSIONS: &[&str] = &["bundle", "cache"];

const DEFAULT_SEVERITIES: &[(&str, modde_core::collision::CollisionSeverity)] = &[
    ("dds", modde_core::collision::CollisionSeverity::Cosmetic),
    ("png", modde_core::collision::CollisionSeverity::Cosmetic),
    ("jpg", modde_core::collision::CollisionSeverity::Cosmetic),
    ("tga", modde_core::collision::CollisionSeverity::Cosmetic),
    ("nif", modde_core::collision::CollisionSeverity::Cosmetic),
    ("ini", modde_core::collision::CollisionSeverity::Config),
    ("json", modde_core::collision::CollisionSeverity::Config),
    ("xml", modde_core::collision::CollisionSeverity::Config),
    ("esp", modde_core::collision::CollisionSeverity::Dangerous),
    ("esm", modde_core::collision::CollisionSeverity::Dangerous),
    ("dll", modde_core::collision::CollisionSeverity::Dangerous),
    ("lua", modde_core::collision::CollisionSeverity::Dangerous),
    ("ws", modde_core::collision::CollisionSeverity::Dangerous),
];

pub(crate) fn generic_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier>
{
    policy_collision_classifier(DEFAULT_ARCHIVE_EXTENSIONS)
}

fn gamebryo_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier> {
    policy_collision_classifier(BSA_ARCHIVE_EXTENSIONS)
}

fn pak_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier> {
    policy_collision_classifier(PAK_ARCHIVE_EXTENSIONS)
}

fn witcher_collision_classifier() -> Box<dyn modde_core::collision::CollisionClassifier> {
    policy_collision_classifier(WITCHER_ARCHIVE_EXTENSIONS)
}

pub const SUPPORTED_GAME_IDS: &[&str] = &[
    "skyrim-se",
    "skyrim-ae",
    "fallout4",
    "fallout76",
    "starfield",
    "cyberpunk2077",
    "stellar-blade",
    "baldurs-gate3",
    "stardew-valley",
    "fallout-new-vegas",
    "oblivion",
    "oblivion-remastered",
    "bannerlord",
    "witcher3",
];

pub static GAME_REGISTRY: &[GameRegistration] = &[
    GameRegistration {
        game_id: "skyrim-se",
        display_name: "The Elder Scrolls V: Skyrim Special Edition",
        engine: EngineFamily::Bethesda,
        launcher: LauncherIds {
            steam_app_id: Some("489830"),
            steam_dir: Some("Skyrim Special Edition"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &["SkyrimSpecialEdition", "SkyrimSE"],
        nexus_domain: Some("skyrimspecialedition"),
        nexus_game_id: Some(1704),
        supports_save_profiles: true,
        plugin: &crate::bethesda::SKYRIM_SE,
        scanner: Some(&crate::bethesda::scanner::SKYRIM_SCANNER),
        save_tracker: Some(&crate::bethesda::saves::SKYRIM_SAVE_TRACKER),
        collision_classifier: Some(bethesda_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "skyrim-ae",
        display_name: "The Elder Scrolls V: Skyrim Anniversary Edition",
        engine: EngineFamily::Bethesda,
        launcher: LauncherIds {
            // AE shares the SE app and install dir. Launcher detection reports
            // skyrim-se by default; users can still select skyrim-ae explicitly.
            steam_app_id: None,
            steam_dir: None,
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &["SkyrimAnniversaryEdition", "SkyrimAE"],
        nexus_domain: Some("skyrimspecialedition"),
        nexus_game_id: Some(1704),
        supports_save_profiles: true,
        plugin: &crate::bethesda::SKYRIM_AE,
        scanner: Some(&crate::bethesda::scanner::SKYRIM_SCANNER),
        save_tracker: Some(&crate::bethesda::saves::SKYRIM_SAVE_TRACKER),
        collision_classifier: Some(bethesda_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "fallout4",
        display_name: "Fallout 4",
        engine: EngineFamily::Bethesda,
        launcher: LauncherIds {
            steam_app_id: Some("377160"),
            steam_dir: Some("Fallout 4"),
            heroic_gog_app_id: Some("1998527297"),
            heroic_epic_app_id: None,
        },
        wabbajack_names: &["Fallout4"],
        nexus_domain: Some("fallout4"),
        nexus_game_id: Some(1151),
        supports_save_profiles: true,
        plugin: &crate::bethesda::FALLOUT4,
        scanner: Some(&crate::bethesda::scanner::FALLOUT4_SCANNER),
        save_tracker: Some(&crate::bethesda::saves::FALLOUT4_SAVE_TRACKER),
        collision_classifier: Some(bethesda_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "fallout76",
        display_name: "Fallout 76",
        engine: EngineFamily::Bethesda,
        launcher: LauncherIds {
            steam_app_id: Some("1151340"),
            steam_dir: Some("Fallout76"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &["Fallout76"],
        nexus_domain: Some("fallout76"),
        nexus_game_id: Some(2590),
        supports_save_profiles: true,
        plugin: &crate::bethesda::FALLOUT76,
        scanner: Some(&crate::bethesda::scanner::FALLOUT76_SCANNER),
        save_tracker: Some(&crate::bethesda::saves::FALLOUT76_SAVE_TRACKER),
        collision_classifier: Some(bethesda_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "starfield",
        display_name: "Starfield",
        engine: EngineFamily::Bethesda,
        launcher: LauncherIds {
            steam_app_id: Some("1716740"),
            steam_dir: Some("Starfield"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &["Starfield"],
        nexus_domain: Some("starfield"),
        nexus_game_id: Some(4187),
        supports_save_profiles: true,
        plugin: &crate::bethesda::STARFIELD,
        scanner: Some(&crate::bethesda::scanner::STARFIELD_SCANNER),
        save_tracker: Some(&crate::bethesda::saves::STARFIELD_SAVE_TRACKER),
        collision_classifier: Some(bethesda_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "cyberpunk2077",
        display_name: "Cyberpunk 2077",
        engine: EngineFamily::CyberpunkRedEngine,
        launcher: LauncherIds {
            steam_app_id: Some("1091500"),
            steam_dir: Some("Cyberpunk 2077"),
            heroic_gog_app_id: Some("1423049311"),
            heroic_epic_app_id: Some("Ginger"),
        },
        wabbajack_names: &["Cyberpunk2077"],
        nexus_domain: Some("cyberpunk2077"),
        nexus_game_id: Some(3333),
        supports_save_profiles: true,
        plugin: &crate::cyberpunk::CYBERPUNK2077,
        scanner: Some(&crate::cyberpunk::scanner::CYBERPUNK_SCANNER),
        save_tracker: Some(&crate::cyberpunk::saves::CYBERPUNK_SAVE_TRACKER),
        collision_classifier: Some(cyberpunk_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "stellar-blade",
        display_name: "Stellar Blade",
        engine: EngineFamily::Unreal4,
        launcher: LauncherIds {
            steam_app_id: Some("3489700"),
            steam_dir: Some("Stellar Blade"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &["StellarBlade"],
        nexus_domain: Some("stellarblade"),
        // nexus_game_id intentionally None: the numeric Nexus game id is
        // not exposed publicly. The domain alone is enough for URL-based
        // installs and update-tracking; only the UI's "Browse Nexus"
        // picker requires the numeric id, so Stellar Blade will be
        // hidden from that picker until we can confirm the value.
        nexus_game_id: None,
        supports_save_profiles: true,
        plugin: &crate::ue4::STELLAR_BLADE,
        scanner: Some(&crate::ue4::scanner::STELLAR_BLADE_SCANNER),
        save_tracker: Some(&crate::ue4::saves::STELLAR_BLADE_SAVE_TRACKER),
        collision_classifier: Some(ue4_collision_classifier),
        optiscaler_profiles: crate::ue4::STELLAR_BLADE_OPTISCALER_PROFILES,
    },
    GameRegistration {
        game_id: "baldurs-gate3",
        display_name: "Baldur's Gate 3",
        engine: EngineFamily::Larian,
        launcher: LauncherIds {
            steam_app_id: Some("1086940"),
            steam_dir: Some("Baldurs Gate 3"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &[],
        nexus_domain: Some("baldursgate3"),
        nexus_game_id: None,
        supports_save_profiles: true,
        plugin: &crate::bg3::BALDURS_GATE3,
        scanner: Some(&crate::bg3::scanner::BG3_SCANNER),
        save_tracker: Some(&crate::bg3::saves::BG3_SAVE_TRACKER),
        collision_classifier: Some(pak_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "stardew-valley",
        display_name: "Stardew Valley",
        engine: EngineFamily::Smapi,
        launcher: LauncherIds {
            steam_app_id: Some("413150"),
            steam_dir: Some("Stardew Valley"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &[],
        nexus_domain: Some("stardewvalley"),
        nexus_game_id: None,
        supports_save_profiles: true,
        plugin: &crate::stardew::STARDEW_VALLEY,
        scanner: Some(&crate::stardew::scanner::STARDEW_SCANNER),
        save_tracker: Some(&crate::stardew::saves::STARDEW_SAVE_TRACKER),
        collision_classifier: Some(generic_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "fallout-new-vegas",
        display_name: "Fallout: New Vegas",
        engine: EngineFamily::Gamebryo,
        launcher: LauncherIds {
            steam_app_id: Some("22380"),
            steam_dir: Some("Fallout New Vegas"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &["FalloutNewVegas", "FalloutNV"],
        nexus_domain: Some("newvegas"),
        nexus_game_id: None,
        supports_save_profiles: true,
        plugin: &crate::gamebryo::FALLOUT_NEW_VEGAS,
        scanner: Some(&crate::gamebryo::scanner::FALLOUT_NEW_VEGAS_SCANNER),
        save_tracker: Some(&crate::gamebryo::saves::GAMEBRYO_SAVE_TRACKER),
        collision_classifier: Some(gamebryo_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "oblivion",
        display_name: "The Elder Scrolls IV: Oblivion",
        engine: EngineFamily::Gamebryo,
        launcher: LauncherIds {
            steam_app_id: Some("22330"),
            steam_dir: Some("Oblivion"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &["Oblivion"],
        nexus_domain: Some("oblivion"),
        nexus_game_id: None,
        supports_save_profiles: true,
        plugin: &crate::gamebryo::OBLIVION,
        scanner: Some(&crate::gamebryo::scanner::OBLIVION_SCANNER),
        save_tracker: Some(&crate::gamebryo::saves::GAMEBRYO_SAVE_TRACKER),
        collision_classifier: Some(gamebryo_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "oblivion-remastered",
        display_name: "The Elder Scrolls IV: Oblivion Remastered",
        engine: EngineFamily::Unreal4,
        launcher: LauncherIds {
            steam_app_id: Some("2623190"),
            steam_dir: Some("Oblivion Remastered"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &["OblivionRemastered"],
        nexus_domain: Some("oblivionremastered"),
        nexus_game_id: None,
        supports_save_profiles: true,
        plugin: &crate::oblivion_remastered::OBLIVION_REMASTERED,
        scanner: Some(&crate::oblivion_remastered::scanner::OBLIVION_REMASTERED_SCANNER),
        save_tracker: Some(&crate::oblivion_remastered::saves::OBLIVION_REMASTERED_SAVE_TRACKER),
        collision_classifier: Some(pak_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "bannerlord",
        display_name: "Mount & Blade II: Bannerlord",
        engine: EngineFamily::Bannerlord,
        launcher: LauncherIds {
            steam_app_id: Some("261550"),
            steam_dir: Some("Mount & Blade II Bannerlord"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &[],
        nexus_domain: Some("mountandblade2bannerlord"),
        nexus_game_id: None,
        supports_save_profiles: true,
        plugin: &crate::bannerlord::BANNERLORD,
        scanner: Some(&crate::bannerlord::scanner::BANNERLORD_SCANNER),
        save_tracker: Some(&crate::bannerlord::saves::BANNERLORD_SAVE_TRACKER),
        collision_classifier: Some(generic_collision_classifier),
        optiscaler_profiles: &[],
    },
    GameRegistration {
        game_id: "witcher3",
        display_name: "The Witcher 3: Wild Hunt",
        engine: EngineFamily::Witcher,
        launcher: LauncherIds {
            steam_app_id: Some("292030"),
            steam_dir: Some("The Witcher 3"),
            heroic_gog_app_id: None,
            heroic_epic_app_id: None,
        },
        wabbajack_names: &[],
        nexus_domain: Some("witcher3"),
        nexus_game_id: None,
        supports_save_profiles: true,
        plugin: &crate::witcher3::WITCHER3,
        scanner: Some(&crate::witcher3::scanner::WITCHER3_SCANNER),
        save_tracker: Some(&crate::witcher3::saves::WITCHER3_SAVE_TRACKER),
        collision_classifier: Some(witcher_collision_classifier),
        optiscaler_profiles: &[],
    },
];

static REGISTRY: OnceLock<RwLock<&'static [GameRegistration]>> = OnceLock::new();

fn build_registry_snapshot() -> &'static [GameRegistration] {
    Box::leak(
        GAME_REGISTRY
            .iter()
            .copied()
            .chain(load_user_games())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

#[must_use]
pub fn all_games() -> &'static [GameRegistration] {
    *REGISTRY
        .get_or_init(|| RwLock::new(build_registry_snapshot()))
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn reload_registry() {
    let registry = REGISTRY.get_or_init(|| RwLock::new(build_registry_snapshot()));
    *registry
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = build_registry_snapshot();
}

#[must_use]
pub fn supported_game_ids() -> Vec<&'static str> {
    all_games().iter().map(|game| game.game_id).collect()
}

#[must_use]
pub fn resolve_game(game_id: &str) -> Option<&'static GameRegistration> {
    all_games().iter().find(|game| game.game_id == game_id)
}

/// Resolve a Nexus Mods domain (e.g. `"stellarblade"`, `"skyrimspecialedition"`)
/// to its registered game. Used by the install pipeline so a Nexus URL
/// like `nexusmods.com/<domain>/mods/<id>` can pick up game-specific
/// install hints even when the domain string differs from the modde
/// `game_id`.
#[must_use]
pub fn resolve_game_by_nexus_domain(domain: &str) -> Option<&'static GameRegistration> {
    all_games()
        .iter()
        .find(|game| game.nexus_domain == Some(domain))
}

pub fn launcher_games() -> impl Iterator<Item = &'static GameRegistration> {
    all_games().iter().filter(|game| {
        game.launcher.steam_app_id.is_some()
            || game.launcher.heroic_gog_app_id.is_some()
            || game.launcher.heroic_epic_app_id.is_some()
    })
}
