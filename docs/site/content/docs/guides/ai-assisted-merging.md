+++
title = "AI-Assisted Merging"
description = "Use VS Code AI extensions with modde merge sessions"
weight = 46
+++

# AI-Assisted Merging

modde can prepare VS Code merge sessions with context files for common AI
extensions. It does not start an agent, call an AI API, or read agent output.
Your VS Code extension owns its own sign-in, API keys, token usage, and costs.

## Before you start

Install at least one of these VS Code extensions:

- Claude Code, which reads `CLAUDE.md`.
- Codex, which reads `AGENTS.md`.
- GitHub Copilot Chat, which reads `.github/copilot-instructions.md`.

VS Code also sees recommendations in `.vscode/extensions.json` when a merge
session opens.

## Open a merge session

```sh
modde merge list --profile my-profile --game witcher3
modde merge open <merge_group> --with vscode --profile my-profile --game witcher3
```

The session directory contains `left.txt`, `right.txt`, `base.txt`,
`result.txt`, optional `participants/`, and the AI context files. The AI-facing
files all contain the same merge context: participating mods, relative path,
file syntax, vanilla-base state, and invariants to preserve.

## Ask for help

Open the extension's chat panel inside VS Code and ask it to help merge the
file. For example:

```text
Help me merge this mod conflict. Edit result.txt only and keep the file valid.
```

The prepared context tells the assistant to edit only `result.txt`. Save the
file when you are done.

## Validate from VS Code

Run the `Validate merge result` task from VS Code. It executes:

```sh
modde merge validate <merge_group>
```

The command validates `result.txt` with the same lightweight checks modde uses
when the VS Code merge editor closes: XML must parse, JSON must parse,
WitcherScript-style text must have balanced braces, and other text must be
non-empty.

The task runs `modde` from your `PATH`. If you have both a packaged build and a
locally installed build, your shell path decides which binary VS Code runs.

## Finish

Close the merge editor. modde validates `result.txt` again and publishes it into
the profile's merged-mod overlay when the result is usable.

For Witcher 3 vanilla-base setup, see
[Witcher 3 Script Merge](/docs/games/witcher3-script-merge/).
For the full merge workflow, see
[Resolving Mod Conflicts](/docs/guides/resolving-mod-conflicts/).
