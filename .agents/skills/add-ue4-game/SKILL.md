---
name: add-ue4-game
description: Add a UE4 or UE5 game through modde's data-driven UE game support.
user_invocable: true
---

**Cross-repository work:** As soon as work is known to span more than one Git repository, invoke `$graphify` before further discovery, planning, or edits. Query a relevant existing graph first; build or update a merged graph if none exists, it is stale, or it does not cover every repository in scope. Reuse a current graph already produced for the same repository set.

# add-ue4-game

Use this when the target game is UE4 or UE5 and the shared UE layer exists.

## Steps

1. Confirm `crates/modde-games/src/ue4` exists. If not, extract the shared UE layer first.
2. Add a new data-driven game entry with id, display name, launcher ids, executable path, pak directory, and save location.
3. Register the game and scanner in `crates/modde-games/src/lib.rs`.
4. Add launcher detection metadata.
5. Add tests for metadata, resolution, mod directory, and scanner behavior.

## Validation

Run `nix develop . -c cargo test -p modde-games`.
