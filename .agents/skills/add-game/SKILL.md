---
name: add-game
description: Add support for a new game to modde — walks through the GamePlugin trait, registration, detection, UI wiring, and tests
user_invocable: true
---

# add-game — add a new game to modde

The argument is the game name (e.g. `stellar-blade`, `palworld`). If omitted, ask which game.

## Before you start

Check whether the game runs on a known engine (UE4/UE5, Bethesda Creation Engine, REDengine, etc.). If a data-driven struct already exists for that engine family under `crates/modde-games/src/` (e.g. `bethesda/BethesdaGame`, `ue4/Ue4Game`), use it — the game is just a new `const` instance plus registration. If the engine family isn't supported yet, **extract the engine layer first** before implementing the game. Use `/add-ue4-game` for UE4 titles. Refer to the engine skill if one exists.

If the game doesn't fit an existing engine family and isn't worth extracting one for (one-off layout), create a dedicated module under `crates/modde-games/src/<game>/mod.rs` with a unit struct (see Cyberpunk as reference: `crates/modde-games/src/cyberpunk/mod.rs`).

## Steps

1. **Identify game metadata.** You need:
   - `game_id` — lowercase, kebab-case (e.g. `stellar-blade`)
   - `display_name` — human-readable (e.g. `Stellar Blade`)
   - Steam App ID (check SteamDB or user)
   - Steam dir name under `steamapps/common/`
   - GOG app ID / Epic app name (if applicable)
   - Nexus Mods game domain (if listed)
   - Mod directory layout — where do mods go relative to install root?
   - Archive format extensions (`.pak`, `.bsa`, `.archive`, etc.)
   - Executable directory for proxy DLL detection

2. **Create or extend the game plugin.** Two paths:

   **Engine-family struct exists** (e.g. `Ue4Game`, `BethesdaGame`):
   - Add a new `pub const` instance in the engine's `mod.rs`
   - Add a scanner `const` in the engine's `scanner.rs` if it has one
   - Done. No new module needed.

   **New module** (bespoke game):
   - Create `crates/modde-games/src/<game>/mod.rs` with `pub struct <Game>;` and `pub static <GAME>: <Game> = <Game>;`
   - Implement `GamePlugin` trait — required: `game_id`, `display_name`, `mod_directory`; override defaults as needed
   - Optionally add `scanner.rs` (`ModScanner`), `collision.rs` (`CollisionClassifier`), `saves.rs` (`SaveTracker`)
   - Add `pub mod <game>;` in `crates/modde-games/src/lib.rs`

3. **Register in resolvers** — all in `crates/modde-games/src/lib.rs`:
   - `SUPPORTED_GAME_IDS` array (line ~40)
   - `resolve_game_plugin()` match (line ~66)
   - `resolve_mod_scanner()` match (line ~80) — if scanner exists
   - `resolve_collision_classifier()` match (line ~92) — if classifier exists
   - `resolve_save_tracker()` match — if save tracker exists

4. **Add detection entry** — `crates/modde-games/src/detection.rs`, append to `KNOWN_GAMES` array with steam_app_id, steam_dir, gog_app_id, epic_app_id.

5. **UI reachability** — the game picker is now derived from `modde_games::supported_games()`, so you usually do **not** need to hand-edit `available_games`. What you do need is to verify the new game appears in the picker and that any game-specific UI assumptions still hold.

6. **Add Wabbajack mapping** (if applicable) — `crates/modde-games/src/lib.rs`, `normalize_wabbajack_game()` match.

7. **Write tests** — create `crates/modde-games/tests/<game>_tests.rs`:
   - `test_<game>_game_id`, `test_<game>_display_name`, `test_<game>_mod_directory`
   - `test_<game>_in_supported_ids`, `test_resolve_game_plugin_<game>`
   - Scanner tests with `tempfile::TempDir` if scanner exists
   - Wine override tests if proxy DLL detection is implemented

8. **Compile and test:**
   ```
   nix develop . -c cargo check -p modde-games
   nix develop . -c cargo test -p modde-games
   nix develop . -c cargo check --workspace
   ```

## Gap checklist

After implementation, review and note any of these gaps in your summary:

- [ ] **SaveTracker** — is save tracking not implemented? Note the save dir location for future work.
- [ ] **Truth status** — if save tracking or conflict detection is not fully wired, report it as `Partial` or `Not shipped`. Do not reuse another game's tracker as a placeholder.
- [ ] **CollisionClassifier** — if the game's archive format is opaque, note it.
- [ ] **Nexus domain** — is the game not on Nexus? Note if `nexus_game_domain` returns `None`.
- [ ] **Wabbajack** — is the game not on Wabbajack? Note if no normalization entry.
- [ ] **Installer layouts** — does this game have mod formats the installer pipeline doesn't detect? If so, note and suggest a `/modde-installer` follow-up.
- [ ] **Post-deploy hooks** — does the game need a tool run after deploy (like Cyberpunk's REDmod)? If unimplemented, note it.
- [ ] **Launcher mapping** — check `crates/modde-games/src/launcher.rs` for any game-specific launch args.

## Critical files

- [crates/modde-games/src/lib.rs](crates/modde-games/src/lib.rs) — resolvers + `SUPPORTED_GAME_IDS`
- [crates/modde-games/src/traits.rs](crates/modde-games/src/traits.rs) — `GamePlugin`, `ModScanner`, `SaveTracker` trait defs
- [crates/modde-games/src/detection.rs](crates/modde-games/src/detection.rs) — `KNOWN_GAMES` launcher detection
- [crates/modde-games/src/bethesda/mod.rs](crates/modde-games/src/bethesda/mod.rs) — data-driven reference (engine family)
- [crates/modde-games/src/cyberpunk/mod.rs](crates/modde-games/src/cyberpunk/mod.rs) — bespoke reference (unit struct)
