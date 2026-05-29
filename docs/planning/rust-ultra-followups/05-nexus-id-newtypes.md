# Phase 05 — Type safety: `NexusModId` / `NexusFileId` newtypes

> **Recommended Codex model: GPT 5.5 medium**
>
> Moderate, contained: two `u64` newtypes threaded through the nexus subsystem and
> a single `i64`↔`u64` conversion boundary at the rusqlite layer. The judgment is
> bounded (which `u64` is a mod id vs file id vs hash) and the blast radius is
> mostly `modde-sources/nexus` + a few profile/db/cli sites. `medium` holds it;
> the only sharp edge is the signed/unsigned DB boundary, which is local. Sub-agent
> role within a subsystem.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Run **after Phase 04**
(newtyped tree) and **after Phase 03** (nexus error types settled). **Shares
`modde-core/src/profile/mod.rs` and `db.rs` with Phase 06** — land 05 before 06.

## Goal

Nexus mod ids and file ids are distinct types (`NexusModId(u64)`,
`NexusFileId(u64)`), so they can't be transposed at a call site, and the single
place they cross the `u64`(API) ↔ `i64`(SQLite) boundary is one explicit
`TryFrom`/`to_i64` conversion at the rusqlite layer instead of scattered `as`
casts. Passing a file id where a mod id is expected becomes a compile error.
No behavior change; the same ids are stored and fetched.

## Why this matters now

Type-safety audit findings #2 and #3:

- **Swap risk:** `install_single_mod(.., mod_id: u64, file_id: u64, ..)` and
  `nexus_browser_url(game_name, mod_id: u64, file_id: u64)`
  (`modde-sources/src/wabbajack/acquire.rs:149`), plus `nexus/install.rs:60-61,
  198-199`, `nexus/cdn.rs:21-22`, and many `nexus/api.rs` signatures take two bare
  `u64`s. Swapping them compiles and yields a wrong-but-plausible store path
  (`format!("{game_domain}_{mod_id}_{file_id}")`) and a wrong CDN URL.
- **Lossy dual representation:** `EnabledMod.nexus_mod_id`/`nexus_file_id` are
  `Option<i64>` (`profile/mod.rs:31-33`, `scanner.rs:35-36`), while the
  API/source layer uses `u64`, reconciled only by unchecked `as` casts
  (`install.rs:839-840` `mod_id as i64`, `update.rs:58/175` `as u64`,
  `scanner.rs:139-140`). A negative `i64` from a corrupt row would wrap to a huge
  `u64`.

## Out of scope

- String game/mod ids (`GameId`/`ModId`) — that's Phase 04.
- Changing the DB column type (SQLite only has signed integers, so `i64` storage
  is forced — keep it; only centralize the conversion).
- `EnabledMod`'s `Option<String>` status/method/tags fields — Phase 06.
- A general hash newtype — recommend-only, not here.

## Plan

1. Define `NexusModId(u64)` and `NexusFileId(u64)` (in `modde-sources` nexus
   module, or `modde-core` if shared by profile/scanner — pick the crate both
   sides can import; likely `modde-core` since `EnabledMod` lives there). Derive
   `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize`, plus
   `Display` and `From<u64>`/`u64` accessor. Add `TryFrom<i64>` (reject negative)
   and a `to_i64()` for the DB boundary.
2. Thread the newtypes through the nexus API/CDN/install signatures and
   `nexus_browser_url`, so `mod_id`/`file_id` params are typed. Convert at the
   true external boundary (the HTTP URL / store-path `format!`) with the accessor.
3. Centralize the `i64`↔`u64` conversion at the rusqlite layer: one `TryFrom<i64>`
   on read (surfacing a corrupt negative row as an error, not a wrapped huge id)
   and one `to_i64()` on write. Remove the scattered `as i64`/`as u64` casts at
   `install.rs:839-840`, `update.rs:58/175`, `scanner.rs:139-140`.
4. Decide `EnabledMod`'s field type: either keep `Option<i64>` at the struct and
   convert at the DB edge, or hold `Option<NexusModId>` and convert only in the
   rusqlite mapping. Prefer holding the newtype in the struct and converting at the
   SQL bind/column-read, so the rest of the code is typed. **Keep the stored bytes
   identical** (same i64 value in the same column).
5. Gate and commit (`refactor: NexusModId/NexusFileId newtypes`).

## Acceptance criteria

- [ ] `NexusModId` and `NexusFileId` exist as distinct types; `install_single_mod`,
      `nexus_browser_url`, the CDN URL builder, and the nexus API signatures take
      the typed ids (a swap is a compile error).
- [ ] All `nexus_mod_id`/`nexus_file_id` `as i64`/`as u64` casts at the cited sites
      are replaced by the single `TryFrom<i64>`/`to_i64` boundary; `rg -n 'as i64|as u64'
      crates/modde-{sources,cli}/src` near those files shows them gone.
- [ ] A negative `i64` in the nexus-id column fails closed (test: a row with `-1`
      yields an error, not a wrapped `u64`).
- [ ] Stored representation unchanged: a profile/DB round-trip test shows the same
      i64 written and read back.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` and
      `cargo test --workspace` green.

## Files likely touched

- `crates/modde-core/src/profile/mod.rs` (the `nexus_mod_id`/`nexus_file_id`
  fields), `crates/modde-core/src/db.rs` (the i64↔u64 boundary), and wherever the
  newtypes are defined.
- `crates/modde-sources/src/nexus/{api.rs,cdn.rs,install.rs}`,
  `crates/modde-sources/src/wabbajack/acquire.rs:149`,
  `crates/modde-sources/src/scanner.rs:35-36,139-140`.
- `crates/modde-cli/src/commands/{install.rs:839-840, update.rs:58,175}`.

## Pitfalls

- **Symptom:** a stored profile's nexus id changes value. **Cause:** you altered
  the serde/DB representation of the newtype. **Recovery:** the wire/disk form must
  be the bare integer; serialize transparently (`#[serde(transparent)]` on the
  newtype).
- **Symptom:** `TryFrom<i64>` rejects a legitimate large id. **Cause:** nexus ids
  fit in `u64` but the DB holds `i64`; values above `i64::MAX` can't be stored as
  positive `i64` anyway. **Recovery:** real nexus ids are well within `i64::MAX`;
  reject only negatives.
- **Symptom:** swap "fixed" by reordering a call site. **Cause:** masking an
  existing transposition. **Recovery:** once typed, a swap must be a type error —
  fix the *definition/usage*, not by reordering to compile.

## Reference

- Type-safety audit findings #2 (`NexusModId`/`NexusFileId`) and #3 (i64↔u64).
- Depends on [04](./04-gameid-deref-removal.md); precedes
  [06](./06-enabledmod-typed-enums.md) (shared profile/db files).
