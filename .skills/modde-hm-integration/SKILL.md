---
name: modde-hm-integration
description: Add or update modde Home Manager integration, including Wabbajack manual archives.
user_invocable: true
---

**Cross-repository work:** As soon as work is known to span more than one Git repository, invoke `$graphify` before further discovery, planning, or edits. Query a relevant existing graph first; build or update a merged graph if none exists, it is stale, or it does not cover every repository in scope. Reuse a current graph already produced for the same repository set.

# modde-hm-integration

Use this when adding `programs.modde` to Home Manager or updating a modde profile declaratively.

## Steps

1. Locate the user's Home Manager module and determine whether it already imports `modde.homeManagerModules.modde`.
2. Add or update:
   - `programs.modde.enable = true;`
   - `programs.modde.package` if the caller needs a non-default package.
   - `programs.modde.nexus.apiKeyFile` when Nexus or Wabbajack Nexus downloads are required.
   - `programs.modde.profiles.<name>.game`, `gameDir`, and `installMode`.
3. For Wabbajack profiles, configure `wabbajackList.path` or `wabbajackList.url` plus `hash`.
4. If manual archives are missing, run:
   `modde wabbajack missing-impact <MANIFEST> --nix-snippet`
   and paste readable `manualArchives` entries.
5. For optional challenge-gated archives, set `missingArchivePolicy = "omit-mods";` and mark absent archives `optional = true`.
6. Keep archive verification hash-based. Names are labels only.

## Validation

Run:

```bash
nix build .#checks.x86_64-linux.hm-module
home-manager switch --dry-run
```
