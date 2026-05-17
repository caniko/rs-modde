pub(crate) mod leak;
pub mod loader;
pub mod manage;
pub mod spec;

use std::path::{Path, PathBuf};

use smallvec::SmallVec;

use crate::traits::GamePlugin;

use self::spec::GameSpec;

/// A generic game with loose file drop support.
pub struct GenericGame {
    id: String,
    name: String,
    install_path_override: Option<PathBuf>,
    install_dir_name: Option<String>,
    mod_dir: Option<PathBuf>,
    executable_dir: PathBuf,
    proxy_dlls: Vec<String>,
    steam_app_id: Option<String>,
    nexus_domain: Option<String>,
}

impl GenericGame {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        install_path_override: Option<PathBuf>,
        mod_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            install_path_override,
            install_dir_name: None,
            mod_dir: Some(mod_dir.into()),
            executable_dir: PathBuf::new(),
            proxy_dlls: Vec::new(),
            steam_app_id: None,
            nexus_domain: None,
        }
    }

    pub fn from_spec(spec: GameSpec) -> Self {
        Self {
            id: spec.id,
            name: spec.display_name,
            install_path_override: spec.install_path_override,
            install_dir_name: spec.install_dir_name,
            mod_dir: spec.mod_dir,
            executable_dir: spec.executable_dir,
            proxy_dlls: spec.proxy_dlls,
            steam_app_id: spec.steam_app_id,
            nexus_domain: spec.nexus_domain,
        }
    }

    fn install_from_steam_dir_name(dir_name: &str) -> Option<PathBuf> {
        modde_core::paths::steam_library_folders()
            .into_iter()
            .map(|library| library.join("steamapps/common").join(dir_name))
            .find(|path| path.is_dir())
    }
}

impl GamePlugin for GenericGame {
    fn game_id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.name
    }

    fn detect_install(&self) -> Option<PathBuf> {
        if let Some(path) = self
            .install_path_override
            .as_ref()
            .filter(|path| path.is_dir())
        {
            return Some(path.clone());
        }

        if let Some(dir_name) = self.install_dir_name.as_deref()
            && let Some(path) = Self::install_from_steam_dir_name(dir_name)
        {
            return Some(path);
        }

        crate::detection::find_game_install(self.game_id())
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        self.mod_dir
            .as_ref()
            .map_or_else(|| install.to_path_buf(), |dir| install.join(dir))
    }

    fn executable_dir(&self, install: &Path) -> PathBuf {
        install.join(&self.executable_dir)
    }

    fn wine_dll_overrides(&self, install: &Path) -> SmallVec<[String; 4]> {
        let executable_dir = self.executable_dir(install);
        self.proxy_dlls
            .iter()
            .filter(|name| executable_dir.join(format!("{name}.dll")).exists())
            .cloned()
            .collect()
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        self.steam_app_id.as_deref()?.parse().ok()
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        self.nexus_domain.as_deref()
    }
}
