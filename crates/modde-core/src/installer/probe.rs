//! Game-specific install detection hooks.
//!
//! `modde-core` cannot reference `modde-games` (would create a dependency
//! cycle), so the analyzer takes an [`InstallProbe`] that the caller
//! constructs from whatever game plugin context it has. `modde-games`
//! provides a `game_probe(plugin)` helper that wraps a
//! `&'static dyn GamePlugin` into a probe.
//!
//! The probe owns its closures (as `Box<dyn Fn>`), so analysis does not
//! leak memory and the probe can be passed across tasks.

use std::path::Path;

use super::types::InstallMethod;

/// Callbacks the analyzer uses to delegate to a game plugin.
///
/// Both hooks have sensible defaults ([`InstallProbe::noop`]), so a game
/// plugin with no special layouts can just leave them unimplemented.
pub struct InstallProbe {
    /// Return a game-specific [`InstallMethod`] if the plugin recognizes
    /// the extracted archive authoritatively (e.g. Cyberpunk identifying
    /// a `REDmod` by `info.json` + `archives/` presence). Runs **before**
    /// the generic probes so it can claim layouts that also happen to
    /// trigger generic heuristics.
    pub analyze: Box<dyn Fn(&Path) -> Option<InstallMethod> + Send + Sync>,

    /// Return `true` if the extracted archive looks like a bare-extract
    /// for this game (e.g. top-level `Data/` for Bethesda). Runs as the
    /// last fallback before [`InstallMethod::Unknown`].
    pub recognizes_bare: Box<dyn Fn(&Path) -> bool + Send + Sync>,

    /// Plugin-supplied id of a `DeployTargetKind::UserConfig` root, if
    /// this game advertises one. The analyzer falls back to a
    /// [`InstallMethod::UserConfigOverlay`] keyed on this id when the
    /// archive contains only config-shaped files. `None` means the
    /// game has no user-config target — config-only archives will go
    /// straight to `Unknown`.
    pub user_config_target: Option<&'static str>,
}

impl InstallProbe {
    /// Construct a probe from two closures.
    pub fn new<A, B>(analyze: A, recognizes_bare: B) -> Self
    where
        A: Fn(&Path) -> Option<InstallMethod> + Send + Sync + 'static,
        B: Fn(&Path) -> bool + Send + Sync + 'static,
    {
        Self {
            analyze: Box::new(analyze),
            recognizes_bare: Box::new(recognizes_bare),
            user_config_target: None,
        }
    }

    /// Builder: attach a user-config target id. The analyzer will
    /// route config-only archives to `UserConfigOverlay { target_id }`.
    #[must_use]
    pub fn with_user_config_target(mut self, target_id: &'static str) -> Self {
        self.user_config_target = Some(target_id);
        self
    }

    /// A probe that never claims anything game-specific. Used by tests of
    /// the generic detection pipeline and as a "no plugin available"
    /// fallback.
    #[must_use]
    pub fn noop() -> Self {
        Self {
            analyze: Box::new(|_| None),
            recognizes_bare: Box::new(|_| false),
            user_config_target: None,
        }
    }
}
