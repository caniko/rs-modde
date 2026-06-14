use super::*;
use crate::profile::{EnabledMod, ProfileSource};
use smallvec::{SmallVec, smallvec};
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
                ..Default::default()
            })
            .collect(),
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: rules,
        load_order_lock: None,
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
                ..Default::default()
            },
            EnabledMod {
                mod_id: "mod_b".to_string(),
                enabled: false,
                version: None,
                fomod_config: None,
                ..Default::default()
            },
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };
    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 1);
    assert_eq!(result.order[0], "mod_a");
}

// ── Stability tests ──────────────────────────────────────────
//
// These pin down the "resolve is stable wrt profile.mods input order"
// contract that makes `Message::ReorderMod` visible in the load_order
// view. Before the Kahn's rewrite, `petgraph::toposort` could return
// any valid order — so reordering profile.mods without a rule change
// didn't necessarily shift anything in `resolved_order`.

fn ids(order: &[ModId]) -> Vec<&str> {
    order.iter().map(super::ModId::as_str).collect()
}

#[test]
fn stable_no_rules_preserves_input_order() {
    let profile = make_profile(vec!["c", "a", "b"], smallvec![]);
    let result = resolve(&profile).unwrap();
    assert_eq!(
        ids(&result.order),
        vec!["c", "a", "b"],
        "with no rules, resolver must emit mods in their profile.mods order"
    );
}

#[test]
fn stable_after_swap_round_trips() {
    // Model what `Message::ReorderMod` does: swap two adjacent
    // entries in profile.mods, then re-resolve. The new resolved
    // order must reflect the swap.
    let mut profile = make_profile(vec!["a", "b", "c"], smallvec![]);
    let before = resolve(&profile).unwrap();
    assert_eq!(ids(&before.order), vec!["a", "b", "c"]);

    profile.mods.swap(0, 1); // [b, a, c]
    let after = resolve(&profile).unwrap();
    assert_eq!(ids(&after.order), vec!["b", "a", "c"]);
}

#[test]
fn stable_with_rule_only_preserves_unrelated_neighbors() {
    // [c, b, a] with rule "a must load after c" — the rule is
    // already satisfied (a is after c), so nothing needs to move.
    // Critically, `b` must not drift even though it has no
    // constraints.
    let profile = make_profile(
        vec!["c", "b", "a"],
        smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("a"),
            after: ModId::from("c"),
        }],
    );
    let result = resolve(&profile).unwrap();
    assert_eq!(ids(&result.order), vec!["c", "b", "a"]);
}

#[test]
fn stable_with_rule_forcing_reorder_is_minimal() {
    // [c, b, a] with rule "b must load after a" forces b→after a.
    // The minimal stable fix: emit `c` first (no deps, lowest input
    // pos), then `a` (in_degree becomes 0 once c is emitted — wait,
    // no; a has no incoming edges at all in this graph, its input
    // pos is 2, so after c at 0 we look at the next ready node).
    // Expected: [c, a, b] — a moves up ahead of b to satisfy the
    // rule, c stays at position 0 because nothing constrains it.
    let profile = make_profile(
        vec!["c", "b", "a"],
        smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("b"),
            after: ModId::from("a"),
        }],
    );
    let result = resolve(&profile).unwrap();
    assert_eq!(
        ids(&result.order),
        vec!["c", "a", "b"],
        "c should stay first; a must come before b due to rule"
    );
}

#[test]
fn stable_resolve_is_deterministic() {
    // Guards against HashMap iteration order sneaking in. Resolve
    // the same profile twice and assert identical output. Run with
    // a largeish mod set to give HashMap iteration a chance to
    // scramble things.
    let mods: Vec<&str> = vec![
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        "lambda", "mu", "nu", "xi", "omicron",
    ];
    let profile = make_profile(mods.clone(), smallvec![]);
    let a = resolve(&profile).unwrap();
    let b = resolve(&profile).unwrap();
    assert_eq!(ids(&a.order), ids(&b.order));
    assert_eq!(ids(&a.order), mods);
}

#[test]
fn stable_disabled_mod_in_middle_preserves_others_input_order() {
    // profile.mods = [a, b(disabled), c] — the output should be
    // [a, c], both in input-position order. The old toposort could
    // return [c, a] depending on graph iteration.
    let profile = Profile {
        id: None,
        name: "test".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            EnabledMod {
                mod_id: "a".to_string(),
                enabled: true,
                ..Default::default()
            },
            EnabledMod {
                mod_id: "b".to_string(),
                enabled: false,
                ..Default::default()
            },
            EnabledMod {
                mod_id: "c".to_string(),
                enabled: true,
                ..Default::default()
            },
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };
    let result = resolve(&profile).unwrap();
    assert_eq!(ids(&result.order), vec!["a", "c"]);
}
