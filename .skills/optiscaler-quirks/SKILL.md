---
name: optiscaler-quirks
description: Research OptiScaler compatibility quirks and encode verified profile data.
user_invocable: true
---

**Cross-repository work:** As soon as work is known to span more than one Git repository, invoke `$graphify` before further discovery, planning, or edits. Query a relevant existing graph first; build or update a merged graph if none exists, it is stale, or it does not cover every repository in scope. Reuse a current graph already produced for the same repository set.

# optiscaler-quirks

Use this before adding or changing OptiScaler support for a game.

## Steps

1. Check the upstream OptiScaler documentation or compatibility notes.
2. Record tested OptiScaler version, proxy DLL, required Wine overrides, companion files, INI settings, and caveats.
3. Do not invent compatibility data when no reliable source exists.
4. Encode verified profiles in the relevant game plugin or shared OptiScaler resolver.
5. Add tests for profile resolution and generated tool settings.

## Validation

Run `nix develop . -c cargo test -p modde-games optiscaler`.
