use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::fmt;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::toposort;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::profile::Profile;

/// Generates a newtype wrapper around `String` with zero-cost `#[repr(transparent)]`
/// layout. Provides domain-level type safety — you cannot accidentally pass a `ModId`
/// where a `GameId` is expected, or vice versa.
///
/// Each invocation produces a struct with: `Display`, `From<&str>`, `From<String>`,
/// `Borrow<str>`, `AsRef<str>`, `PartialEq<str>`, and `PartialEq<&str>`.
macro_rules! define_id_newtype {
    (
        $(#[$meta:meta])*
        $vis:vis struct $Name:ident;
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[repr(transparent)]
        $vis struct $Name(pub String);

        impl $Name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $Name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $Name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl From<String> for $Name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl Borrow<str> for $Name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl std::ops::Deref for $Name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $Name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $Name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $Name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl rusqlite::types::ToSql for $Name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }
    };
}

define_id_newtype! {
    /// Unique identifier for a mod within a profile.
    ///
    /// Prevents accidental use of arbitrary strings where a mod ID is expected.
    pub struct ModId;
}

define_id_newtype! {
    /// Unique identifier for a supported game (e.g. `"skyrim-se"`, `"cyberpunk2077"`).
    ///
    /// Prevents mixing up game IDs with profile names, mod IDs, or other strings
    /// at the type level. Zero runtime cost via `#[repr(transparent)]`.
    pub struct GameId;
}

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

/// Maps each deployed file path to the set of mods that provide it.
#[derive(Debug, Clone, Default)]
pub struct ConflictMap {
    pub files: HashMap<String, HashSet<ModId>>,
}

impl ConflictMap {
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
                    mods: Box::new(smallvec::smallvec![mod_a.0.clone(), mod_b.0.clone()]),
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
        .map(|&idx| ModId::from(graph[idx]))
        .collect();

    Ok(ResolvedLoadOrder { order })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{EnabledMod, ProfileSource};
    use smallvec::{smallvec, SmallVec};
    use std::path::PathBuf;

    fn make_profile(mods: Vec<&str>, rules: SmallVec<[LoadOrderRule; 4]>) -> Profile {
        Profile {
            id: None,
            name: "test".to_string(),
            game_id: GameId::from("skyrim-se"),
            source: ProfileSource::Manual,
            mods: mods
                .into_iter()
                .map(|id| EnabledMod {
                    mod_id: id.to_string(),
                    enabled: true,
                    version: None,
                    fomod_config: None,
                })
                .collect(),
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: rules,
        }
    }

    #[test]
    fn test_resolve_simple_order() {
        let profile = make_profile(vec!["mod_a", "mod_b", "mod_c"], smallvec![]);
        let result = resolve(&profile).unwrap();
        assert_eq!(result.order.len(), 3);
    }

    #[test]
    fn test_resolve_with_load_after() {
        let profile = make_profile(
            vec!["mod_a", "mod_b", "mod_c"],
            smallvec![LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_c"),
                after: ModId::from("mod_a"),
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
            smallvec![LoadOrderRule::LoadBefore {
                mod_id: ModId::from("mod_a"),
                before: ModId::from("mod_b"),
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
            smallvec![
                LoadOrderRule::LoadAfter {
                    mod_id: ModId::from("mod_b"),
                    after: ModId::from("mod_a"),
                },
                LoadOrderRule::LoadAfter {
                    mod_id: ModId::from("mod_a"),
                    after: ModId::from("mod_b"),
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
            smallvec![LoadOrderRule::Incompatible {
                mod_a: ModId::from("mod_a"),
                mod_b: ModId::from("mod_b"),
            }],
        );
        let result = resolve(&profile);
        assert!(result.is_err());
    }

    #[test]
    fn test_conflict_map() {
        let mut cm = ConflictMap::default();
        cm.register("textures/sky.dds".to_string(), ModId::from("mod_a"));
        cm.register("textures/sky.dds".to_string(), ModId::from("mod_b"));
        cm.register("meshes/tree.nif".to_string(), ModId::from("mod_a"));

        let conflicts = cm.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].0, "textures/sky.dds");
    }

    #[test]
    fn test_disabled_mods_excluded() {
        let profile = Profile {
            id: None,
            name: "test".to_string(),
            game_id: GameId::from("skyrim-se"),
            source: ProfileSource::Manual,
            mods: vec![
                EnabledMod {
                    mod_id: "mod_a".to_string(),
                    enabled: true,
                    version: None,
                    fomod_config: None,
                },
                EnabledMod {
                    mod_id: "mod_b".to_string(),
                    enabled: false,
                    version: None,
                    fomod_config: None,
                },
            ],
            overrides: PathBuf::from("/tmp"),
            load_order_rules: smallvec![],
        };
        let result = resolve(&profile).unwrap();
        assert_eq!(result.order.len(), 1);
        assert_eq!(result.order[0], "mod_a");
    }
}
