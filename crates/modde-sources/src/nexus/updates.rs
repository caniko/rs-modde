use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::api::NexusApi;

/// Information about a mod that has an available update on Nexus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModUpdate {
    /// The mod_id string used in the profile (local identifier).
    pub mod_id: String,
    /// Nexus mod ID.
    pub nexus_mod_id: u64,
    /// Currently installed version (if known).
    pub installed_version: Option<String>,
    /// Timestamp when the mod was installed locally.
    pub installed_timestamp: i64,
    /// Timestamp of the latest file update on Nexus.
    pub latest_file_update: u64,
    /// Timestamp of the latest mod activity on Nexus.
    pub latest_mod_activity: u64,
}

/// A mod tracked for update checking.
#[derive(Debug, Clone)]
pub struct TrackedMod {
    pub mod_id: String,
    pub nexus_mod_id: u64,
    pub nexus_game_domain: String,
    pub installed_version: Option<String>,
    pub installed_timestamp: i64,
}

/// Check for updates for a set of tracked mods.
///
/// Groups mods by game domain, calls `updated_mods` once per domain,
/// then cross-references against installed timestamps.
pub async fn check_updates(
    api: &NexusApi,
    tracked: &[TrackedMod],
    period: &str,
) -> Result<Vec<ModUpdate>> {
    if tracked.is_empty() {
        return Ok(Vec::new());
    }

    // Group tracked mods by game domain
    let mut by_domain: HashMap<&str, Vec<&TrackedMod>> = HashMap::new();
    for t in tracked {
        by_domain.entry(t.nexus_game_domain.as_str()).or_default().push(t);
    }

    let mut updates = Vec::new();

    for (domain, domain_mods) in &by_domain {
        debug!(domain, mod_count = domain_mods.len(), "checking updates for domain");

        // Build a lookup: nexus_mod_id -> TrackedMod
        let lookup: HashMap<u64, &&TrackedMod> = domain_mods
            .iter()
            .map(|m| (m.nexus_mod_id, m))
            .collect();

        // One API call per domain
        let updated = api.updated_mods(domain, period).await?;

        for nexus_mod in &updated {
            if let Some(tracked_mod) = lookup.get(&nexus_mod.mod_id) {
                // Mod is in our tracked list — check if update is newer
                let install_ts = tracked_mod.installed_timestamp as u64;
                if nexus_mod.latest_file_update > install_ts {
                    updates.push(ModUpdate {
                        mod_id: tracked_mod.mod_id.clone(),
                        nexus_mod_id: nexus_mod.mod_id,
                        installed_version: tracked_mod.installed_version.clone(),
                        installed_timestamp: tracked_mod.installed_timestamp,
                        latest_file_update: nexus_mod.latest_file_update,
                        latest_mod_activity: nexus_mod.latest_mod_activity,
                    });
                }
            }
        }
    }

    if updates.is_empty() {
        info!("all tracked mods are up to date");
    } else {
        info!(count = updates.len(), "found mods with available updates");
    }

    Ok(updates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracked_mod_grouping() {
        let tracked = vec![
            TrackedMod {
                mod_id: "skyui".to_string(),
                nexus_mod_id: 12604,
                nexus_game_domain: "skyrimspecialedition".to_string(),
                installed_version: Some("5.2".to_string()),
                installed_timestamp: 1700000000,
            },
            TrackedMod {
                mod_id: "ussep".to_string(),
                nexus_mod_id: 266,
                nexus_game_domain: "skyrimspecialedition".to_string(),
                installed_version: Some("4.2.8".to_string()),
                installed_timestamp: 1700000000,
            },
            TrackedMod {
                mod_id: "fo4_patch".to_string(),
                nexus_mod_id: 4598,
                nexus_game_domain: "fallout4".to_string(),
                installed_version: None,
                installed_timestamp: 1700000000,
            },
        ];

        let mut by_domain: HashMap<&str, Vec<&TrackedMod>> = HashMap::new();
        for t in &tracked {
            by_domain.entry(t.nexus_game_domain.as_str()).or_default().push(t);
        }

        assert_eq!(by_domain.len(), 2);
        assert_eq!(by_domain["skyrimspecialedition"].len(), 2);
        assert_eq!(by_domain["fallout4"].len(), 1);
    }
}
