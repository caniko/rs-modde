+++
title = "Resolving Mod Conflicts"
description = "Resolve mergeable file conflicts with modde merge sessions"
weight = 44
+++

# Resolving Mod Conflicts

modde resolves normal file conflicts by load-order priority: the later enabled
mod wins. Some text conflicts are mergeable instead of simple winner-takes-all
overwrites. For those files, modde creates a merge session and publishes the
resolved result into the profile's synthetic merged-mod overlay.

## Mergeable conflicts

A mergeable conflict is a file-level conflict where modde knows how to prepare
plain-text merge inputs and validate the result. Witcher 3 script and config
files are the first shipped target. XML, JSON, and WitcherScript-style text get
syntax checks before the result is accepted.

Binary assets, archives, and plugin formats that need record-level tooling are
not merged by this workflow yet. For those, load order, file hiding, or a
game-specific plugin tool remains the right fix.

## Find sessions

```sh
modde merge list --profile my-profile --game witcher3
modde merge drivers
```

`merge list` prints pending merge groups for the selected profile. `merge
drivers` shows available backends such as VS Code, Meld, KDiff3, and inline
manual editing.

## Open a merge

```sh
modde merge open <merge_group> --with vscode --profile my-profile --game witcher3
```

The merge directory contains:

- `left.txt` - the lower-priority side.
- `right.txt` - the higher-priority side.
- `base.txt` - the vanilla base when available, or a synthetic marker.
- `result.txt` - the file to edit and save.
- `participants/` - additional mod inputs when more than two mods touch the
  same file.

If `--with` is omitted, modde picks the first available driver. Use `--with
vscode`, `--with meld`, `--with kdiff3`, or `--with inline` to force a driver.

## Validate the result

```sh
modde merge validate <merge_group> --profile my-profile --game witcher3
```

Validation reads `result.txt` from the merge session directory. It prints `OK`
on success and exits non-zero on syntax errors or an empty result. VS Code merge
sessions also include a `Validate merge result` task that runs the same command.

## Accept the winner

If the higher-priority mod should simply win:

```sh
modde merge accept-winner <merge_group> --profile my-profile --game witcher3
```

This writes the current load-order winner into `result.txt` and records the
session result without opening an editor.

## Witcher 3 vanilla base

Register a vanilla scripts/config cache before doing Witcher 3 merges:

```sh
modde merge witcher3 set-vanilla /path/to/witcher3-vanilla-cache
modde merge witcher3 show-vanilla
```

See [Witcher 3 Script Merge](/docs/games/witcher3-script-merge/) for the cache
layout and setup details.

## AI assistance in VS Code

VS Code merge sessions include context files for Claude Code, Codex, and GitHub
Copilot Chat. modde writes those files but never starts an agent, calls an AI
API, or reads agent output. Open the extension's chat panel yourself and ask it
to help merge `result.txt`.

See [AI-Assisted Merging](/docs/guides/ai-assisted-merging/) for the extension
filenames and validation task.
