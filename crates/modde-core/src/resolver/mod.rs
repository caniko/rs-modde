use std::borrow::Borrow;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt;

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

        // Binding to SQL goes through `crate::db::Val` (see `From<&$Name> for Val`),
        // so no driver-specific `ToSql` impl is needed here.
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
    #[must_use]
    pub fn conflicts(&self) -> Vec<(&str, &HashSet<ModId>)> {
        self.files
            .iter()
            .filter(|(_, mods)| mods.len() > 1)
            .map(|(path, mods)| (path.as_str(), mods))
            .collect()
    }

    /// Determine the winner for a given file path based on mod priority order.
    ///
    /// `priority_order` lists mods from lowest to highest priority.
    /// The last mod in the list that provides the file wins.
    /// Hidden `(mod_id, rel_path)` pairs are excluded.
    #[must_use]
    pub fn winner_for(
        &self,
        file_path: &str,
        priority_order: &[ModId],
        hidden: &HashSet<(String, String)>,
    ) -> Option<ModId> {
        let providers = self.files.get(file_path)?;
        priority_order
            .iter()
            .rev()
            .find(|mod_id| {
                providers.contains(*mod_id)
                    && !hidden.contains(&(mod_id.0.clone(), file_path.to_string()))
            })
            .cloned()
    }

    /// Return all conflicts with their resolved winners.
    ///
    /// Returns `(file_path, all_providers, winner)` tuples.
    #[must_use]
    pub fn resolved_conflicts(
        &self,
        priority_order: &[ModId],
        hidden: &HashSet<(String, String)>,
    ) -> Vec<(&str, &HashSet<ModId>, Option<ModId>)> {
        self.conflicts()
            .into_iter()
            .map(|(path, providers)| {
                let winner = self.winner_for(path, priority_order, hidden);
                (path, providers, winner)
            })
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
/// **Stability contract:** the output preserves `profile.mods` input order
/// wherever possible, deviating *only* when a `LoadAfter` / `LoadBefore`
/// rule would otherwise be violated. This means:
///
/// 1. **No rules → exact input order.** `resolve(profile).order` equals the
///    enabled subset of `profile.mods` in the same sequence.
/// 2. **Round-trip via swap.** Swapping two adjacent mods in `profile.mods`
///    produces a resolved order with those two mods swapped, as long as no
///    rule spans the swap. This is what makes `Message::ReorderMod` visible
///    in the `load_order` view — without stability, reordering could
///    silently vanish.
/// 3. **Minimal change under rules.** When a rule *does* force movement,
///    only the rule-involved pair shifts; unrelated neighbors stay put.
/// 4. **Deterministic.** Identical inputs always produce identical outputs;
///    we don't rely on `HashMap` iteration order anywhere.
///
/// ## Algorithm
///
/// Stable Kahn's with input-position tiebreaking:
///
/// 1. Collect enabled mods, recording each `mod_id → input_pos`.
/// 2. Build adjacency + in-degree from `LoadAfter` / `LoadBefore` rules,
///    silently dropping edges whose endpoints aren't enabled (matches
///    the old behaviour).
/// 3. Seed a min-heap (`BinaryHeap<Reverse<(input_pos, mod_id)>>`) with
///    every node whose in-degree is 0.
/// 4. Pop the smallest-input-position ready node, emit it, decrement the
///    in-degree of its successors, pushing any that hit 0.
/// 5. If fewer nodes come out than went in, there's a cycle — pick any
///    remaining node to name in `CoreError::DependencyCycle`.
///
/// `Incompatible` rules are checked up-front and short-circuit the
/// resolution with a `FileConflict` error (unchanged from the old impl).
pub fn resolve(profile: &Profile) -> Result<ResolvedLoadOrder> {
    // Enabled mods, in input order. `input_pos[mod_id] = index in this Vec`.
    let enabled_mods: Vec<&str> = profile
        .mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.mod_id.as_str())
        .collect();

    let input_pos: HashMap<&str, usize> = enabled_mods
        .iter()
        .enumerate()
        .map(|(i, &m)| (m, i))
        .collect();
    let enabled_set: HashSet<&str> = enabled_mods.iter().copied().collect();

    // Check for incompatible mods — must fail before we try to resolve.
    for rule in &profile.load_order_rules {
        if let LoadOrderRule::Incompatible { mod_a, mod_b } = rule
            && enabled_set.contains(mod_a.as_str())
            && enabled_set.contains(mod_b.as_str())
        {
            return Err(CoreError::FileConflict {
                path: String::new(),
                mods: Box::new(smallvec::smallvec![mod_a.0.clone(), mod_b.0.clone()]),
            });
        }
    }

    // Build adjacency + in-degree. `successors[u] = [v, ...]` means "u must
    // be emitted before v".
    let mut successors: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = enabled_mods.iter().map(|&m| (m, 0usize)).collect();

    for rule in &profile.load_order_rules {
        let (from, to) = match rule {
            // `mod_id must load after after` → `after` must come before `mod_id`
            LoadOrderRule::LoadAfter { mod_id, after } => (after.as_str(), mod_id.as_str()),
            // `mod_id must load before before` → `mod_id` must come before `before`
            LoadOrderRule::LoadBefore { mod_id, before } => (mod_id.as_str(), before.as_str()),
            LoadOrderRule::Incompatible { .. } => continue,
        };
        // Silently drop rules referencing disabled / unknown mods, matching
        // the old petgraph-based implementation.
        if !enabled_set.contains(from) || !enabled_set.contains(to) {
            continue;
        }
        successors.entry(from).or_default().push(to);
        *in_degree.get_mut(to).expect("to is enabled") += 1;
    }

    // Min-heap keyed on input position. `Reverse` flips the default max-heap
    // to a min-heap; ties on `input_pos` are impossible because positions
    // are unique, but we include the mod_id in the tuple for total ordering.
    let mut ready: BinaryHeap<Reverse<(usize, &str)>> = BinaryHeap::new();
    for &m in &enabled_mods {
        if in_degree[m] == 0 {
            ready.push(Reverse((input_pos[m], m)));
        }
    }

    let mut order: Vec<ModId> = Vec::with_capacity(enabled_mods.len());
    while let Some(Reverse((_, m))) = ready.pop() {
        order.push(ModId::from(m));
        if let Some(succs) = successors.get(m) {
            for &s in succs {
                let d = in_degree.get_mut(s).expect("successor is enabled");
                *d -= 1;
                if *d == 0 {
                    ready.push(Reverse((input_pos[s], s)));
                }
            }
        }
    }

    // Cycle detection: if any node still has in_degree > 0, it's part of a
    // cycle. Name one of the surviving nodes in the error message, matching
    // the old `toposort` behaviour.
    if order.len() != enabled_mods.len() {
        let offender = enabled_mods
            .iter()
            .find(|m| in_degree.get(**m).copied().unwrap_or(0) > 0)
            .copied()
            .unwrap_or("<unknown>");
        return Err(CoreError::DependencyCycle(offender.to_string()));
    }

    Ok(ResolvedLoadOrder { order })
}

#[cfg(test)]
mod tests;
