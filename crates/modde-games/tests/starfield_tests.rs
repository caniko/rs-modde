use modde_games::bethesda;
use modde_games::{resolve_game_plugin, GamePlugin, SUPPORTED_GAME_IDS};

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
