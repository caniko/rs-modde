---
name: modde-installer
description: Triage an unknown-install-type mod dossier and extend modde's installer pipeline to recognize the layout. Invoke as `/modde-installer [<slug>]`.
user_invocable: true
---

# modde-installer — extend the installer for an unknown mod layout

When modde's install pipeline cannot classify a mod's archive structure, it writes a dossier to `$XDG_DATA_HOME/modde/unknown-installers/<slug>/` and surfaces a "Send to Claude" button in the UI. This skill consumes those dossiers and lands the missing detection code.

The argument (optional) is the dossier slug — usually `<game_domain>_<mod_id>_<file_id>`. Without an argument, list the pending dossiers and ask the user which one to work on.

## Steps

1. **Locate dossiers.** Run `ls -la $XDG_DATA_HOME/modde/unknown-installers/` (fall back to `~/.local/share/modde/unknown-installers/` if `XDG_DATA_HOME` is unset). Each subdirectory is one mod.

2. **Pick one.**
   - If the user gave a slug, match it literally or as a prefix.
   - If not, list the dossier names and ask which one.
   - Skip directories whose name ends in `.resolved` — those have already been handled.

3. **Read the dossier inputs** inside the chosen dir:
   - `metadata.json` — mod name, author, game, Nexus URL
   - `archive_tree.txt` — recursive listing of the extracted archive
   - `file_samples/` — up to 5 small text files extracted verbatim (READMEs, `info.json`, `ModuleConfig.xml`, etc.)
   - `analyzer_trace.json` — which generic probes the analyzer already tried
   - `PROMPT.md` — a primed prompt already written by modde. **Read this too** — it captures context at dump time.

4. **Decide where the fix belongs.** Two places to extend:

   - **Generic layout** (e.g. a new package format that any game might ship):
     add a variant to `InstallMethod` in `crates/modde-core/src/installer/types.rs` and a detection branch in `crates/modde-core/src/installer/analyze.rs::detect_method`.
     Add an execute arm in `crates/modde-core/src/installer/execute.rs` if the new variant needs custom staging.

   - **Game-specific layout** (e.g. a weird Cyberpunk or Bethesda convention):
     extend the relevant game plugin's `analyze_mod_archive` in
     `crates/modde-games/src/<game>/mod.rs`. Use `crate::traits::InstallMethod` — no new variant needed if an existing one fits.

   Prefer the game-specific hook when the layout is tied to one game. Use the generic path only when you'd expect other games to benefit.

5. **Write a unit test** using file samples from this dossier. The generic tests live in `crates/modde-core/src/installer/analyze.rs` under `#[cfg(test)] mod tests`. Use `tempfile::tempdir()` + the `touch()` helper already there.

6. **Compile and test:**
   ```
   cargo test -p modde-core installer::
   cargo check -p modde-games
   ```
   Both must pass.

7. **Mark the dossier as resolved** so the UI surfaces a **Retry Install** button:
   ```
   mv "$XDG_DATA_HOME/modde/unknown-installers/<slug>" "$XDG_DATA_HOME/modde/unknown-installers/<slug>.resolved"
   ```

8. **Summarize** the change for the user in 3-5 lines:
   - which file you edited
   - which variant / branch you added
   - which test you wrote
   - the cargo test output
   - "retry via `modde mod install` or the **Retry Install** button"

## Critical files

- [crates/modde-core/src/installer/types.rs](crates/modde-core/src/installer/types.rs) — `InstallMethod`, `InstallPlan`, `StagedFile`, `InstallerError`
- [crates/modde-core/src/installer/analyze.rs](crates/modde-core/src/installer/analyze.rs) — detection pipeline and existing probes
- [crates/modde-core/src/installer/execute.rs](crates/modde-core/src/installer/execute.rs) — stages files into the store per method
- [crates/modde-core/src/installer/probe.rs](crates/modde-core/src/installer/probe.rs) — `InstallProbe` closures bridging game plugins → analyzer
- [crates/modde-games/src/traits.rs](crates/modde-games/src/traits.rs) — `GamePlugin::analyze_mod_archive` + `recognizes_bare_layout` hooks
- [crates/modde-games/src/cyberpunk/mod.rs](crates/modde-games/src/cyberpunk/mod.rs) — reference impl for REDmod detection
- [crates/modde-games/src/bethesda/mod.rs](crates/modde-games/src/bethesda/mod.rs) — reference impl for Bethesda bare layouts

## Guidelines

- **Don't overfit.** If the dossier shows `blob.bin` and nothing else, don't add an `InstallMethod::Blob` variant — the user's archive is probably just mispackaged. Ask the user to confirm the mod actually works before landing code.
- **Don't bypass the framework.** The point of the installer pipeline is that uninstall knows which files belong to which mod. Any new variant must end up routing through `execute::stage_tree` (or an equivalent) so `StagedFile` rows are produced.
- **Leave script-merge support alone.** `InstallMethod::ScriptMerge` exists as a hook for a future feature. Don't execute merges yet — just use it to tag files that participate in a group.
- **Preserve fallbacks.** When adding a game-specific rule, check that the existing `recognizes_bare_layout` still fires for the mods it already handled. A dossier fix should be additive.
