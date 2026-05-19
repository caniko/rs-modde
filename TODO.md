# rs-modde — Remaining Work

Living checklist of feature, test, and infrastructure gaps. Tick items as they land; add notes/PR links inline.

---

## 1. Feature gaps

### HM module tool coverage
- [x] Phase 01 decision: hybrid `profiles.<name>.tools` shape, tool activation after `modde deploy`, and eager Nix-pinned release assets. See `docs/planning/hm-module-tool-coverage`.

### 1.1 modde-cli
- [x] Executable management — named executables registry (`executable_configs` table; upsert on `(game_id, name)`)
- [x] Executable management — per-executable arguments (`arguments_json` column)
- [x] Executable management — per-executable output / overwrite mod (`output_mod` column, defaults to `__overwrite__`)
- [x] Executable management — working directory + env overrides (`working_dir`, `environment_json`, `wine_dll_overrides`)
- [x] `modde exec run <name>` command + listing/edit subcommands (`crates/modde-cli/tests/cli_exec.rs` round-trips add/list/remove/run; add doubles as edit via UPSERT)
- [x] `modde skill` subcommand for installing agent skills (`crates/modde-cli/src/commands/skill.rs`)
- [ ] Multi-instance selection flag (`--instance`) wired through every command
- [ ] Instance create / clone / delete / list subcommands (MO2-portable layout)
- [~] End-to-end coverage for `install`, `nexus`, `nxm`, `update`, `scan` (see §2.4)

### 1.2 modde-core
- [ ] Expose `GenericGame` trait to users (config-driven game definition)
- [ ] `modde game add` flow for arbitrary games via TOML/JSON spec
- [ ] Merged-VFS browser — archive content visibility
- [ ] Merged-VFS browser — hidden-file filters
- [ ] Merged-VFS browser — origin tagging (which mod provides each path)
- [ ] Concurrent-deploy stress / error-injection tests (see §2.5)

### 1.3 modde-games
- [ ] Baldur's Gate 3 — finish plugin (PAK load order, modsettings.lsx)
- [ ] Stardew Valley — finish SMAPI mod handling
- [ ] Fallout: New Vegas — parity with FO4/Skyrim plugin handling
- [ ] Oblivion — parity (incl. Oblivion Remastered if scoped)
- [ ] Fallout 76 — finish save tracking (server-side reconciliation)
- [ ] BAIN — user sub-package selection UI (CLI prompt + GUI dialog)
- [ ] BAIN — persisted user choices per mod
- [ ] Tests for generic UE4 game path (currently under-covered)

### 1.4 modde-sources
- [x] Per-archive batched apply (INSTALL_PIPELINE_REWORK Phase 1)
- [x] Native Rust decompression (zip / 7z / RAR / BSA / BA2) via `decompress` (INSTALL_PIPELINE_REWORK Phase 2)
- [x] Streaming I/O for large outputs (INSTALL_PIPELINE_REWORK Phase 3)
- [x] InlineFile zip index via `InlineSource` (INSTALL_PIPELINE_REWORK Phase 4)
- [x] Hardlink/reflink-aware deploy + Stock Game via `link_or_copy` (INSTALL_PIPELINE_REWORK Phase 5)
- [x] Bounded LRU patch-source cache via `ByteLruCache` (INSTALL_PIPELINE_REWORK Phase 6)
- [x] zstd-recompressed staging (INSTALL_PIPELINE_REWORK Phase 7a/7c)
- [ ] CAS chunk-dedup store (INSTALL_PIPELINE_REWORK Phase 7b — deferred because the zstd staging tier landed first)
- [x] Durable resumable apply/download-side archive reuse — store entries survive process restarts and are re-verified before trust (INSTALL_PIPELINE_REWORK Phase 9)
- [x] Resume across process restarts for Wabbajack apply/import using persisted archive store state (INSTALL_PIPELINE_REWORK Phase 9)
- [ ] Pause/cancel surfaced through backend trait, not just UI state (transport API still needs explicit pause/cancel hooks)
- [x] MediaFire and manual-archive download sources (`manual`, `mediafire`, `wabbajack::acquire`)
- [ ] BAIN installer — user-input flow integration with §1.3
- [x] Integrity verification surfaced in CLI (`modde verify`; Phase 8 streaming verify)

### 1.5 modde-ui
- [ ] Mod info dialog — file tree tab
- [ ] Mod info dialog — image preview tab
- [ ] Mod info dialog — conflicts tab (winners/losers)
- [ ] Mod info dialog — metadata / nexus tab
- [ ] "Problems" button — diagnostics panel
- [ ] "Problems" button — guided fix actions
- [ ] Instance switcher — MO2-style portable UX
- [ ] Downloads view — full pause / resume / cancel semantics tied to §1.4

### 1.6 Cross-cutting — Mod Scanner ([SCANNER_DESIGN.md](SCANNER_DESIGN.md))
- [x] Phase 1 — `ModScanner` trait + core scaffolding (`modde-games/src/traits.rs`)
- [x] Phase 1 — recover deployed-but-untracked mods into DB (`modde scan --import-to <profile>` merges discovered mods into the profile via `ProfileManager::create_or_update`)
- [x] Phase 2 — Cyberpunk 2077 scanner implementation (`modde-games/src/cyberpunk/scanner.rs`)
- [x] Phase 3 — Wabbajack archive matching by hash (`modde-core/src/scanner.rs::match_wabbajack_manifest`)
- [~] CLI surface (`modde scan`) and UI entry point — CLI shipped (`crates/modde-cli/src/commands/scan.rs`); UI button still missing

---

## 2. Test coverage

### 2.1 modde-core (strong — maintain)
- [x] Add proptest for load-order resolver (`tests/resolver_proptest.rs`: identity, determinism, `LoadAfter` honoured, disabled-drop)
- [ ] Add proptest for manifest parser round-trip
- [ ] Concurrent VFS deploy / undeploy stress test
- [ ] DB migration forward/backward integration test

### 2.2 modde-games
- [ ] Generic game support — full integration suite
- [ ] BG3 plugin tests
- [ ] Stardew plugin tests
- [ ] FNV / Oblivion plugin tests
- [ ] BAIN selection-flow tests

### 2.3 modde-sources
- [ ] BAIN end-to-end flow (multi-package, conditional)
- [ ] Resume-after-kill integration test for each backend
- [ ] Hash-mismatch / partial-file recovery tests

### 2.4 modde-cli (thin — priority)
- [x] `assert_cmd` harness + tempdir fixtures (`tests/common/mod.rs`: `Fixture` isolates `MODDE_DATA_DIR` + HOME + XDG)
- [~] `modde install` end-to-end (archive → deploy → verify) — failure paths covered (`tests/cli_install_mod.rs`: bad URL / 404 / non-Premium gate against wiremock); full archive-extract-deploy-verify happy path still pending (needs game plugin fixture + extracted-archive)
- [x] `modde nexus` auth + download flow (mocked API) — `tests/cli_nexus_status.rs` covers premium/free/401 against wiremock; download flow needs a CDN-redirect mock layer
- [x] `modde nxm` URL-handler dispatch (`tests/cli_nxm_dispatch.rs` — covers parse-error paths offline; happy-path needs Nexus mock)
- [~] `modde update` flow — `tests/cli_update_check.rs` covers the no-tracked-mods short-circuit; mocked-Nexus "updates available" path still pending (needs profile seeded with `nexus_mod_id` rows)
- [x] `modde scan` flow (ties to §1.6) — `tests/cli_scan_dispatch.rs` covers unsupported-game, missing game dir, prune-duplicates gate, and empty-dir dry run
- [x] Snapshot tests for CLI help / error output (`tests/cli_help_snapshots.rs` via `insta`)

### 2.5 modde-ui (thin — priority)
- [ ] Iced view snapshot tests (or golden-string equivalent)
- [ ] Semantic e2e expansion beyond current minimal coverage
- [ ] Headless interaction tests for mod-info dialog (after §1.5)

### 2.6 Cross-cutting
- [x] Property-based tests (proptest) introduced as workspace dev-dep
- [x] Criterion benches workspace setup (`Cargo.toml` workspace deps + `crates/modde-core/Cargo.toml` `[[bench]]`)
- [~] Bench: VFS symlink farm deploy/undeploy at 10k / 50k files (`benches/vfs_deploy.rs` covers up to ~10k cold-deploy; 50k requires manual harness — capped at 10k for criterion timing budget)
- [ ] Bench: collision detection at scale
- [ ] Bench: archive extraction (BSA/BA2/zip/7z) — target the native `decompress` readers, not the old `7zz` subprocess path

---

## 3. Infrastructure

### Packaging — flatpak
- [x] Flatpak app ID namespace settled as `com.tartanoglu.modde`; Phase 05 keeps the Flathub path open by using the controlled `tartanoglu.com` domain.

### 3.1 Coverage tooling
- [x] Add `cargo-llvm-cov` to flake devShell
- [x] `just coverage` recipe producing HTML + lcov
- [x] Wire coverage run into Forgejo Actions CI
- [ ] Publish coverage artifact / badge
- [ ] Set baseline % and fail-under threshold (recipe accepts `FAIL_UNDER`; pick a number after first measured run)

### 3.2 CI
- [ ] Cache cargo + nix store between runs
- [ ] Matrix: stable + MSRV
- [x] Clippy `-D warnings` gate (wired in `.forgejo/workflows/ci.yml`; passing on trunk)
- [ ] `cargo deny` advisory + license gate
- [ ] Nightly fuzz job (once fuzz targets exist)

### 3.3 Quality
- [ ] Fuzz targets for archive parsers (BSA/BA2/Wabbajack)
- [ ] Fuzz target for FOMOD XML
- [ ] Doc-test pass — ensure public APIs have runnable examples
- [ ] `cargo doc` deploy to website

### 3.4 Docs / housekeeping
- [ ] Audit `docs/` for stale entries
- [ ] Audit `dist/` for stale artifacts
- [ ] Refresh `website/` against current feature set
- [ ] CHANGELOG entries linked from each completed item above

---

## Priority shortlist (highest leverage first)
1. [x] Wire `cargo-llvm-cov` + baseline (§3.1) — recipe + CI landed; baseline % still TBD
2. [~] CLI integration test harness (§2.4) — fixture, help snapshots, nxm dispatch, nexus status, install-mod failure paths, and update-check short-circuit landed via wiremock; install/update happy paths still pending
3. [x] Mod Scanner Phase 1 (§1.6) — trait, all per-game scanners, Wabbajack matcher, and CLI all already shipped; only UI button outstanding
4. [x] Executable management (§1.1) — registry/args/output/env all already in DB; added `modde exec` top-level alias for discoverability with round-trip tests
5. [~] Criterion benches + VFS stress tests (§2.6, §2.1) — criterion + proptest deps wired, `vfs_deploy` bench live, resolver proptest live; collision + archive-extract benches still pending
