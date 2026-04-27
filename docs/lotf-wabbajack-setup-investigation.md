# Legends of the Frost Wabbajack Setup Investigation

Date: 2026-04-27

## Scope

This report records the sandboxed Legends of the Frost Wabbajack setup probe
performed against the current authored-files artifact:

```text
https://authored-files.wabbajack.org/Legends of the Frost.wabbajack_25251156-7fbf-48e2-808c-001a62e1dd97
```

The install trial used isolated modde directories and a copied fake Skyrim
directory. It did not deploy into the live Steam Skyrim install.

## Implemented During Investigation

- Parsed `WabbajackCDNDownloader+State, Wabbajack.Lib` archives.
- Added a Wabbajack authored-files chunk downloader for CDN-backed archives.
- Added a generic HTML mirror resolver used by ModDB-style intermediate pages.
- Mapped Wabbajack `ModDBDownloader` archives to normal direct downloads with
  resolver metadata instead of a dedicated ModDB downloader.
- Added candidate fallback in `DirectSource`: resolved mirrors are tried in
  order, then verified with the normal hash path.
- Added Home Manager `wabbajackList.path` support for local or Nix-store
  `.wabbajack` files.
- Added config-file Nexus API key loading so tests can use the normal modde
  credential path without exposing the key.
- Added archive cache verification before skipping existing store files, so
  interrupted downloads are removed and retried.
- Improved game-file source validation to report all missing or mismatched
  local game files in one error.
- Added authored-file availability preflight, so unavailable Wabbajack CDN
  archives fail before bulk downloads.
- Added a dedicated missing-authored-artifact remediation error.
- Added `modde wabbajack import-archive` for hash-verified local archive import.
- Added resumable authored-files chunk downloads using a `.part` body and
  `.part.json` sidecar.
- Added a synthetic Wabbajack pipeline test that exercises late installer stages
  without relying on live third-party services.

## Validation Completed

Focused automated checks passed:

```bash
nix develop . -c cargo fmt --check
nix develop . -c cargo test -p modde-core manifest::wabbajack
nix develop . -c cargo test -p modde-sources mirror
nix develop . -c cargo test -p modde-sources wabbajack
nix develop . -c cargo test -p modde-sources --test download_source_tests
nix develop . -c cargo test -p modde-sources direct::tests::direct_download_tries_candidate_urls_in_order
nix develop . -c cargo test -p modde-cli cli_wabbajack
```

Nexus credential bridge was verified without reading the key:

```bash
stat -c 'mode=%a size=%s path=%n' /home/can/.config/modde/nexus_api_key
nix develop . -c cargo run -q -p modde-cli -- nexus status
```

Observed result:

```text
mode=600 size=88 path=/home/can/.config/modde/nexus_api_key
Nexus API key: valid
Account type: Premium
```

The synthetic LoTF probe command was:

```bash
XDG_DATA_HOME=/tmp/modde-lotf-data \
XDG_CONFIG_HOME=/home/can/.config \
XDG_CACHE_HOME=/tmp/modde-lotf-cache \
nix develop . -c cargo run -q -p modde-cli -- install wabbajack \
  '/tmp/modde-lotf-plan/Legends of the Frost.local-game-hashes.wabbajack' \
  --profile skyrim-synthetic \
  --game-dir /tmp/fake-skyrim-copy
```

The probe confirmed:

- Nexus downloads work with the shared config-file key.
- Wabbajack authored CDN downloads work for available authored files.
- The four ModDB Skyrim Realistic Overhaul entries resolve through the generic
  HTML mirror layer and download successfully.
- The install advanced beyond the earlier parser, Nexus, ModDB, and CDN-source
  blockers.

## Current State

modde now fails fast during Wabbajack preflight when the LoTF manifest references
unavailable authored-files archives. The current synthetic LoTF probe stops
before bulk downloads and reports exactly the three missing authored files below,
including their expected Wabbajack hashes and `curl -fI` validation commands.

This is the expected result until those exact upstream files are restored or
the user imports exact local copies by hash. The synthetic artifact remains a
pipeline probe because its local game-file hashes were rewritten; it is not
proof of a real LoTF install.

## Current Blocking Input

The run now stops because three Wabbajack-authored CDN archives referenced by
the LoTF manifest return `404 Not Found` from both the direct CDN URL and the
chunk metadata endpoint.

Missing required artifacts:

| Archive | Munged authored-file id | Required because |
| --- | --- | --- |
| `Legends of the Frost - DynDOLOD Output 2.5.1.7z` | `Legends of the Frost - DynDOLOD Output 2.5.1.7z_d08ce1cb-b109-4d6f-890d-5fa4e1b5241d` | The `.wabbajack` manifest references this archive as an install input. |
| `Legends of the Frost - Facegen (Snow Elves) 2.4.7z` | `Legends of the Frost - Facegen (Snow Elves) 2.4.7z_54879af0-ac8d-498d-8921-c70080fc3001` | The `.wabbajack` manifest references this archive as an install input. |
| `Legends of the Frost - Facegen CC 2.4.7z` | `Legends of the Frost - Facegen CC 2.4.7z_e7c989b3-4a88-4b2b-9c74-f744d3a74548` | The `.wabbajack` manifest references this archive as an install input. |

Validation command for each missing artifact:

```bash
curl -fI 'https://build.wabbajack.org/authored_files/download/<munged-authored-file-id>'
```

The upstream producer to fix this is the Legends of the Frost/Wabbajack
authored-files publishing workflow. It must either restore those exact authored
files or publish a new `.wabbajack` artifact whose manifest references available
authored-file IDs. modde cannot proceed truthfully without the exact files
because the archive hashes and directive inputs are fixed by the manifest.

## Local Recovery Attempt

On 2026-04-27, the three authoritative metadata URLs were rechecked with
`curl -fI`. All three still returned `404 Not Found`.

Local recovery was attempted without substituting archives. The search covered:

- `/tmp/modde-lotf-plan`
- `/tmp/modde-lotf-data`
- `/home/can`
- `/data`

The search looked for the expected archive names and the exact modde store
filenames:

```text
b547423a91bc433a.archive
15f31c001fe701ce.archive
d75635216ba812cd.archive
```

No candidate files were found, and the three exact store paths under
`/tmp/modde-lotf-data/modde/store` are absent. Because there were no candidate
archives, `modde wabbajack import-archive` was not run against local files and
the synthetic install was not rerun. The blocker remains the missing upstream
authored-files inputs.

## Implemented Follow-Up Features

The previously identified follow-up features are now implemented:

| Feature | Status | Validation |
| --- | --- | --- |
| Wabbajack authored-file availability preflight | Implemented | `cargo test -p modde-sources wabbajack_authored_preflight` |
| Missing authored-artifact remediation output | Implemented | `cargo test -p modde-sources missing_authored_file_error` |
| User-supplied local archive import | Implemented | `cargo test -p modde-sources wabbajack_archive_import` and `cargo test -p modde-cli cli_wabbajack_import_archive` |
| Resume-friendly authored-file chunks | Implemented | `cargo test -p modde-sources authored_files_resume` |
| Synthetic late-stage Wabbajack fixture | Implemented | `cargo test -p modde-sources synthetic_wabbajack_pipeline` |

Local archive import is explicit and hash-gated:

```bash
XDG_DATA_HOME=/tmp/modde-lotf-data \
nix develop . -c cargo run -q -p modde-cli -- wabbajack import-archive \
  '/tmp/modde-lotf-plan/Legends of the Frost.local-game-hashes.wabbajack' \
  /path/to/Legends-of-the-Frost-DynDOLOD-Output-2.5.1.7z \
  /path/to/Legends-of-the-Frost-Facegen-Snow-Elves-2.4.7z \
  /path/to/Legends-of-the-Frost-Facegen-CC-2.4.7z
```

The import command computes the Wabbajack archive hash and imports only files
whose hash is referenced by the manifest. Name-only matches are refused.

## Not Missing For The Current Blocker

The current blocker is not caused by these features:

- Nexus API authentication: validated as Premium and working.
- ModDB mirror resolution: verified by the LoTF probe.
- Wabbajack authored-files chunk download support: verified for available files.
- Local game-file source parsing: the synthetic probe rewrote hashes and used a
  copied fake game directory only to move past game-file validation.
- Direct CDN URL handling: the missing files return `404` at both direct CDN and
  metadata URLs.

## Next Practical Step

The remaining blocker is the missing upstream input itself. To continue the LoTF
probe, either:

1. Restore the three missing authored-files entries upstream.
2. Publish a newer LoTF `.wabbajack` manifest that references available authored
   files.
3. Import exact local copies of the three archives with `modde wabbajack
   import-archive`, then rerun the isolated synthetic probe.

## Codeberg Issue Update Draft

The current modde branch now handles the previously observed LoTF blockers:

- `WabbajackCDNDownloader+State, Wabbajack.Lib` parses and downloads through
  Wabbajack's authored-files chunk metadata.
- Authored CDN downloads resume via `.part` plus `.part.json` sidecar state.
- `ModDBDownloader` is implemented as generic HTML mirror resolution feeding the
  normal direct downloader; the Skyrim Realistic Overhaul ModDB entries resolve
  and download in the sandbox probe.
- Nexus downloads work through the normal config-file API key path.
- The current LoTF blocker is upstream data availability: three authored-files
  archives referenced by the manifest return `404` from
  `https://build.wabbajack.org/authored_files/download/<munged-id>`.

Missing authored-files IDs:

```text
Legends of the Frost - DynDOLOD Output 2.5.1.7z_d08ce1cb-b109-4d6f-890d-5fa4e1b5241d
Legends of the Frost - Facegen (Snow Elves) 2.4.7z_54879af0-ac8d-498d-8921-c70080fc3001
Legends of the Frost - Facegen CC 2.4.7z_e7c989b3-4a88-4b2b-9c74-f744d3a74548
```
