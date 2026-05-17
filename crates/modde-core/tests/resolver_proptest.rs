//! Property-based tests for the load-order resolver.
//!
//! The doc-stated stability contract on [`modde_core::resolver::resolve`]
//! is rich enough that example-based tests can't cover the corners.
//! These properties pin the load-bearing invariants:
//!
//! 1. **Identity under no rules** — output equals the enabled subset
//!    of the input, in the same order.
//! 2. **Determinism** — same profile in, same order out, every time.
//! 3. **`LoadAfter` is honoured** — for every `LoadAfter { a, after: b }`
//!    rule, if both are enabled the output places `b` before `a`.
//! 4. **Disabled mods are dropped** — they never appear in the output.
//!
//! When this file fails, proptest prints a minimal counter-example
//! that you can drop straight into a unit test as a regression guard.

use std::path::PathBuf;

use proptest::prelude::*;
use smallvec::SmallVec;

use modde_core::GameId;
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::resolver::{LoadOrderRule, ModId, resolve};

/// Generator: a vec of distinct mod-id strings of length 1..=N.
///
/// We use string-prefix collision as a forcing function: small alphabet
/// + small length = real chance of duplicates the dedup must handle.
fn mod_ids(max: usize) -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec("[a-c]{1,3}", 1..=max).prop_map(|v| {
        // Dedup while preserving first-seen order.
        let mut seen = std::collections::HashSet::new();
        v.into_iter().filter(|s| seen.insert(s.clone())).collect()
    })
}

fn enabled_mods(ids: &[String]) -> Vec<EnabledMod> {
    ids.iter()
        .map(|id| EnabledMod {
            mod_id: id.clone(),
            enabled: true,
            version: None,
            fomod_config: None,
            ..Default::default()
        })
        .collect()
}

fn profile_with(mods: Vec<EnabledMod>, rules: Vec<LoadOrderRule>) -> Profile {
    Profile {
        id: None,
        name: "proptest".into(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp"),
        load_order_rules: SmallVec::from_vec(rules),
        load_order_lock: None,
    }
}

proptest! {
    /// With no rules, the output is exactly the enabled subset of the input,
    /// in the same order. This is the doc-stated "identity" guarantee.
    #[test]
    fn identity_under_no_rules(ids in mod_ids(20)) {
        let profile = profile_with(enabled_mods(&ids), vec![]);
        let resolved = resolve(&profile).expect("no rules → no cycle");
        let got: Vec<&str> = resolved
            .order
            .iter()
            .map(modde_core::ModId::as_str)
            .collect();
        let want: Vec<&str> = ids.iter().map(String::as_str).collect();
        prop_assert_eq!(got, want);
    }

    /// Resolution is deterministic: identical inputs produce identical outputs
    /// across repeated calls. Guards against accidental reliance on hash-map
    /// iteration order.
    #[test]
    fn deterministic(ids in mod_ids(15)) {
        let profile = profile_with(enabled_mods(&ids), vec![]);
        let a = resolve(&profile).unwrap().order;
        let b = resolve(&profile).unwrap().order;
        prop_assert_eq!(a, b);
    }

    /// `LoadAfter { a, after: b }` is honoured: in the output, `b` precedes `a`
    /// whenever both are enabled. We construct the rule between two distinct
    /// indices in the enabled set so the rule is always well-formed.
    #[test]
    fn load_after_is_honoured(
        ids in mod_ids(10).prop_filter("need 2+ mods", |v| v.len() >= 2),
        i in 0usize..10,
        j in 0usize..10,
    ) {
        let n = ids.len();
        let i = i % n;
        let j = j % n;
        prop_assume!(i != j);

        // Rule: ids[i] must load after ids[j] → ids[j] must come before ids[i]
        let rule = LoadOrderRule::LoadAfter {
            mod_id: ModId::from(ids[i].as_str()),
            after: ModId::from(ids[j].as_str()),
        };
        let profile = profile_with(enabled_mods(&ids), vec![rule]);
        let resolved = resolve(&profile).expect("DAG of one edge has no cycle");

        let pos = |needle: &str| {
            resolved.order.iter().position(|m| m.as_str() == needle)
        };
        let pi = pos(&ids[i]).expect("ids[i] enabled");
        let pj = pos(&ids[j]).expect("ids[j] enabled");
        prop_assert!(pj < pi, "{} (j) should precede {} (i); order={:?}",
                     &ids[j], &ids[i], resolved.order);
    }

    /// Disabled mods never appear in the resolved order.
    #[test]
    fn disabled_mods_are_dropped(
        ids in mod_ids(15).prop_filter("need 1+", |v| !v.is_empty()),
        disabled_idx in 0usize..15,
    ) {
        let mut mods = enabled_mods(&ids);
        let idx = disabled_idx % mods.len();
        mods[idx].enabled = false;
        let disabled_id = mods[idx].mod_id.clone();

        let profile = profile_with(mods, vec![]);
        let resolved = resolve(&profile).unwrap();
        prop_assert!(
            resolved.order.iter().all(|m| m.as_str() != disabled_id),
            "disabled mod {disabled_id} leaked into output"
        );
    }
}
