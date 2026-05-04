---
name: add-ue4-game
description: Add a UE4/UE5 game to modde using the data-driven Ue4Game struct — just metadata, registration, and UI wiring
user_invocable: true
---

# add-ue4-game — add a UE4/UE5 game to modde

The argument is the game name (e.g. `palworld`, `lies-of-p`). If omitted, ask which game.

This skill uses the existing `Ue4Game` data-driven struct. If `crates/modde-games/src/ue4/mod.rs` does not exist yet, **stop** — the generic UE4 engine layer needs to be extracted first. Use `/add-game` with engine extraction guidance. The layer should provide `Ue4Game` (struct implementing `GamePlugin`) and `Ue4Scanner` (struct implementing `ModScanner`), following the `BethesdaGame` / `BethesdaScanner` pattern in `crates/modde-games/src/bethesda/`.

## What you need from the user

- Game name and `game_id` (kebab-case)
- Steam App ID
- Steam directory name (under `steamapps/common/`)
- **UE4 project name** — the folder under install root containing `Content/Paks/` and `Binaries/Win64/`. Common examples: `SB` (Stellar Blade), `Pal` (Palworld), `LOP` (Lies of P). Check the game's install dir or community wikis.
- Nexus Mods domain (if listed)
- GOG / Epic IDs (if applicable)
- OptiScaler compatibility data from `/optiscaler-quirks <game>` if the OptiScaler wiki has a community-tested profile. Do not invent or substitute missing compatibility data.

## Steps

1. **Add the game const** in `crates/modde-games/src/ue4/mod.rs`:
   ```rust
   pub const <GAME>: Ue4Game = Ue4Game::new(
       "<game-id>",
       "<Display Name>",
       "<steam_app_id>",
       "<ProjectName>",
       None, // or Some("<nexus_domain>")
   );
   ```

   If `/optiscaler-quirks <game>` returns verified community data, add an `OptiScalerProfile` entry for the game and extend the `OptiScalerProfiles for Ue4Game` match. Include the source URL, tested version, proxy DLL, optional release tag/asset, Wine overrides, companion-file behavior, INI overrides, and notes. If no verified data exists, do not add a placeholder profile.

2. **Add the scanner const** in `crates/modde-games/src/ue4/scanner.rs`:
   ```rust
   pub static <GAME>_SCANNER: Ue4Scanner = Ue4Scanner {
       game_id: "<game-id>",
       project_name: "<ProjectName>",
   };
   ```

3. **Register** — follow `/add-game` step 3. All four edits in `crates/modde-games/src/lib.rs`:
   - `SUPPORTED_GAME_IDS` — append `"<game-id>"`
   - `resolve_game_plugin()` — `"<game-id>" => Some(&ue4::<GAME>)`
   - `resolve_mod_scanner()` — `"<game-id>" => Some(&ue4::scanner::<GAME>_SCANNER)`
   - No collision classifier (pak format is opaque)

4. **Detection** — append `KnownGame` to `KNOWN_GAMES` in `crates/modde-games/src/detection.rs`.

5. **UI** — the game picker is derived from `modde_games::supported_games()`, so you usually do not need to patch a hard-coded UI list anymore. Verify the new game appears in the picker after registration.

6. **Tests** — add cases to `crates/modde-games/tests/ue4_tests.rs` (preferred) or a new test file:
   - `test_<game>_game_id`, `test_<game>_display_name`, `test_<game>_mod_directory`
   - `test_<game>_in_supported_ids`, `test_resolve_game_plugin_<game>`
   - OptiScaler profile resolver/default tests if community data exists; also preserve a no-profile UE4 game case when applicable
   - Scanner test with tempdir: create `<ProjectName>/Content/Paks/~mods/<Mod>.pak`, assert `DiscoveredMod` with `mod_id = "pak/<Mod>"`

7. **Build + test:**
   ```
   nix develop . -c cargo check -p modde-games
   nix develop . -c cargo test -p modde-games
   nix develop . -c cargo check --workspace
   ```

## UE4 conventions to know

- **Mod directory**: `<ProjectName>/Content/Paks/~mods/` — the tilde forces UE4's pak mounter to load these after base paks
- **LogicMods**: `<ProjectName>/Content/Paks/LogicMods/` — UE4SS blueprint mods (scanned automatically by `Ue4Scanner`)
- **Pak triples**: a mod can be `.pak` alone or `.pak` + `.ucas` + `.utoc` (IoStore). The scanner groups them by stem.
- **Proxy DLLs**: UE4SS installs as `dwmapi.dll` (default) or `xinput1_3.dll` (alternate) in `<ProjectName>/Binaries/Win64/`. Wine overrides are detected automatically by `Ue4Game`.
- **Nested mods**: some mod authors pack their paks inside a subdirectory — the scanner walks one level of subdirs.

## Gap checklist (UE4-specific)

- [ ] **SaveTracker** — UE4 saves typically live at `AppData/Local/<ProjectName>/Saved/SaveGames/`. Not implemented in v1.
- [ ] **CollisionClassifier** — pak format is opaque; no content-level overlap detection.
- [ ] **Post-deploy** — some UE4 games need custom steps (e.g. loose-file cooking). Note if applicable.
- [ ] **UE5 IoStore** — if the game uses `.ucas`/`.utoc` exclusively (no `.pak`), the scanner still picks them up, but verify `stem_for()` handles the exact naming.
- [ ] **Non-standard mod dir** — a few UE4 games use a different paks subdirectory (e.g. `Mods/` instead of `~mods/`). If so, the `Ue4Game` struct may need a new field; note it for follow-up.
- [ ] **Nexus domain** — if `None`, note it.
- [ ] **Installer layouts** — UE4 mods often ship as bare `.pak` files or zips with the pak inside. If the installer pipeline struggles, suggest `/modde-installer`.
- [ ] **OptiScaler profiles** — record whether `/optiscaler-quirks <game>` found a community-tested profile. If yes, include the profile ID and source URL. If no, state that no OptiScaler profile is shipped.

## Critical files

- [crates/modde-games/src/ue4/mod.rs](crates/modde-games/src/ue4/mod.rs) — `Ue4Game` struct, game `const` instances
- [crates/modde-games/src/optiscaler.rs](crates/modde-games/src/optiscaler.rs) — `OptiScalerProfile`, `OptiScalerProfiles`, profile resolver
- [crates/modde-games/src/ue4/scanner.rs](crates/modde-games/src/ue4/scanner.rs) — `Ue4Scanner` struct, scanner instances
- [crates/modde-games/src/lib.rs](crates/modde-games/src/lib.rs) — resolvers + `SUPPORTED_GAME_IDS`
- [crates/modde-games/src/detection.rs](crates/modde-games/src/detection.rs) — `KNOWN_GAMES`
- [crates/modde-games/tests/ue4_tests.rs](crates/modde-games/tests/ue4_tests.rs) — existing UE4 test suite
