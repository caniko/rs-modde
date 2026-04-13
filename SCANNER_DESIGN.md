# Modular Game Trait System — Scanner + Ecosystem Integration

## Context

Profile "3077" for Cyberpunk 2077 has ~170 mods deployed on disk but zero records in the database. There's no way to scan and discover installed mods. Beyond that, several modde subsystems have hardcoded game logic that should be unified under the trait system.

The existing `GamePlugin` and `SaveTracker` traits in `modde-games` are well-designed — this plan extends that pattern with a new `ModScanner` trait and consolidates scattered game-specific logic back into traits.

## Phase 1: Core Types & `ModScanner` Trait

**File: `crates/modde-games/src/traits.rs`** — Add new types and trait

```rust
/// A file discovered on disk during a mod scan.
pub struct DiscoveredFile {
    pub rel_path: String,  // relative to install dir, forward slashes
    pub size: u64,
}

/// How a mod was identified.
pub enum ModSource {
    /// Matched against a Wabbajack manifest archive.
    Wabbajack { archive_hash: u64, archive_name: String },
    /// Matched against Nexus metadata.
    Nexus { mod_id: i64, file_id: i64, game_domain: String },
    /// Detected from filesystem structure alone.
    Filesystem { location: String },
}

/// A mod discovered by scanning the game directory.
pub struct DiscoveredMod {
    pub mod_id: String,
    pub display_name: String,
    pub version: Option<String>,
    pub files: Vec<DiscoveredFile>,
    pub source: ModSource,
    pub confidence: f32,  // 0.0..=1.0
}

/// Context for scanning — carries manifest data and stock file exclusions.
pub struct ScanContext<'a> {
    pub install_dir: &'a Path,
    pub wabbajack_manifest: Option<&'a WabbajackManifest>,
    pub stock_files: Option<&'a HashSet<String>>,
}

/// Game-specific mod scanning and discovery.
pub trait ModScanner: Send + Sync {
    /// Directories to scan, relative to install root.
    fn scan_directories(&self) -> &[&str];

    /// Scan game directory and return discovered mods from filesystem structure.
    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> anyhow::Result<Vec<DiscoveredMod>>;
}
```

**File: `crates/modde-games/src/traits.rs`** — Add shared helpers

```rust
/// Walk all files under `dir`, returning paths relative to `root`.
pub fn walk_files_relative(root: &Path, dir: &Path) -> Vec<DiscoveredFile>;

/// Generate a filesystem-safe slug from a mod name.
pub fn slug(name: &str) -> String;
```

**File: `crates/modde-games/src/lib.rs`** — Add resolver

```rust
pub fn resolve_mod_scanner(game_id: &str) -> Option<&'static dyn ModScanner>;
```

## Phase 2: Cyberpunk Scanner (MVP — unblocks 3077 profile)

**New file: `crates/modde-games/src/cyberpunk/scanner.rs`**

Scans these Cyberpunk-specific mod locations:

| Directory | Grouping strategy |
|-----------|------------------|
| `bin/x64/plugins/cyber_engine_tweaks/mods/` | Each subdirectory = one CET mod |
| `r6/scripts/` | Each subdirectory = one REDscript mod |
| `r6/tweaks/` | Each subdirectory = one TweakXL mod |
| `archive/pc/mod/` | Each `.archive` file = one mod |
| `mods/` | Each subdirectory = one REDmod (parse `info.json` for metadata) |

For REDmod mods, parse `info.json` to extract name/version. For CET mods, check for `init.lua` as confidence signal.

## Phase 3: Wabbajack Manifest Matching (game-agnostic, in modde-core)

**New file: `crates/modde-core/src/scanner.rs`**

```rust
/// Match files on disk against a Wabbajack manifest.
/// Groups directives by archive_hash, checks what fraction of each
/// archive's `to` paths exist in `on_disk_files`.
pub fn match_wabbajack_manifest(
    manifest: &WabbajackManifest,
    on_disk_files: &HashSet<String>,  // lowercased, forward-slash paths
    threshold: f32,
) -> Vec<DiscoveredMod>;

/// Convert a DiscoveredMod into an EnabledMod for database storage.
pub fn discovered_to_enabled(discovered: &DiscoveredMod) -> EnabledMod;
```

Algorithm:
1. `manifest.install_directives()` → group `FromArchive`/`PatchedFromArchive` by `archive_hash`
2. Normalize `to` paths: `.replace('\\', "/").to_lowercase()`
3. For each archive: `present_count / total_count >= threshold` → emit `DiscoveredMod`
4. Populate `nexus_mod_id`/`nexus_file_id` from `ArchiveState::NexusDownloader` when available

## Phase 4: `modde scan` CLI Command

**New file: `crates/modde-cli/src/commands/scan.rs`**

```
modde scan --game <id> [--game-dir <path>] [--manifest <.wabbajack>] [--import-to <profile>] [--threshold 0.5] [--dry-run]
```

Handler flow:
1. Resolve scanner via `resolve_mod_scanner(&game)`, bail if unsupported
2. Resolve install dir from `--game-dir` or `GamePlugin::detect_install()`
3. Build case-insensitive file index of game directory (walkdir, lowercased paths → `HashSet<String>`)
4. If `--manifest` provided: call `modde_core::scanner::match_wabbajack_manifest()`
5. Call `scanner.scan_filesystem()` for filesystem-based discovery
6. Merge: manifest matches take precedence; filesystem-only mods fill gaps
7. Print report table (mod name, source, confidence, file count)
8. If `--import-to`: convert to `EnabledMod`, merge into profile via `ProfileManager::create_or_update()`

## Phase 5: Bethesda Scanner

**New file: `crates/modde-games/src/bethesda/scanner.rs`**

Data-driven `BethesdaScanner` struct (like `BethesdaGame`):
- Scans `Data/` for `.esp`/`.esm`/`.esl` plugins
- Groups each plugin + companion `.bsa`/`.ba2` as one mod
- Reads `plugins.txt` for enabled/disabled status
- Uses existing `plugin_header.rs` to extract form version

## Phase 6: Consolidate Hardcoded Game Logic into Traits

### 6a. External tool detection → `GamePlugin`

**File: `crates/modde-cli/src/commands/tool.rs:120-128`** — Hardcoded Bethesda tool list.

**Fix:** Add to `GamePlugin`:
```rust
fn external_tools(&self) -> &[(&str, &[&str])] { &[] }
```

### 6b. LOOT game ID mapping → `GamePlugin`

**File: `crates/modde-cli/src/commands/loot.rs:110-117`** — Hardcoded `match game_id`.

**Fix:** Add to `GamePlugin`:
```rust
fn steam_app_id(&self) -> Option<&str> { None }
fn my_games_dir(&self) -> Option<&str> { None }
fn supports_plugin_sorting(&self) -> bool { false }
```

### 6c. OptiScaler `bin/x64` path → `GamePlugin::executable_dir()`

**File: `crates/modde-games/src/tools/optiscaler.rs:171`** — Hardcoded `game_dir.join("bin/x64")`.

**Fix:** Pass `game_plugin` and use `game_plugin.executable_dir(install)`.

### 6d. Deduplicate deploy logic (CLI vs UI)

Deploy is implemented in both `deploy.rs` and `app.rs`. Extract shared `modde_core::deploy::deploy_profile()`.

## Files Summary

| File | Action |
|------|--------|
| `crates/modde-games/src/traits.rs` | Add `ModScanner` trait, types, helpers, extend `GamePlugin` |
| `crates/modde-games/src/lib.rs` | Add `resolve_mod_scanner()`, export new types |
| `crates/modde-games/src/cyberpunk/scanner.rs` | **New** — Cyberpunk scanner implementation |
| `crates/modde-games/src/cyberpunk/mod.rs` | Add `pub mod scanner;` |
| `crates/modde-games/src/bethesda/scanner.rs` | **New** — Bethesda scanner implementation |
| `crates/modde-games/src/bethesda/mod.rs` | Add `pub mod scanner;` |
| `crates/modde-core/src/scanner.rs` | **New** — Wabbajack manifest matching + `discovered_to_enabled()` |
| `crates/modde-core/src/lib.rs` | Add `pub mod scanner;` |
| `crates/modde-cli/src/commands/scan.rs` | **New** — `modde scan` CLI command |
| `crates/modde-cli/src/commands/mod.rs` | Add `pub mod scan;` |
| `crates/modde-cli/src/main.rs` | Add `Scan` variant, wire handler |
| `crates/modde-cli/src/commands/tool.rs` | Migrate hardcoded tools → trait |
| `crates/modde-cli/src/commands/loot.rs` | Migrate hardcoded game mapping → trait |
| `crates/modde-games/src/tools/optiscaler.rs` | Use `executable_dir()` instead of hardcoded path |

## Implementation Order

1. **Phase 1** — Core types + trait definition (no behavior change, everything compiles)
2. **Phase 2** — Cyberpunk scanner (unblocks the 3077 use case immediately)
3. **Phase 3** — Wabbajack manifest matching in modde-core
4. **Phase 4** — `modde scan` CLI command (test end-to-end with 3077.wabbajack)
5. **Phase 5** — Bethesda scanner
6. **Phase 6a-d** — Consolidate hardcoded logic (can be done incrementally)

## Verification

1. `cargo build -p modde-cli` — all phases must compile
2. `modde scan --game cyberpunk2077 --game-dir "/path/to/cyberpunk/" --dry-run` — filesystem scan
3. `modde scan --game cyberpunk2077 --manifest 3077.wabbajack --dry-run` — manifest + filesystem
4. `modde scan ... --import-to 3077` — populate profile, verify in UI
5. `cargo test -p modde-games` — scanner unit tests
