---
name: modde-installer
description: Triage an unknown-install-type dossier and extend modde's installer detection.
user_invocable: true
---

# modde-installer

Use this when modde writes an unknown installer dossier under the active data dir.

## Steps

1. Locate `unknown-installers/<slug>` under `$MODDE_DATA_DIR` or `$XDG_DATA_HOME/modde`.
2. Read `metadata.json`, `archive_tree.txt`, `analyzer_trace.json`, `PROMPT.md`, and small samples.
3. Decide whether the fix is generic installer detection or game-specific archive analysis.
4. Add the narrowest detection rule that covers the dossier without overfitting.
5. Add tests using the dossier shape.
6. Mark the dossier resolved only after validation passes.

## Validation

Run:

```bash
nix develop . -c cargo test -p modde-core installer::
nix develop . -c cargo check -p modde-games
```
