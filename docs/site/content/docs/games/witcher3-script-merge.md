+++
title = "Witcher 3 Script Merge"
description = "Configuring a vanilla scripts cache for Witcher 3 merge bases"
weight = 20
+++

# Witcher 3 Script Merge

modde can use a user-provided Witcher 3 vanilla scripts/config cache as the
base side for 3-way merges. modde does not extract `.bundle` archives.
Build the cache with REDkit, `wcc_lite`, Script Merger's vanilla cache, or
another tool that produces the same unpacked directory layout.

The cache directory should contain paths like:

```text
content/scripts/game/r4Player.ws
bin/config/r4game/user_config_matrix/pc/*.xml
```

Register the cache with:

```sh
modde merge witcher3 set-vanilla /path/to/witcher3-vanilla-cache
```

modde canonicalises the path and validates the canary file
`content/scripts/game/r4Player.ws` before storing it. To inspect the current
setting:

```sh
modde merge witcher3 show-vanilla
```

Script Merger is available on Nexus Mods as mod 484 and remains a useful
reference for vanilla cache creation. For supported merge sessions, modde's
`modde merge` workflow supersedes launching the legacy tool directly:
<https://www.nexusmods.com/witcher3/mods/484>.

When opening a merge in VS Code, modde also writes context files for Claude
Code, Codex, and GitHub Copilot Chat. See
[AI-Assisted Merging](/docs/guides/ai-assisted-merging/) for that workflow.
For the full command flow, see
[Resolving Mod Conflicts](/docs/guides/resolving-mod-conflicts/).
