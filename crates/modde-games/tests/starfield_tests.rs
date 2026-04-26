use modde_games::bethesda;
use modde_games::{GamePlugin, SUPPORTED_GAME_IDS, resolve_game_plugin, resolve_save_tracker};

#[test]
fn starfield_constant_has_correct_game_id() {
    assert_eq!(bethesda::STARFIELD.game_id(), "starfield");
}

#[test]
fn resolve_game_plugin_returns_some_for_starfield() {
    assert!(
        resolve_game_plugin("starfield").is_some(),
        "resolve_game_plugin should return Some for starfield"
    );
}

#[test]
fn starfield_in_supported_game_ids() {
    assert!(
        SUPPORTED_GAME_IDS.contains(&"starfield"),
        "SUPPORTED_GAME_IDS should contain starfield"
    );
}

#[test]
fn starfield_save_tracker_is_not_exposed_until_implemented() {
    assert!(
        resolve_save_tracker("starfield").is_none(),
        "Starfield save tracking should stay explicitly unsupported until it has a real tracker"
    );
}
