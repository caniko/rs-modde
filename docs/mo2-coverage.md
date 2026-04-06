# Mod Organizer 2 Feature Coverage

Comprehensive tracking of modde's coverage of every known Mod Organizer 2 feature. Based on deep research of MO2's source code, wiki, changelogs, and community documentation.

## Skill Documents

| Document | Scope |
|----------|-------|
| [VFS & Deployment](skills/vfs-deployment.md) | Symlink farm, conflict resolution, file hiding, overwrite capture, Data tab |
| [Profiles & Organization](skills/profiles-organization.md) | Profile system, categories, tags, dual-pane, filters, flags, columns |
| [Nexus Integration](skills/nexus-integration.md) | API client, update checking, nxm://, downloads, endorsement, .meta files |
| [Bethesda Plugin Management](skills/bethesda-plugins.md) | Load order, LOOT, validation, BSA handling, Archives tab, diagnostics |
| [Save Management](skills/save-management.md) | Git-backed vaults, fingerprinting, auto-capture |
| [Tools & Installers](skills/tools-installers.md) | FOMOD, Wabbajack, tool launcher, executables, game detection, mod info dialog |

---

## Legend

| Symbol | Meaning |
|--------|---------|
| **Done** | Fully implemented and tested |
| Partial | Core logic exists, not fully polished |
| -- | Not implemented |
| N/A | Not applicable (Linux, architecture difference) |

---

## 1. Core VFS & Deployment

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 1 | Virtual filesystem (clean game folder) | **Done** | Symlink farm; globally visible to all processes |
| 2 | Per-mod isolated directories | **Done** | Content store at `~/.local/share/modde/store/` |
| 3 | Priority-based file conflict resolution | **Done** | Position in mod list = priority; last wins |
| 4 | Per-file hiding (.mohidden) | **Done** | `hidden_files` DB table + VFS exclusion |
| 5 | Batch unhide all hidden files in a mod | -- | MO2 "Restore hidden files" context action |
| 6 | Overwrite folder for tool output | **Done** | `modde tool run` with diff-based capture |
| 7 | Per-executable output mod | -- | MO2 lets each tool write to a specific named mod, not just Overwrite |
| 8 | Overwrite management (sync to mods, create mod, clear) | -- | MO2 has specialized Overwrite context actions |
| 9 | Atomic rollback | **Done** | staging.bak swap |
| 10 | Data tab (merged VFS browser) | -- | MO2 shows merged virtual FS with conflict/archive/hidden filters |
| 11 | Automatic archive invalidation | -- | MO2 generates BSA invalidation files per-profile |
| 12 | BSA back-dating | -- | MO2 changes BSA timestamps so loose files always win |

## 2. Profile System

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 13 | Multiple profiles per game | **Done** | SQLite-backed, unlimited |
| 14 | Independent mod lists per profile | **Done** | `profile_mods` with `sort_index` |
| 15 | Per-profile INI files | **Done** | Auto-swap on profile switch |
| 16 | Per-profile save games | **Done** | Git-backed vault with auto-swap |
| 17 | Instant profile switching | **Done** | Save + INI swap in one operation |
| 18 | Profile forking/cloning | **Done** | `profile fork` clones mods + rules + saves |
| 19 | Experiment mode (try/rollback/commit) | **Done** | Stackable — unique to modde |
| 20 | Profile-specific separator collapse state | -- | MO2 saves expand/collapse per profile |

## 3. Plugin Load Order (Bethesda)

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 21 | Dual-pane: mod priority vs plugin order | **Done** | Separate `plugin_order` table |
| 22 | plugins.txt read/write | **Done** | Proton path auto-resolution |
| 23 | ESM/ESL flag detection | **Done** | Binary header parsing |
| 24 | Form 43 detection | **Done** | Warns about Oldrim plugins in SSE |
| 25 | Missing master detection | **Done** | Header master list vs active plugins |
| 26 | LOOT integration (auto-sort) | **Done** | Pure Rust masterlist parser |
| 27 | LOOT sorting reports | Partial | Rules printed, no rich diagnostic report |
| 28 | "Lock load order" for individual plugins | -- | MO2 pins plugins to resist LOOT re-sorting |
| 29 | Plugin load order backup/restore | -- | MO2 can snapshot and restore plugin order |
| 30 | Optional ESPs (move plugins to optional/ subfolder) | -- | MO2 mod info dialog has Optional ESPs tab |
| 31 | Plugin list columns (priority, mod index, form version, header version, author, description) | -- | MO2 has 8 columns in plugin list |
| 32 | Archives tab (BSA load order management) | -- | MO2 has dedicated Archives right-pane tab |

## 4. Conflict Resolution

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 33 | File-level conflict detection | **Done** | `ConflictMap` tracks all providers |
| 34 | Per-file winner display | **Done** | `winner_for()` with priority + hidden |
| 35 | Per-file hiding | **Done** | DB + VFS integration |
| 36 | Conflict flags column (overwrite/overwritten/mixed/redundant) | -- | MO2 shows 5 conflict flag states per mod |
| 37 | Conflict detail in mod info (General + Advanced tabs) | -- | MO2 has two sub-tabs: overwriting/overwritten/non-conflicting |
| 38 | "Go to..." conflicting mod navigation | -- | MO2 conflict items link to the other mod involved |
| 39 | BSA/BA2 archive conflict detection | -- | MO2 indexes files inside archives for conflict checking |
| 40 | BSA/BA2 content preview | -- | No archive content browsing |

## 5. Nexus Mods Integration

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 41 | API key authentication | **Done** | Env, keyring, file fallback chain |
| 42 | OAuth2 authentication | -- | API key only currently |
| 43 | Mod details/search | **Done** | Full API v1 client |
| 44 | CDN download links | **Done** | Requires Premium |
| 45 | "Download with Manager" (nxm://) | **Done** | XDG desktop handler |
| 46 | Mod update checking | **Done** | Per-domain batched |
| 47 | API rate limit tracking | **Done** | Warns when < 10 remaining |
| 48 | Nexus Collections | **Done** | Full collection install flow |
| 49 | Wabbajack modlist support | **Done** | Full manifest + download + install |
| 50 | Download queue with pause/resume | -- | Sequential downloads only |
| 51 | Download states (10 distinct states) | -- | MO2 has pausing, fetching info, etc. |
| 52 | Speed tracking / ETA | -- | Progress callbacks exist but no speed |
| 53 | .meta sidecar files per download | -- | MO2 stores rich metadata per archive |
| 54 | Query Info via MD5 hash | -- | MO2 identifies unknown archives by hashing |
| 55 | Drag-and-drop download to mod list | -- | MO2 drops download at specific priority |
| 56 | Endorsement (endorse/un-endorse/won't endorse) | -- | MO2 has flag icons and context actions |
| 57 | Tracking (start/stop tracking on Nexus) | -- | MO2 has pin icon and context actions |
| 58 | Version color-coding (green=current, red=outdated) | -- | MO2 colors version field |
| 59 | "Ignore update" per mod | -- | Suppresses update notification for a version |
| 60 | "Force-check updates" per mod | -- | Re-query single mod from context menu |
| 61 | Category auto-mapping from Nexus categories | -- | MO2 maps Nexus categories to local |
| 62 | Status bar API counter (queued/daily/hourly) | -- | MO2 shows live counter with color coding |

## 6. Mod Organization & UI

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 63 | Mod categories | **Done** | DB-backed with colors |
| 64 | Collapsible separators with aggregated child info | -- | MO2 shows combined flags/content when collapsed |
| 65 | Separator scrollbar color markers | -- | MO2 draws marks on scrollbar at separator positions |
| 66 | Per-mod notes | **Done** | `notes` field on EnabledMod |
| 67 | Per-mod tags | **Done** | JSON array in `tags` field |
| 68 | Per-row notes color | -- | MO2 lets you color-code individual rows |
| 69 | "Send to..." priority system | -- | Send to separator, first/last conflict, specific priority |
| 70 | Three-state filters (show/hide/only) | -- | DB supports it, UI not wired |
| 71 | AND/OR filter logic | -- | MO2 supports AND/OR across criteria |
| 72 | Inverted (negated) filters | -- | Each criterion can be inverted |
| 73 | Content type filter icons | -- | MO2 auto-detects textures, meshes, scripts, etc. per mod |
| 74 | Group-by modes (category, Nexus ID) | -- | MO2 mod list can be grouped |
| 75 | Flags column (~12 status icons) | -- | Invalid, backup, foreign, hidden, endorsed, notes, tracked, etc. |
| 76 | Content column (content type icons) | -- | Per-mod icons for detected content types |
| 77 | Author / Uploader columns | -- | MO2 tracks both separately |
| 78 | Install time column | -- | MO2 records when each mod was installed |
| 79 | CSV export (configurable columns/rows) | -- | MO2 exports active/all/visible mods |
| 80 | Compact/detailed list view toggle | -- | MO2 has denser list mode |
| 81 | "Mark as converted/working" | -- | Dismiss alternate-game warnings |
| 82 | "Ignore missing data" | -- | Dismiss invalid-data warnings |
| 83 | Keyboard shortcuts (A-Z jump, F2 rename, Space toggle) | -- | MO2 has extensive shortcuts |

## 7. Mod Backup & Reinstall

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 84 | Mod backup (copy mod directory) | -- | MO2 creates backup copies with `_backup1` suffix |
| 85 | Mod restore from backup | -- | MO2 replaces current mod with backup |
| 86 | Reinstall mod from original archive | -- | MO2 re-runs installer from stored archive |

## 8. Installer Support

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 87 | FOMOD installer | **Done** | Interactive wizard + declarative configs |
| 88 | BAIN installer | -- | Wizard-based Bethesda installer format |
| 89 | OMOD installer | -- | Legacy Oblivion format |
| 90 | NCC installer | -- | Legacy NMM format |
| 91 | Bundle installer | -- | MO2-specific multi-archive |
| 92 | Manual install | **Done** | CLI-based, URL or local file |

## 9. Mod Information Dialog

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 93 | Text Files tab (in-mod text editor) | -- | MO2 edits .txt/.log/.xml/.json inside mods |
| 94 | INI Files tab (per-mod INI tweaks) | -- | MO2 edits .ini files inside mods in-place |
| 95 | Images tab (thumbnail browser + preview) | -- | MO2 shows images inside mods with DDS support |
| 96 | Optional ESPs tab | -- | Move plugins between active/optional within a mod |
| 97 | Conflicts tab (General: overwriting/overwritten/non-conflicting) | -- | MO2 has detailed two-sub-tab conflict view |
| 98 | Conflicts tab (Advanced: unified list with alternatives) | -- | MO2 shows every file with conflict status |
| 99 | Categories tab (multi-assign + primary) | -- | MO2 has checkbox tree + primary selector |
| 100 | Nexus tab (embedded browser, version, endorse/track) | -- | MO2 shows Nexus page with action buttons |
| 101 | Notes tab (short notes + extended comments) | Partial | Notes field exists, no extended comments |
| 102 | Filetree tab (full mod directory browser) | -- | MO2 has file tree with rename/delete/hide/execute actions |
| 103 | Previous/Next mod navigation in dialog | -- | PgUp/PgDn to navigate without closing |

## 10. Save Management

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 104 | Per-profile save games | **Done** | Git-backed vaults with full history |
| 105 | Save swapping on profile switch | **Done** | Automatic with fingerprint tracking |
| 106 | Saves tab in right pane | Partial | MO2 shows save list; modde has CLI + Iced view |
| 107 | Save history/snapshots | **Done** | Full git history — MO2 has no equivalent |
| 108 | Save restore from snapshot | **Done** | With mod mismatch warnings — MO2 has no equivalent |
| 109 | Auto-capture on game exit | **Done** | `save auto-capture`, `save watch` — MO2 has no equivalent |
| 110 | Mod fingerprint warnings | **Done** | SHA-256 fingerprint — MO2 has no equivalent |
| 111 | Unadopted save detection | **Done** | Prompts before overwriting |

## 11. Tool Integration

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 112 | Run tools through VFS | **Done** | Symlinks globally visible |
| 113 | Tool output capture (Overwrite) | **Done** | Diff-based capture to `__overwrite__` |
| 114 | Tool auto-detection | **Done** | Scans for xEdit, FNIS, Nemesis, etc. |
| 115 | Executables management dialog | -- | MO2: name, path, args, working dir, Steam overlay, output mod |
| 116 | Per-executable output mod redirect | -- | Each tool writes to a named mod, not just Overwrite |
| 117 | Forced DLL injection per executable | -- | MO2 can inject DLLs into launched processes |
| 118 | Desktop/Start Menu shortcut creation | -- | Launch through VFS from desktop shortcut |

## 12. Diagnostics & Notifications

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 119 | Problems/notifications button | -- | MO2 has plugin-based diagnostic system |
| 120 | Diagnostic plugins (IPluginDiagnose) | -- | Extensible issue detection with guided auto-fixes |
| 121 | Built-in diagnostics (missing masters, Form 43, overwrite not empty) | Partial | We detect Form 43 and missing masters via CLI |
| 122 | Log panel (dockable, configurable verbosity) | -- | MO2 has real-time log viewer in main window |
| 123 | Crash dump management | -- | MO2 configures and manages crash dumps |

## 13. Extensibility

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 124 | Python plugin system | -- | MO2 has IPluginGame, IModList, etc. |
| 125 | Plugin hot-reload | -- | MO2 reloads plugins without restart |
| 126 | Preview plugin architecture | -- | MO2 has base preview + DDS preview plugins |
| 127 | Diagnostic plugin architecture | -- | Any plugin can register problems |
| 128 | Tool plugin architecture | -- | FNIS, BodySlide etc. integrate as tool plugins |

## 14. Game Support

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 129 | Skyrim SE/AE | **Done** | Full support with LOOT, saves, INI, plugins |
| 130 | Fallout 4 | **Done** | Full support |
| 131 | Fallout 76 | **Done** | Server-side saves noted |
| 132 | Cyberpunk 2077 | **Done** | REDmod, CET, REDscript, proxy DLLs |
| 133 | Generic game support | **Done** | `GenericGame` for unlisted games |
| 134 | 50+ other games | -- | 5 games vs MO2's 50+ |
| 135 | Starfield | -- | High demand, not yet added |
| 136 | Oblivion / Morrowind / older Bethesda | -- | Would need new game plugins |

## 15. Platform & Infrastructure

| # | MO2 Feature | modde | Notes |
|---|-------------|-------|-------|
| 137 | Linux/Steam Deck native | **Done** | No Wine needed for the manager itself |
| 138 | Proton/Wine game integration | **Done** | Path resolution, DLL overrides, launch wrappers |
| 139 | Instance management (portable/global) | -- | MO2 has portable and global instance modes |
| 140 | Offline mode | -- | MO2 can disable all internet access |
| 141 | Theming (QSS/CSS) | **Done** | 6 themes: Dark, Light, Dracula, Nord, Gruvbox, Catppuccin |
| 142 | Tutorial/first-run guidance | -- | MO2 has overlay hints for new users |
| 143 | Built-in web browser | -- | MO2 has tabbed QWebEngine browser |

---

## Coverage Summary

| Category | Done | Partial | Not Yet | Total | Coverage |
|----------|------|---------|---------|-------|----------|
| 1. Core VFS & Deployment | 6 | 0 | 6 | 12 | 50% |
| 2. Profile System | 7 | 0 | 1 | 8 | 88% |
| 3. Plugin Load Order | 6 | 1 | 5 | 12 | 54% |
| 4. Conflict Resolution | 3 | 0 | 5 | 8 | 38% |
| 5. Nexus Integration | 9 | 0 | 13 | 22 | 41% |
| 6. Mod Organization & UI | 3 | 0 | 18 | 21 | 14% |
| 7. Mod Backup & Reinstall | 0 | 0 | 3 | 3 | 0% |
| 8. Installer Support | 2 | 0 | 4 | 6 | 33% |
| 9. Mod Information Dialog | 0 | 1 | 10 | 11 | 5% |
| 10. Save Management | 7 | 1 | 0 | 8 | 94% |
| 11. Tool Integration | 3 | 0 | 4 | 7 | 43% |
| 12. Diagnostics & Notifications | 0 | 1 | 4 | 5 | 10% |
| 13. Extensibility | 0 | 0 | 5 | 5 | 0% |
| 14. Game Support | 5 | 0 | 3 | 8 | 63% |
| 15. Platform & Infrastructure | 3 | 0 | 4 | 7 | 43% |
| **TOTAL** | **54** | **4** | **85** | **143** | **40%** |

## modde-Only Features (Not in MO2)

These features exist in modde but have no MO2 equivalent:

| Feature | Description |
|---------|-------------|
| Git-backed save vaults | Full history, branching, diffing for game saves |
| Save fingerprinting | SHA-256 of save-breaking mods; warns on mismatch during restore |
| Stackable experiment mode | try/rollback/commit for non-destructive profile testing |
| Declarative FOMOD configs | TOML/JSON/Nix configs for reproducible, non-interactive installs |
| Wabbajack installation | modde installs Wabbajack modlists (MO2 is a target, not an installer) |
| Nexus Collections install | Full collection install flow (MO2 doesn't support Collections) |
| Multi-launcher detection | Steam + Heroic GOG/Epic/Sideload in one scan |
| NixOS-native | sops-nix compatible key management, no Wine needed |
| Profile forking with save branch cloning | Clone a profile including its entire save history |
| Save auto-capture on game exit | Automatic save snapshot when game closes |
| Stock game tree-hash verification | xxh3-64 deterministic tree hashing for vanilla integrity |

## Gap Analysis: What Matters Most

### Tier 1: High Impact, Achievable

These gaps affect daily workflow for power users with 100+ mods:

1. **Mod Information Dialog** (#93-103) — The single biggest UI gap. MO2's double-click-a-mod experience with 9 tabs (filetree, conflicts, images, text editor) is essential for debugging mod issues.
2. **Conflict detail view** (#36-38) — Showing overwriting/overwritten/mixed flags per mod and navigating between conflicting mods.
3. **Download queue with pause/resume** (#50-52) — Essential for large modlist builds.
4. **BSA/BA2 archive conflict detection** (#39) — Many Bethesda mod conflicts are archive-vs-loose.
5. **Endorsement and tracking** (#56-57) — Community participation from within the manager.
6. **CSV export** (#79) — Sharing/documenting mod lists.
7. **More games** (#134-136) — Starfield especially.

### Tier 2: Quality of Life

8. **Collapsible separators with aggregated info** (#64) — Makes 200+ mod lists navigable.
9. **"Send to..." priority system** (#69) — Faster conflict resolution workflow.
10. **Version color-coding** (#58) — Visual update status at a glance.
11. **Data tab (merged VFS browser)** (#10) — See the game's view of all files.
12. **Problems/diagnostics system** (#119-121) — Proactive issue detection.
13. **Mod backup/restore** (#84-86) — Safety net when updating mods.
14. **Plugin load order backup/restore** (#29) — Snapshot before LOOT sorting.

### Tier 3: Nice to Have

15. **Full executable management** (#115-118) — Per-tool config, shortcuts.
16. **Filter system** (#70-74) — AND/OR, three-state, content type.
17. **Plugin system** (#124-128) — Community extensibility.
18. **Instance management** (#139) — Portable vs global.
19. **Legacy installers** (#88-91) — BAIN, OMOD, NCC.
