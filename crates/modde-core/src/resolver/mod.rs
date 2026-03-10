use std::collections::{HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::toposort;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::profile::Profile;

/// Unique identifier for a mod within a profile.
pub type ModId = String;

/// A rule constraining load order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoadOrderRule {
    /// This mod must load after the specified mod.
    LoadAfter { mod_id: ModId, after: ModId },
    /// This mod must load before the specified mod.
    LoadBefore { mod_id: ModId, before: ModId },
    /// These two mods are incompatible; error if both enabled.
    Incompatible { mod_a: ModId, mod_b: ModId },
}

/// Ordered load order with LOOT-style rule support.
#[derive(Debug, Clone)]
pub struct LoadOrder {
    pub mods: Vec<ModId>,
    pub rules: Vec<LoadOrderRule>,
}

/// Maps each deployed file path to the set of mods that provide it.
#[derive(Debug, Clone, Default)]
pub struct ConflictMap {
    pub files: HashMap<String, HashSet<ModId>>,
}

impl ConflictMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register that `mod_id` provides `file_path`.
    pub fn register(&mut self, file_path: String, mod_id: ModId) {
        self.files.entry(file_path).or_default().insert(mod_id);
    }

    /// Return all file paths that have more than one provider.
    pub fn conflicts(&self) -> Vec<(&str, &HashSet<ModId>)> {
        self.files
            .iter()
            .filter(|(_, mods)| mods.len() > 1)
            .map(|(path, mods)| (path.as_str(), mods))
            .collect()
    }
}

/// The result of resolving a profile's load order.
#[derive(Debug, Clone)]
pub struct ResolvedLoadOrder {
    /// Mods in final load order (first = lowest priority).
    pub order: Vec<ModId>,
    /// File conflict map.
    pub conflicts: ConflictMap,
}

/// Resolve a profile into a topologically sorted load order.
///
/// Uses petgraph to build a DAG from mods and load order rules,
/// then performs a topological sort.
pub fn resolve(profile: &Profile) -> Result<ResolvedLoadOrder> {
    let enabled_mods: Vec<&str> = profile
        .mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.mod_id.as_str())
        .collect();

    let enabled_set: HashSet<&str> = enabled_mods.iter().copied().collect();

    // Check for incompatible mods
    for rule in &profile.load_order_rules {
        if let LoadOrderRule::Incompatible { mod_a, mod_b } = rule {
            if enabled_set.contains(mod_a.as_str()) && enabled_set.contains(mod_b.as_str()) {
                return Err(CoreError::FileConflict {
                    path: String::new(),
                    mods: vec![mod_a.clone(), mod_b.clone()],
                });
            }
        }
    }

    // Build DAG
    let mut graph = DiGraph::<&str, ()>::new();
    let mut node_map: HashMap<&str, NodeIndex> = HashMap::new();

    for mod_id in &enabled_mods {
        let idx = graph.add_node(mod_id);
        node_map.insert(mod_id, idx);
    }

    // Add edges from load order rules
    for rule in &profile.load_order_rules {
        match rule {
            LoadOrderRule::LoadAfter { mod_id, after } => {
                if let (Some(&from), Some(&to)) = (node_map.get(after.as_str()), node_map.get(mod_id.as_str())) {
                    graph.add_edge(from, to, ());
                }
            }
            LoadOrderRule::LoadBefore { mod_id, before } => {
                if let (Some(&from), Some(&to)) = (node_map.get(mod_id.as_str()), node_map.get(before.as_str())) {
                    graph.add_edge(from, to, ());
                }
            }
            LoadOrderRule::Incompatible { .. } => {} // Already handled above
        }
    }

    // Topological sort
    let sorted = toposort(&graph, None).map_err(|cycle| {
        let mod_id = graph[cycle.node_id()];
        CoreError::DependencyCycle(mod_id.to_string())
    })?;

    let order: Vec<ModId> = sorted
        .iter()
        .map(|&idx| graph[idx].to_string())
        .collect();

    // Build conflict map from file mappings
    let conflicts = ConflictMap::new();
    // Actual file registration would happen during VFS construction;
    // here we return an empty map that the caller can populate.

    Ok(ResolvedLoadOrder { order, conflicts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{EnabledMod, ProfileSource};
    use std::path::PathBuf;

    fn make_profile(mods: Vec<&str>, rules: Vec<LoadOrderRule>) -> Profile {
        Profile {
            name: "test".to_string(),
            game_id: "skyrim-se".to_string(),
            source: ProfileSource::Manual,
            mods: mods
                .into_iter()
                .map(|id| EnabledMod {
                    mod_id: id.to_string(),
                    enabled: true,
                    version: None,
                })
                .collect(),
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: rules,
        }
    }

    #[test]
    fn test_resolve_simple_order() {
        let profile = make_profile(vec!["mod_a", "mod_b", "mod_c"], vec![]);
        let result = resolve(&profile).unwrap();
        assert_eq!(result.order.len(), 3);
    }

    #[test]
    fn test_resolve_with_load_after() {
        let profile = make_profile(
            vec!["mod_a", "mod_b", "mod_c"],
            vec![LoadOrderRule::LoadAfter {
                mod_id: "mod_c".to_string(),
                after: "mod_a".to_string(),
            }],
        );
        let result = resolve(&profile).unwrap();
        let pos_a = result.order.iter().position(|m| m == "mod_a").unwrap();
        let pos_c = result.order.iter().position(|m| m == "mod_c").unwrap();
        assert!(pos_a < pos_c, "mod_a should come before mod_c");
    }

    #[test]
    fn test_resolve_with_load_before() {
        let profile = make_profile(
            vec!["mod_a", "mod_b"],
            vec![LoadOrderRule::LoadBefore {
                mod_id: "mod_a".to_string(),
                before: "mod_b".to_string(),
            }],
        );
        let result = resolve(&profile).unwrap();
        let pos_a = result.order.iter().position(|m| m == "mod_a").unwrap();
        let pos_b = result.order.iter().position(|m| m == "mod_b").unwrap();
        assert!(pos_a < pos_b);
    }

    #[test]
    fn test_resolve_cycle_detection() {
        let profile = make_profile(
            vec!["mod_a", "mod_b"],
            vec![
                LoadOrderRule::LoadAfter {
                    mod_id: "mod_b".to_string(),
                    after: "mod_a".to_string(),
                },
                LoadOrderRule::LoadAfter {
                    mod_id: "mod_a".to_string(),
                    after: "mod_b".to_string(),
                },
            ],
        );
        let result = resolve(&profile);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_incompatible() {
        let profile = make_profile(
            vec!["mod_a", "mod_b"],
            vec![LoadOrderRule::Incompatible {
                mod_a: "mod_a".to_string(),
                mod_b: "mod_b".to_string(),
            }],
        );
        let result = resolve(&profile);
        assert!(result.is_err());
    }

    #[test]
    fn test_conflict_map() {
        let mut cm = ConflictMap::new();
        cm.register("textures/sky.dds".to_string(), "mod_a".to_string());
        cm.register("textures/sky.dds".to_string(), "mod_b".to_string());
        cm.register("meshes/tree.nif".to_string(), "mod_a".to_string());

        let conflicts = cm.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].0, "textures/sky.dds");
    }

    #[test]
    fn test_disabled_mods_excluded() {
        let profile = Profile {
            name: "test".to_string(),
            game_id: "skyrim-se".to_string(),
            source: ProfileSource::Manual,
            mods: vec![
                EnabledMod {
                    mod_id: "mod_a".to_string(),
                    enabled: true,
                    version: None,
                },
                EnabledMod {
                    mod_id: "mod_b".to_string(),
                    enabled: false,
                    version: None,
                },
            ],
            overrides: PathBuf::from("/tmp"),
            load_order_rules: vec![],
        };
        let result = resolve(&profile).unwrap();
        assert_eq!(result.order.len(), 1);
        assert_eq!(result.order[0], "mod_a");
    }
}
