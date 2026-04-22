pub mod scanner;

use std::path::{Path, PathBuf};

use smallvec::SmallVec;

use crate::traits::{
    ContentCategory, GamePlugin, ModClassifyConfig, ModSafety, classify_mod_by_content,
};

/// Data-driven UE4 game plugin.
///
/// UE4 titles share a near-identical layout: a project folder under the install
/// root (`SB/`, `Palworld/`, etc.) that contains `Content/Paks/` for pak files
/// and `Binaries/Win64/` for the shipping executable and proxy DLLs. Mods drop
/// into `Content/Paks/~mods/` (the tilde forces mount order after base paks).
///
/// A new UE4 game needs only a new `const` instance; the trait impl is shared.
pub struct Ue4Game {
    game_id: &'static str,
    display_name: &'static str,
    /// Steam App ID as a string (parsed on demand for `steam_app_id_u32`).
    steam_app_id: &'static str,
    /// UE4 project short name — the folder under the install root that
    /// contains `Content/Paks/` and `Binaries/Win64/`. For Stellar Blade this
    /// is `"SB"`.
    project_name: &'static str,
    /// Nexus Mods game domain name, if the game is tracked there.
    nexus_domain: Option<&'static str>,
}

impl Ue4Game {
    #[must_use]
    pub const fn new(
        game_id: &'static str,
        display_name: &'static str,
        steam_app_id: &'static str,
        project_name: &'static str,
        nexus_domain: Option<&'static str>,
    ) -> Self {
        Self {
            game_id,
            display_name,
            steam_app_id,
            project_name,
            nexus_domain,
        }
    }

    /// `<install>/<ProjectName>/Content/Paks`
    #[must_use]
    pub fn paks_root(&self, install: &Path) -> PathBuf {
        install.join(self.project_name).join("Content").join("Paks")
    }

    #[must_use]
    pub fn project_name(&self) -> &'static str {
        self.project_name
    }
}

/// UE4SS / proxy DLLs commonly used by UE4 mod loaders. Detected in
/// `<install>/<ProjectName>/Binaries/Win64/` and surfaced as `WINEDLLOVERRIDES`
/// so Wine loads the native (mod) DLL instead of its built-in stub.
const UE4_PROXY_DLLS: &[&str] = &[
    "dwmapi",    // UE4SS default proxy
    "xinput1_3", // UE4SS alternate + some trainers
    "d3d11",     // ReShade / ENB-style
    "dxgi",      // ReShade / DLSS swappers
    "version",   // Generic ASI loader
    "winmm",     // Generic ASI loader
    "dinput8",   // Generic hook
];

/// File extensions that indicate a UE4 mod alters game logic.
const UE4_SAVE_BREAKING_EXT: &[&str] = &["pak", "ucas", "utoc", "dll", "lua"];

/// File extensions that are purely cosmetic in UE4 games.
const UE4_COSMETIC_EXT: &[&str] = &["png", "jpg", "dds", "tga", "ini"];

const UE4_CLASSIFY_CONFIG: ModClassifyConfig = ModClassifyConfig {
    save_breaking_ext: UE4_SAVE_BREAKING_EXT,
    cosmetic_ext: UE4_COSMETIC_EXT,
    save_breaking_dirs: &[],
};

pub const STELLAR_BLADE: Ue4Game = Ue4Game::new(
    "stellar-blade",
    "Stellar Blade",
    "3489700",
    "SB",
    // Not currently listed on Nexus Mods.
    None,
);

impl GamePlugin for Ue4Game {
    fn game_id(&self) -> &str {
        self.game_id
    }

    fn display_name(&self) -> &str {
        self.display_name
    }

    /// Deploy target: `<install>/<ProjectName>/Content/Paks/~mods`.
    ///
    /// The tilde prefix forces UE4's pak mounter to load these after the
    /// shipping paks so mods can override base content.
    fn mod_directory(&self, install: &Path) -> PathBuf {
        self.paks_root(install).join("~mods")
    }

    fn executable_dir(&self, install: &Path) -> PathBuf {
        install
            .join(self.project_name)
            .join("Binaries")
            .join("Win64")
    }

    fn wine_dll_overrides(&self, game_dir: &Path) -> SmallVec<[String; 4]> {
        let exe_dir = self.executable_dir(game_dir);
        let mut out: SmallVec<[String; 4]> = SmallVec::new();
        for &name in UE4_PROXY_DLLS {
            if exe_dir.join(format!("{name}.dll")).exists() {
                out.push(name.to_string());
            }
        }
        out
    }

    fn wine_dll_overrides_from_staging(&self, staging: &Path) -> SmallVec<[String; 4]> {
        // UE4 mods that ship a proxy DLL (e.g. UE4SS) drop it at the root of
        // their mod dir. Walk one level for any *.dll and match.
        let mut out: SmallVec<[String; 4]> = SmallVec::new();
        let Ok(entries) = std::fs::read_dir(staging) else {
            return out;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Ok(inner) = std::fs::read_dir(entry.path()) else {
                continue;
            };
            for f in inner.flatten() {
                let name = f.file_name().to_string_lossy().to_lowercase();
                if let Some(stem) = name.strip_suffix(".dll")
                    && UE4_PROXY_DLLS.contains(&stem) && !out.iter().any(|x| x == stem) {
                        out.push(stem.to_string());
                    }
            }
        }
        out
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        classify_mod_by_content(mod_dir, &UE4_CLASSIFY_CONFIG)
    }

    fn classify_extension(&self, ext: &str) -> ContentCategory {
        match ext {
            "pak" | "ucas" | "utoc" => ContentCategory::Archive,
            "dll" | "so" => ContentCategory::Binary,
            "lua" => ContentCategory::Script,
            "dds" | "png" | "tga" | "jpg" => ContentCategory::Texture,
            "ini" | "json" | "yaml" | "xml" | "toml" => ContentCategory::Config,
            _ => ContentCategory::Other,
        }
    }

    fn archive_extensions(&self) -> &[&str] {
        &["pak", "ucas", "utoc"]
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        self.steam_app_id.parse().ok()
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        self.nexus_domain
    }
}
