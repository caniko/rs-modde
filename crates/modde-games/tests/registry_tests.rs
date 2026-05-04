use std::collections::HashSet;

use modde_games::{SUPPORTED_GAME_IDS, all_games, normalize_wabbajack_game};

#[test]
fn registry_game_ids_are_unique() {
    let ids: Vec<_> = all_games().iter().map(|game| game.game_id).collect();
    let unique: HashSet<_> = ids.iter().copied().collect();

    assert_eq!(ids.len(), unique.len(), "registry has duplicate game IDs");
}

#[test]
fn supported_game_ids_are_registry_order() {
    let registry_ids: Vec<_> = all_games().iter().map(|game| game.game_id).collect();

    assert_eq!(SUPPORTED_GAME_IDS, registry_ids.as_slice());
}

#[test]
fn registry_resolves_public_components_consistently() {
    for game in all_games() {
        let plugin = modde_games::resolve_game_plugin(game.game_id).unwrap();
        assert_eq!(plugin.game_id(), game.game_id);
        assert_eq!(plugin.display_name(), game.display_name);
        assert_eq!(
            modde_games::resolve_mod_scanner(game.game_id).is_some(),
            game.scanner.is_some(),
            "{} scanner mismatch",
            game.game_id
        );
        assert_eq!(
            modde_games::resolve_save_tracker(game.game_id).is_some(),
            game.save_tracker.is_some(),
            "{} save tracker mismatch",
            game.game_id
        );
        assert_eq!(
            modde_games::resolve_collision_classifier(game.game_id).is_some(),
            game.collision_classifier.is_some(),
            "{} collision classifier mismatch",
            game.game_id
        );
        assert_eq!(
            modde_games::supports_save_profiles(game.game_id),
            game.supports_save_profiles,
            "{} save support mismatch",
            game.game_id
        );
    }
}

#[test]
fn registry_preserves_known_nexus_metadata() {
    let expected = [
        ("skyrim-se", Some("skyrimspecialedition"), Some(1704)),
        ("skyrim-ae", Some("skyrimspecialedition"), Some(1704)),
        ("fallout4", Some("fallout4"), Some(1151)),
        ("fallout76", Some("fallout76"), Some(2590)),
        ("starfield", Some("starfield"), Some(4187)),
        ("cyberpunk2077", Some("cyberpunk2077"), Some(3333)),
        ("stellar-blade", Some("stellarblade"), None),
    ];

    for (game_id, domain, numeric_id) in expected {
        let game = modde_games::resolve_game(game_id).unwrap();
        assert_eq!(game.nexus_domain, domain, "{game_id} Nexus domain");
        assert_eq!(game.nexus_game_id, numeric_id, "{game_id} Nexus numeric ID");
    }
}

#[test]
fn registry_preserves_wabbajack_normalization() {
    assert_eq!(
        normalize_wabbajack_game("Cyberpunk2077"),
        Some("cyberpunk2077")
    );
    assert_eq!(
        normalize_wabbajack_game("SkyrimSpecialEdition"),
        Some("skyrim-se")
    );
    assert_eq!(normalize_wabbajack_game("SkyrimAE"), Some("skyrim-ae"));
    assert_eq!(normalize_wabbajack_game("Fallout4"), Some("fallout4"));
    assert_eq!(normalize_wabbajack_game("Fallout76"), Some("fallout76"));
    assert_eq!(normalize_wabbajack_game("Starfield"), Some("starfield"));
    assert_eq!(
        normalize_wabbajack_game("StellarBlade"),
        Some("stellar-blade")
    );
}
