# Save Management

## Overview

modde has a **git-backed save vault** that provides full history, branching, and fingerprint tracking for game saves. This goes significantly beyond MO2's per-profile save isolation — modde tracks the full history of every save, warns about mod mismatches on restore, and auto-captures saves on profile switch and game exit. MO2's save management is comparatively basic (per-profile directory, no history).

## Architecture

### Git-Backed Save Vault

Each profile gets its own git repository for save storage:

```
~/.local/share/modde/saves/<game_id>/<profile_name>/
    .git/           -- full git history
    <save files>    -- current save state
```

### Save Fingerprinting

**Key file:** `crates/modde-core/src/save.rs`

`SaveFingerprint` is a SHA-256 hash of sorted save-breaking mod IDs. It's embedded in git commit trailers:

```
Mod-Fingerprint: a1b2c3d4...
Save-Breaking-Mods: skse,ussep,immersive_armors
```

On restore, if the current mod fingerprint differs from the snapshot's, modde warns:
- `FingerprintCheck::Match` — safe to restore
- `FingerprintCheck::Mismatch { added, removed }` — risk of save corruption

### Save Operations

| CLI Command | Purpose |
|-------------|---------|
| `save assign <path> --profile P` | Link a save to a profile |
| `save unassign <path>` | Remove assignment |
| `save list --profile P` | List assigned saves |
| `save scan --game G` | Find unassigned saves |
| `save adopt --game G --profile P` | Import game directory saves |
| `save capture --game G --profile P [-m msg]` | Create snapshot |
| `save history --game G --profile P` | View snapshot history |
| `save restore --game G --profile P <commit>` | Restore from snapshot |
| `save auto-capture --game G` | Detect new saves (game exit hook) |
| `save watch --game G [--interval 30]` | Poll for changes |

### Profile-Integrated Save Swapping

When switching profiles (`profile switch`, `profile try`, `profile rollback`):
1. Current saves are captured into the outgoing profile's vault
2. Incoming profile's saves are restored to the game save directory
3. Fingerprints are embedded in the capture commit

### Bethesda Save Detection

**Key file:** `crates/modde-games/src/bethesda/saves.rs`

Binary save header parsing:
- Magic bytes: `TESV_SAVEGAME`, `FO4_SAVEGAME`, `FO76_SAVEGAME`
- Extracts: player name, save slot number
- Classifies: `autosave*` -> "auto", `quicksave*` -> "quick", others -> "manual"
- Labels: `"PlayerName -- Save 42"`
- Sorts by modification time (newest first)

### Cyberpunk 2077 Save Detection

**Key file:** `crates/modde-games/src/cyberpunk/saves.rs`

- Patterns: `ManualSave-*`, `AutoSave-*`, `QuickSave-*`, `PointOfNoReturn-*`
- Reads `metadata.9.json` for NamedSaves mod support
- Excludes `user.gls` global settings

### Unadopted Save Detection

On profile activation, if existing saves are found in the game directory with no active profile, `ActivateResult::AdoptionRequired` is returned so the caller can prompt the user before overwriting.

### Key Files

| File | Purpose |
|------|---------|
| `crates/modde-core/src/save.rs` | SaveManager, SaveFingerprint, vault operations |
| `crates/modde-core/src/profile/mod.rs` | Save-aware activation, try, rollback |
| `crates/modde-games/src/bethesda/saves.rs` | Binary save parsing, detection |
| `crates/modde-games/src/cyberpunk/saves.rs` | Cyberpunk save detection |
| `crates/modde-games/src/traits.rs` | SaveTracker trait |
| `crates/modde-cli/src/commands/save.rs` | CLI save commands |

## MO2 Feature Comparison

| MO2 Feature | modde | Notes |
|-------------|-------|-------|
| Per-profile save games | **Done** | Git-backed vaults with full history |
| Save swapping on profile switch | **Done** | Automatic with fingerprint tracking |
| Saves tab in right pane | Partial | MO2 shows save list with metadata; modde has CLI + Iced view |
| Save history/snapshots | **Done** | Full git history per profile — MO2 has no equivalent |
| Save restore from snapshot | **Done** | With mod mismatch warnings — MO2 has no equivalent |
| Auto-capture on game exit | **Done** | `save auto-capture`, `save watch` — MO2 has no equivalent |
| Save file detection | **Done** | Binary header parsing for Bethesda + metadata for Cyberpunk |
| Mod fingerprint warnings | **Done** | SHA-256 fingerprint in git trailers — MO2 has no equivalent |
| Unadopted save detection | **Done** | Prompts before overwriting |
| Save branching (fork) | **Done** | `profile fork` clones the save branch — MO2 has no equivalent |
