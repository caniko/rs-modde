//! Per-game `OptiScaler` compatibility profiles.
//!
//! These profiles are intentionally game-owned metadata. They represent
//! community-tested configurations users may choose when enabling `OptiScaler`;
//! the presence of a profile must not imply `OptiScaler` is enabled by default.

/// One `OptiScaler` INI override expressed as a dotted section/key path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptiScalerIniOverride {
    pub key: &'static str,
    pub value: &'static str,
}

/// A community-tested `OptiScaler` configuration for a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptiScalerProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub source_url: &'static str,
    pub tested_optiscaler_version: &'static str,
    pub source_mode: Option<&'static str>,
    pub goverlay_channel: Option<&'static str>,
    pub proxy_dll: &'static str,
    pub release_tag: Option<&'static str>,
    pub release_asset: Option<&'static str>,
    pub wine_dll_overrides: &'static [&'static str],
    pub copy_companion_files: bool,
    pub enable_optipatcher: bool,
    pub fsr4_variant: Option<&'static str>,
    pub emulate_fp8: bool,
    pub spoof_dlss: bool,
    pub ini_overrides: &'static [OptiScalerIniOverride],
    pub notes: &'static str,
}

/// Opt-in provider for games with known `OptiScaler` compatibility profiles.
pub trait OptiScalerProfiles {
    fn optiscaler_profiles(&self) -> &'static [OptiScalerProfile] {
        &[]
    }
}

/// Resolve community-tested `OptiScaler` profiles for a supported game.
#[must_use]
pub fn resolve_optiscaler_profiles(game_id: &str) -> &'static [OptiScalerProfile] {
    crate::registry::resolve_game(game_id).map_or(&[], |game| game.optiscaler_profiles)
}

/// Return the default community-tested profile for a game, if any.
#[must_use]
pub fn default_optiscaler_profile(game_id: &str) -> Option<&'static OptiScalerProfile> {
    resolve_optiscaler_profiles(game_id).first()
}

#[cfg(test)]
mod tests {
    use super::{default_optiscaler_profile, resolve_optiscaler_profiles};

    #[test]
    fn stellar_blade_resolves_community_optiscaler_profile() {
        let profiles = resolve_optiscaler_profiles("stellar-blade");

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "community-dxgi");
        assert_eq!(profiles[0].proxy_dll, "dxgi.dll");
        assert_eq!(profiles[0].tested_optiscaler_version, "0.9");
        assert_eq!(profiles[0].source_mode, Some("github_release"));
        assert_eq!(profiles[0].goverlay_channel, None);
        assert!(profiles[0].enable_optipatcher);
        assert_eq!(profiles[0].fsr4_variant, Some("latest_fp8"));
        assert!(profiles[0].emulate_fp8);
        assert!(!profiles[0].spoof_dlss);
        assert_eq!(
            default_optiscaler_profile("stellar-blade").map(|profile| profile.id),
            Some("community-dxgi")
        );
    }

    #[test]
    fn unsupported_optiscaler_profiles_resolve_empty() {
        assert!(resolve_optiscaler_profiles("skyrim-se").is_empty());
        assert!(default_optiscaler_profile("skyrim-se").is_none());
    }
}
