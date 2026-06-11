---
name: add-game
description: Add support for a new game to modde.
user_invocable: true
---

# add-game

Use this when adding a new supported game.

## Steps

1. Identify game id, display name, launcher ids, install directory names, Nexus domain, mod directory, executable directory, archive extensions, and save location.
2. Prefer an existing engine family such as Bethesda or UE4/UE5. If the game is UE4/UE5, use `add-ue4-game`.
3. Implement the game plugin in `crates/modde-games`, then register it in the resolver and supported game list.
4. Add launcher detection metadata.
5. Add Wabbajack normalization if the game appears in Wabbajack manifests.
6. Add tests for resolution, metadata, scanner behavior, save tracking, and OptiScaler profiles when available.

## Validation

Run:

```bash
nix develop . -c cargo test -p modde-games
nix develop . -c cargo check --workspace
```
