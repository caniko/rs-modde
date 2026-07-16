---
name: wabbajack-readiness
description: Assess and prepare a Wabbajack install before a large unattended run.
user_invocable: true
---

**Cross-repository work:** If scope spans repositories, invoke `$graphify` before discovery, planning, or edits. Query an existing graph; build/update a merged graph when missing, stale, or incomplete. Reuse a current graph for the same repository set.

# wabbajack-readiness

Use this before running a large Wabbajack install.

## Steps

1. Run `modde wabbajack assess <MANIFEST> --game-dir <GAME_DIR>`.
2. Run `modde wabbajack missing-impact <MANIFEST>` and resolve required manual/Nexus archives.
3. Import user-provided archives with `modde wabbajack import-archive <MANIFEST> <FILE...>`.
4. For optional manual archives, document impact and choose `--missing-archive-policy omit-mods`.
5. For a smoke run, prefer `--no-deploy --continue-on-error --skip-validate --diagnostics-dir <DIR>`.
6. Inspect diagnostics with `modde wabbajack analyze-diagnostics <DIR>`.

## Rule

Do not fabricate missing archives or accept extracted staging leftovers as substitutes.
