# Architecture decisions

Enduring rationale for non-obvious choices in rs-modde. New contributors read this once; everyone else consults it when something looks odd and they want to know why before changing it.

Each entry: the decision, why it was made, and the conditions under which it should be revisited. Implementation history lives in `git log`.

---

## Release tooling: `cargo xtask release` → `simit release`

**Decision.** Releases go through `cargo xtask release {patch|minor|major|prerelease} -m "<message>"`. The xtask wrapper delegates to `simit release` from the devShell.

**Why.** simit standardizes semver bumps, CHANGELOG promotion, commit, and tag across the maintainer's Rust projects. Centralizing that flow prevents per-project drift. The xtask binary owns rs-modde-specific bindings (RPM spec rewrite, COPR vendor tarball) that simit does not model.

**Conventions.**

- Tags are bare semver: `0.2.0`, `1.0.0-rc.1`. No `v` prefix. `.forgejo/workflows/release.yml` triggers on `[0-9]*`.
- Keep `## [Unreleased]` in `CHANGELOG.md` exactly as-is — simit's promotion logic looks for that literal heading.
- Major versions are cut whenever they make sense per SemVer; there is no 1.0 milestone and no "save up breaking changes" policy.

**Revisit when.** simit grows an exact-version release API (currently bump-kind only) that matches `cargo xtask release {version}`'s call shape. At that point the xtask wrapper can shrink.

---

## CI and flake stay bespoke — no `simit init-ci --check` / `init-flake --check`

**Decision.** rs-modde does **not** run `simit init-ci --check` or `simit init-flake --check`. The Forgejo workflows and `flake.nix` are intentionally hand-maintained.

**Why.**

- The simit CI generator emits a generic `ci.yaml` / `publish-crate.yaml` that does not model: `.#flatpak-manifest`, `.#appimage-*`, `.#modde-windows`, `.#docs` builds; the Attic closure push; `nix flake check --keep-going --print-build-logs`; the self-hosted `atlas` runner; or `cargo xtask coverage --ci`. Running `--check` would report all of that intentional customization as drift.
- The flake is rs-harbor-driven and wires cross-compilation (Windows, aarch64-linux, macOS x86_64/aarch64 via osxcross), the Zola `website`/`docs` outputs, Flatpak/AppImage packaging, and a simit-pinned devShell. `init-flake --check` would treat the heavily-customized flake as drift from a vanilla crane template.

**Revisit when.** Any one of:

- simit gains enough configurability to express the extra jobs, Attic push, coverage gate, and `atlas` runner without local patching.
- rs-harbor publishes an `mkSimitCi` helper (or equivalent) that lets simit generate CI which preserves the current requirements.
- rs-modde deliberately simplifies its CI/flake so the simit defaults become the intended source of truth.

---

## xtask: `harbor-xtask` library + per-project thin binary

**Decision.** Reusable build-tooling logic lives in `harbor-xtask` (a library crate in [rs-harbor](https://codeberg.org/caniko/rs-harbor) at `crates/harbor-xtask/`). Each consuming project ships a thin binary that wires project-specific bindings. rs-modde's binary is `crates/modde-xtask/`, invoked via `cargo xtask`.

The `rs-harbor` CLI does **not** gain top-level `release` / `copr` / `docs` subcommands — those would couple the shared binary to downstream project layouts.

**Why.**

- Project-specific bindings (RPM spec path `modde.spec`; workspace crate list; Zola roots `docs/site` and `website`; Nix package names `modde`/`site`/`modde-windows`/`appimage-*`/`flatpak-manifest`; COPR source archive URL) cannot live in a shared CLI without polluting its compatibility surface.
- The library API is synchronous (`std::process::Command` shell-outs; `anyhow::Result`). No async runtime introduced; matches existing rs-harbor style.

**Release backend.** `cargo xtask release` shells to `simit release` (see `crates/modde-xtask/src/main.rs`). simit owns the version bump, CHANGELOG promotion, commit, and tag. The RPM spec `Version:` rewrite happens in CI (`.forgejo/workflows/release.yml`) after the tag fires, against the tagged tree — not committed back to trunk. The earlier "v0.1 uses cargo-release" position from the xtask design was reversed once simit's release flow matured.

**What stays project-specific (don't lift to `harbor-xtask`).** `cargo xtask gui` (binds `modde-ui`); anything that depends on `modde.spec` paths, the rs-modde COPR archive URL, or the rs-modde Nix package names.

**Revisit when.** A second consumer (bikipy is the planned pilot, SynDB follow-up) is wired and surfaces a missing-capability gap.

---

## Flatpak app ID: `com.tartanoglu.modde`

**Decision.** The Flatpak (and AppStream metainfo) app ID is `com.tartanoglu.modde` — reverse-DNS of `tartanoglu.com`, a domain owned by the project author.

**Why.** Flathub reserves provider-owned prefixes; the maintainer-owned domain keeps the application identity portable across forges. The Codeberg-namespace alternative (`page.codeberg.caniko.rs-modde` or similar) would tie identity to the forge host, which would need to change if the project ever migrates.

**Where it's encoded.** `dist/com.tartanoglu.modde.metainfo.xml`, `dist/modde-ui.desktop`, the Flatpak manifest output in `flake.nix`.

---

## Home Manager `programs.modde.profiles.<name>.tools` contract

**Decision.** The `tools` option is `attrsOf toolSubmodule` with `enable`, free-form `settings`, reserved `release`, and `applyOnActivation` flags. Only `gamemode`, `vkbasalt`, and `reshade` carry strict per-setting typing; `mangohud`, `optiscaler`, and `proton` remain `attrsOf anything`.

**Why.** `ToolConfig.settings` is stored as a JSON blob in SQLite — the type-safety contract is at evaluation time, not at storage. Strict typing for tools with sprawling or contextual settings (114 mangohud knobs, 33 proton knobs, OptiScaler's dynamic per-game fields) buys little and costs schema churn.

**Activation contract.**

- Tool activation runs **after** `modde install` / `modde deploy`. Install/deploy is the prerequisite surface; tool writes land last.
- For each enabled tool: HM unconditionally calls idempotent `modde tool enable`, then `modde tool configure` when `settings` is non-empty.
- `modde tool apply` runs only when `applyOnActivation = true`. It is repeatable but mutates the game directory and rewrites the applied-files manifest.
- Disabled tools call `modde tool disable` and skip configure/apply.
- Every `modde tool` non-zero exit **warns and continues** — activation does not fail on a single tool error. Nix-time assertion failures remain fatal.

**Release pinning.** HM-managed tool releases are eager Nix fetches with pinned hashes (`{ url, hash, tag, asset }`). Activation must **not** call networked `modde tool install-release` — Nix fetches the asset, then hands modde a local path.

---

## Mod-merge framework: VS Code as canonical UI, no AI-agent shellout

**Decision.** rs-modde detects content-mergeable mod conflicts and drives **VS Code's built-in 3-way merge editor** (`code --merge <left> <right> <base> <result>`) as the canonical merge UI. Meld, KDiff3, and a plain-Inline fallback are registered as alternates discovered via `which::which`. **modde does not spawn any AI agent itself** — no `claude -p`, no `codex`, no LLM HTTP calls. When the VS Code driver is selected, modde writes `CLAUDE.md`, `AGENTS.md`, `.github/copilot-instructions.md`, `MERGE.md`, and a `.vscode/{tasks,extensions}.json` pair into the session directory. The user invokes their installed Claude Code / Codex / GitHub Copilot extension from inside the merge editor; the extension reads the staged context files automatically.

**Why.** modde stays free of LLM credentials, tool-allowlist surface, and per-request costs. Users pay for and configure their own agent via their existing IDE extension. AI assistance is opt-in per merge by virtue of the user clicking the chat panel — never automatic. The three context-file conventions (Anthropic's `CLAUDE.md`, the `AGENTS.md` convention used by Codex and other agentic tools, GitHub's `.github/copilot-instructions.md`) all carry the same rendered body; the difference is filename, not content.

**Invariants worth preserving.**

- The synthetic mod that owns merged outputs is `__merged__` (mirrors the pre-existing `__overwrite__` reserved id in `executable_configs.output_mod`). `ProfileManager::add_mod` refuses to claim it; `ProfileManager::remove_mod` refuses to delete it.
- `__merged__` ranks above every real mod for paths it provides, and **only** for those paths — the resolver registers it through `inject_merged_mod` per-rel-path, not as a blanket high-priority mod set.
- Per-game merge metadata lives on `GamePlugin` (`mergeable(&str) -> Option<MergeKind>`, `vanilla_base(install, rel_path) -> Option<PathBuf>`). Witcher 3 is implemented; Bethesda / BG3 / Cyberpunk currently return `None` or `Some(BethesdaPlugin)` without an active backend.
- The Witcher 3 vanilla-base cache is **user-pointed** via `modde merge witcher3 set-vanilla <dir>`. No in-tree `.bundle` extractor exists; without a cache the merger drops to 2-way mode with a `# no vanilla base available` marker file.
- Agent-context files are **never** written by Meld / KDiff3 / Inline drivers. They are VS-Code-specific.
- Per-session validation runs the same syntactic check inside VS Code (via the `.vscode/tasks.json` "Validate merge result" task shelling to `modde merge validate <group>`) that modde runs on editor close. The two paths share `merge::validation`.

**Where it's encoded.** `crates/modde-core/src/merge/` (data model, drivers, agent_context, validation, vanilla); `crates/modde-games/src/witcher3/` (`mergeable`, `vanilla_base`); `crates/modde-core/src/resolver/mod.rs` (`inject_merged_mod`); `crates/modde-cli/src/commands/merge.rs` (`list / open / accept-winner / validate / drivers` and the Witcher 3 vanilla subcommands); `crates/modde-ui/src/views/{mod_details,merges,merge_badge}.rs` (Conflicts tab + Merges panel).

**Revisit when.** Bethesda record-level merging (ESP/ESM) becomes in scope — it is not a text merge and will need either xEdit-script delegation or an in-tree record-level engine. The `MergeKind::BethesdaPlugin` variant exists as a reserved slot for that work.

---

## Removed planning directory

The `docs/planning/` directory was removed once the work it tracked landed in trunk. The DECISION.md files have been distilled into this document; the per-phase implementation narratives are recoverable from `git log` if needed.
