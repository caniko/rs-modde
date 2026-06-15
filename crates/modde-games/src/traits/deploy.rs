/// Classes of filesystem roots a [`super::GamePlugin`] can advertise as
/// deployment destinations *outside* the game install dir.
///
/// New variants extend the installer's routing without requiring it to
/// know per-engine path conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployTargetKind {
    /// Per-user config files the engine reads at startup. Examples:
    /// UE4/UE5 `<Project>/Saved/Config/Windows/Engine.ini`, Bethesda
    /// `Documents/My Games/<Game>/*.ini`, Larian's
    /// `AppData/Local/<Game>/Player.ini`. Files in this target are
    /// usually whole-file replacements keyed by filename.
    UserConfig,
    /// Per-user save directory. Reserved for save-replacing mods (rare
    /// but real, e.g. shipped 100% completion saves).
    UserSaves,
    /// Anything the plugin wants to expose that doesn't fit the above.
    /// The installer just routes files to the resolved path; semantics
    /// are entirely the plugin's.
    Custom,
}

/// A named alternate deployment root advertised by a [`super::GamePlugin`].
///
/// The installer pipeline keys mods to a target by `id`; the plugin
/// resolves `id` → real path at deploy time via
/// [`super::GamePlugin::resolve_deploy_target`]. Resolution is deferred so
/// plugins can incorporate runtime context (Wine prefix, Steam
/// `compatdata`, XDG dirs) without baking a path into a static.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeployTarget {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: DeployTargetKind,
}

/// Runtime VFS patch support advertised by a game plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotDeploySupport {
    Unsupported,
    Experimental,
}

/// Describes whether a game can accept a narrow live-deploy patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotDeployCapability {
    pub support: HotDeploySupport,
    pub cosmetic_only: bool,
}

impl HotDeployCapability {
    #[must_use]
    pub const fn unsupported() -> Self {
        Self {
            support: HotDeploySupport::Unsupported,
            cosmetic_only: true,
        }
    }

    #[must_use]
    pub const fn experimental_cosmetic_only() -> Self {
        Self {
            support: HotDeploySupport::Experimental,
            cosmetic_only: true,
        }
    }

    #[must_use]
    pub const fn is_supported(self) -> bool {
        !matches!(self.support, HotDeploySupport::Unsupported)
    }
}
