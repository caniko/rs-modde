#![allow(clippy::wildcard_imports)]
use super::*;

#[test]
fn add_custom_game_submit_registers_and_selects_game() {
    let _guard = db_lock();
    reset_isolated_db();
    let custom_id = "elden-ring-custom";
    let _ = modde_games::remove_user_game(custom_id);
    modde_games::reload_user_games();

    let install = tempfile::tempdir().expect("install dir");
    let game_dir = install.path().join("Game");
    std::fs::create_dir_all(&game_dir).expect("game dir");
    std::fs::write(game_dir.join("eldenring.exe"), b"exe").expect("write exe");

    let mut app = test_app();
    let _ = app.update(Message::OpenAddCustomGame);
    let _ = app.update(Message::AddCustomGameFieldChanged {
        field: AddCustomGameDraftField::Id,
        value: custom_id.to_string(),
    });
    let _ = app.update(Message::AddCustomGameFieldChanged {
        field: AddCustomGameDraftField::DisplayName,
        value: "ELDEN RING".to_string(),
    });
    let _ = app.update(Message::AddCustomGameInstallPathPicked(
        install.path().to_path_buf(),
    ));
    let _ = app.update(Message::AddCustomGameSubmit);

    assert!(!app.add_custom_game_dialog_open);
    assert_eq!(app.selected_game.as_deref(), Some(custom_id));
    assert_eq!(app.settings.selected_game.as_deref(), Some(custom_id));
    assert_eq!(
        app.settings.game_path(&GameId::from(custom_id)),
        Some(&install.path().to_path_buf())
    );
    assert!(
        app.available_games
            .iter()
            .any(|(id, name)| id == custom_id && name == "ELDEN RING")
    );
    assert!(
        modde_games::supported_games()
            .iter()
            .any(|(id, name)| *id == custom_id && *name == "ELDEN RING")
    );
    let plugin = modde_games::resolve_game_plugin(custom_id).expect("custom game should resolve");
    assert_eq!(
        plugin.executable_dir(install.path()),
        install.path().join("Game")
    );

    modde_games::remove_user_game(custom_id).expect("remove custom game");
    modde_games::reload_user_games();
}

// ─── ReorderMod refusal / allow paths ────────────────────────

#[test]
fn reorder_refused_when_profile_wabbajack_locked() {
    let _guard = db_lock();
    seed_profile(
        "reorder_refuse_wabbajack",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", None),
        ],
        Some(LoadOrderLock::now(LockReason::Wabbajack {
            manifest_hash: "deadbeef".to_string(),
        })),
    );
    let mut app = loaded_test_app("reorder_refuse_wabbajack");
    let _ = app.update(Message::ReorderMod {
        mod_id: "a".to_string(),
        direction: ReorderDirection::Down,
    });
    complete_reorder_write(
        &mut app,
        "reorder_refuse_wabbajack",
        "a",
        ReorderDirection::Down,
    );
    let persisted = reload_seeded("reorder_refuse_wabbajack");
    assert_eq!(
        mod_ids(&persisted),
        vec!["a", "b", "c"],
        "order must be unchanged when profile is Wabbajack-locked"
    );
    assert!(
        app.status_message.contains("locked by Wabbajack"),
        "status message should name the lock reason, got: {}",
        app.status_message
    );
}

#[test]
fn reorder_refused_when_target_mod_pinned() {
    let _guard = db_lock();
    seed_profile(
        "reorder_refuse_target_pinned",
        vec![
            seed_mod("a", None),
            seed_mod("b", Some(LockReason::Manual { note: None })),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("reorder_refuse_target_pinned");
    let _ = app.update(Message::ReorderMod {
        mod_id: "b".to_string(),
        direction: ReorderDirection::Up,
    });
    complete_reorder_write(
        &mut app,
        "reorder_refuse_target_pinned",
        "b",
        ReorderDirection::Up,
    );
    let persisted = reload_seeded("reorder_refuse_target_pinned");
    assert_eq!(mod_ids(&persisted), vec!["a", "b", "c"]);
    assert!(
        app.status_message.contains("pinned"),
        "status message should mention the pin, got: {}",
        app.status_message
    );
}

#[test]
fn reorder_refused_when_swap_partner_pinned() {
    let _guard = db_lock();
    seed_profile(
        "reorder_refuse_partner_pinned",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", Some(LockReason::Manual { note: None })),
        ],
        None,
    );
    let mut app = loaded_test_app("reorder_refuse_partner_pinned");
    // Move b down → swap partner is c (pinned) → should refuse.
    let _ = app.update(Message::ReorderMod {
        mod_id: "b".to_string(),
        direction: ReorderDirection::Down,
    });
    complete_reorder_write(
        &mut app,
        "reorder_refuse_partner_pinned",
        "b",
        ReorderDirection::Down,
    );
    let persisted = reload_seeded("reorder_refuse_partner_pinned");
    assert_eq!(mod_ids(&persisted), vec!["a", "b", "c"]);
    assert!(
        app.status_message.contains("Cannot move past a pinned mod"),
        "status message should explain the adjacent pin, got: {}",
        app.status_message
    );
}

#[test]
fn reorder_allowed_when_unlocked_moves_up() {
    let _guard = db_lock();
    seed_profile(
        "reorder_allow_up",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("reorder_allow_up");
    // Move b up → swap with a → [b, a, c].
    let _ = app.update(Message::ReorderMod {
        mod_id: "b".to_string(),
        direction: ReorderDirection::Up,
    });
    complete_reorder_write(&mut app, "reorder_allow_up", "b", ReorderDirection::Up);
    let persisted = reload_seeded("reorder_allow_up");
    assert_eq!(mod_ids(&persisted), vec!["b", "a", "c"]);
    assert!(
        app.status_message.contains("up"),
        "status should confirm the upward move, got: {}",
        app.status_message
    );
}

#[test]
fn reorder_allowed_when_unlocked_moves_down() {
    let _guard = db_lock();
    seed_profile(
        "reorder_allow_down",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("reorder_allow_down");
    // Move a down → swap with b → [b, a, c].
    let _ = app.update(Message::ReorderMod {
        mod_id: "a".to_string(),
        direction: ReorderDirection::Down,
    });
    complete_reorder_write(&mut app, "reorder_allow_down", "a", ReorderDirection::Down);
    let persisted = reload_seeded("reorder_allow_down");
    assert_eq!(mod_ids(&persisted), vec!["b", "a", "c"]);
    assert!(
        app.status_message.contains("down"),
        "status should confirm the downward move, got: {}",
        app.status_message
    );
}

#[test]
fn reorder_noop_at_top_edge() {
    let _guard = db_lock();
    seed_profile(
        "reorder_noop_edge",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("reorder_noop_edge");
    let status_before = app.status_message.clone();
    let _ = app.update(Message::ReorderMod {
        mod_id: "a".to_string(),
        direction: ReorderDirection::Up,
    });
    complete_reorder_write(&mut app, "reorder_noop_edge", "a", ReorderDirection::Up);
    let persisted = reload_seeded("reorder_noop_edge");
    assert_eq!(
        mod_ids(&persisted),
        vec!["a", "b", "c"],
        "order unchanged at top boundary"
    );
    // The handler silently short-circuits on `AtBoundary` — status
    // message is not rewritten. Contract documented at
    // `Message::ReorderMod` handler.
    assert_eq!(
        app.status_message, status_before,
        "AtBoundary should not mutate the status message"
    );
}

// ─── LockMod / UnlockMod (per-mod pins) ──────────────────────

#[test]
fn lock_mod_sets_per_mod_lock() {
    let _guard = db_lock();
    seed_profile(
        "lock_mod_sets_pin",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("lock_mod_sets_pin");
    let _ = app.update(Message::LockMod {
        mod_id: "b".to_string(),
    });
    complete_lock_write(&mut app, "lock_mod_sets_pin", "b", true);
    let persisted = reload_seeded("lock_mod_sets_pin");
    assert!(
        matches!(
            persisted.mods[1].lock,
            Some(LockReason::Manual { note: None })
        ),
        "mod 'b' should be pinned with Manual reason, got {:?}",
        persisted.mods[1].lock
    );
    // Other mods untouched.
    assert!(persisted.mods[0].lock.is_none());
    assert!(persisted.mods[2].lock.is_none());
}

#[test]
fn unlock_mod_clears_per_mod_lock() {
    let _guard = db_lock();
    seed_profile(
        "unlock_mod_clears_pin",
        vec![
            seed_mod("a", None),
            seed_mod("b", Some(LockReason::Manual { note: None })),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("unlock_mod_clears_pin");
    let _ = app.update(Message::UnlockMod {
        mod_id: "b".to_string(),
    });
    complete_lock_write(&mut app, "unlock_mod_clears_pin", "b", false);
    let persisted = reload_seeded("unlock_mod_clears_pin");
    assert!(
        persisted.mods[1].lock.is_none(),
        "mod 'b' pin should be cleared"
    );
}
