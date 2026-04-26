# Nexus Mods Integration

## Overview

modde has comprehensive Nexus Mods integration: API v1 client, CDN downloads, mod update checking, collection support, and `nxm://` browser protocol handling. The truth today is that Nexus, Wabbajack, and Collections are the real install surfaces; the Downloads UI now renders actual queue state, but transport-level pause/resume and durable metadata sidecars are still incomplete.

## Architecture

### API Client

**Key file:** `crates/modde-sources/src/nexus/api.rs`

`NexusApi` wraps all Nexus v1 endpoints:

| Method | Endpoint | Purpose |
|--------|----------|---------|
| `get_mod()` | `/games/{domain}/mods/{id}.json` | Mod details |
| `get_mod_files()` | `/games/{domain}/mods/{id}/files.json` | File listings |
| `search_mods()` | `/games/{domain}/mods/search.json` | Search |
| `trending_mods()` | `/games/{domain}/mods/trending.json` | Trending |
| `updated_mods()` | `/games/{domain}/mods/updated.json` | Recently updated |
| `get_collection()` | `/games/{domain}/collections/{slug}.json` | Collection manifest |
| `get_collection_by_slug()` | Two-step auto-discovery | Collection install |

### Authentication

**Key file:** `crates/modde-sources/src/nexus/auth.rs`

API key lookup chain (highest priority first):
1. `NEXUS_API_KEY` environment variable
2. System keyring (secret-service D-Bus) — `modde/nexus-api-key`
3. `NEXUS_API_KEY_FILE` file path (sops-nix compatible)

CLI commands:
- `modde nexus auth` — store API key in keyring
- `modde nexus status` — validate key, check premium status

### Mod Update Checking

**Key file:** `crates/modde-sources/src/nexus/updates.rs`

How it works:
1. Collects mods with Nexus metadata (`nexus_mod_id`, `nexus_game_domain`, `installed_timestamp`) from the active profile
2. Groups by game domain to minimize API calls
3. Calls `updated_mods(domain, period)` once per domain
4. Cross-references: `latest_file_update > installed_timestamp` = update available

CLI: `modde update check [--profile P] [--game G] [--period 1d|1w|1m]`

### nxm:// Protocol Handler

**Key file:** `crates/modde-cli/src/commands/nxm.rs`

Parses URIs like: `nxm://skyrimspecialedition/mods/12604/files/35834?key=abc&expires=123`

- `modde nxm handle <uri>` — download mod file via CDN
- `modde nxm install` — register as system nxm:// handler via XDG desktop entry

### Download Sources

**Key file:** `crates/modde-sources/src/traits.rs`

The `DownloadSource` trait with `AnySource` enum dispatch supports:
- **Nexus** — Premium CDN downloads with hash verification
- **GitHub** — Direct release downloads
- **Google Drive** — GDrive file downloads
- **Mega** — Mega.nz downloads
- **Direct** — Generic HTTP URLs with optional headers

All sources support progress callbacks and xxh3-64 hash verification. In normal user-facing flows, Nexus is first-class today; the other backends are used primarily by Wabbajack/directive installs.

### Wabbajack Integration

**Key file:** `crates/modde-sources/src/wabbajack/`

Full manifest parser supporting:
- `FromArchive`, `CreateDirectory`, `InlineFile`, `PatchedFromArchive`, `CreateBSA` directives
- Concurrent downloads (configurable parallelism)
- BSA archive repacking
- Binary patching

### Collection Support

Two-step collection install flow:
1. `get_collection_meta(slug)` — discovers game domain
2. `get_collection_revision(domain, slug, revision)` — fetches full manifest

CLI: `modde install nexus-collection <slug> [--version V]`

## MO2 Feature Comparison

| MO2 Feature | modde | Notes |
|-------------|-------|-------|
| Nexus API integration | **Done** | Full v1 API client |
| API key management | **Done** | Env, keyring, file fallback chain |
| CDN download links | **Done** | Requires Premium |
| Mod update checking | **Done** | Per-domain batched checking |
| "Download with Manager" (nxm://) | **Done** | XDG desktop handler |
| Rate limit tracking | **Done** | Warns when < 10 remaining |
| Nexus Collections | **Done** | Full collection install flow |
| Wabbajack modlist support | **Done** | Full manifest + download + install |
| Nexus OAuth2 | -- | Currently API key only |
| Download queue with pause/resume | Partial | UI queue state exists, but pause/resume is not yet wired to the network layer |
| Download states (pausing, fetching info, etc.) | Partial | Basic queued/active/paused/failed/complete state exists; MO2 is still richer |
| Speed tracking / ETA | -- | Progress callbacks exist but no speed calc |
| .meta sidecar files per download | -- | Data model exists in code, but it is not yet a shipped end-to-end workflow |
| Query Info via MD5 hash lookup | -- | MO2 can identify unknown archives by hashing against Nexus |
| Drag-and-drop download to mod list | -- | MO2 lets you drop a download at a specific priority |
| Endorsement integration | -- | MO2: endorse, un-endorse, won't endorse, with flag icons |
| Tracking integration | -- | MO2: start/stop tracking with pin icon in flags column |
| Version color-coding (green/red) | -- | MO2 colors version field based on update state |
| "Ignore update" per mod | -- | MO2 can suppress update notifications for specific versions |
| "Change versioning scheme" | -- | MO2 cycles version parsers when downgrade is falsely detected |
| "Force-check updates" per mod | -- | MO2 can re-query a single mod from context menu |
| Category auto-mapping from Nexus | -- | MO2 reads Nexus category and maps to local categories |
| Compact download list view | -- | MO2 has a toggle for denser download display |
| Status bar API counter (live) | -- | MO2 shows queued/daily/hourly with color coding in status bar |
