---
name: wabbajack-readiness
description: Assess and prepare a Wabbajack install before a large unattended run.
user_invocable: true
---

**Cross-repository work:** As soon as work is known to span more than one Git repository, invoke `$graphify` before further discovery, planning, or edits. Query a relevant existing graph first; build or update a merged graph if none exists, it is stale, or it does not cover every repository in scope. Reuse a current graph already produced for the same repository set.

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
