# Phase 06 — Type safety: store `EnabledMod` status/method as enums, not `Option<String>`

> **Recommended Codex model: GPT 5.5 high**
>
> Complex because of the data-migration risk: `EnabledMod` persists
> `install_status`/`install_method` as TOML/JSON-in-`String`, and tightening them
> to the existing enums means a profile/DB schema change that must read every
> *legacy* row correctly (including the documented "None = legacy Installed"
> case) without corrupting on-disk user data. A migration that silently drops or
> mis-maps a status is a real regression. `medium` underweights the persistence
> hazard; `high` is warranted. Orchestrator role over the profile/DB layer.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Run **after Phase 05**
(both edit `modde-core/src/profile/mod.rs` and `db.rs`). Solo within `modde-core`
+ a couple of CLI call sites.

## Goal

`EnabledMod` holds its install status and method as the existing typed enums
(`Option<InstallStatus>`, `Option<InstallMethod>`) and its tags as `Vec<String>`,
instead of stringly-typed `Option<String>` carriers that get re-parsed on every
read. Legacy on-disk profiles/DB rows still load correctly (the "absent status =
legacy Installed" rule is modeled explicitly, not conflated). An unparseable
status becomes a load-time error or an explicit legacy variant — not a value that
fails deep in use.

## Why this matters now

Type-safety audit finding #4. `EnabledMod` (`crates/modde-core/src/profile/mod.rs`
lines ~27/62/67/72) stores:

- `install_status: Option<String>` — but `InstallStatus` is a real enum with
  `as_str()`/`parse()` (`installer/types.rs:204-239`); it's written via
  `status.as_str().to_string()` (`install.rs:842`, `app.rs:2516`) and re-parsed via
  `InstallStatus::parse` (`db.rs:1678`, `deploy.rs:280`) — repeated stringify/reparse
  of a value that could just be the enum. The doc even says "`None` means legacy —
  treat as `Installed`", conflating absent with a real state.
- `install_method: Option<String>` — holds a TOML-serialized `InstallMethod` enum.
- `fomod_config: Option<String>` — holds a TOML-serialized `DeclarativeConfig`.
- `tags: Option<String>` — holds a JSON array of strings.

Any of these can hold a garbage string that only fails at use.

## Out of scope

- `GameId`/`ModId` (Phase 04) and `NexusModId`/`NexusFileId` (Phase 05).
- `fomod_config` — converting that `Option<String>` (TOML `DeclarativeConfig`) is
  optional; it's lower value and bigger. Do `install_status`, `install_method`,
  and `tags`; leave `fomod_config` unless trivial, and note the decision.
- Changing the SQLite column *types* unless required (you can keep TEXT columns and
  change only the in-memory type + the serialize/deserialize at the edge).

## Plan

1. Read `InstallStatus`/`InstallMethod` (`modde-core/src/installer/types.rs:204-239`)
   and every `EnabledMod` read/write site (`install.rs:842`, `app.rs:2516`,
   `db.rs:1678`, `deploy.rs:280`, plus serde for the profile file).
2. Change `EnabledMod` fields: `install_status: Option<InstallStatus>`,
   `install_method: Option<InstallMethod>`, `tags: Vec<String>`. Both enums already
   derive serde — lean on that.
3. **Migration / legacy reads (the careful part):**
   - For the profile file (serde): make deserialization accept the legacy string
     form. Use a custom `Deserialize` or `#[serde(with=...)]` that parses the old
     `Option<String>` (via `InstallStatus::parse`) into the enum, so existing
     profiles load. Decide the legacy-`None` policy explicitly — e.g. an
     `InstallStatus::Installed`-as-default at the *read* boundary, or keep `None`
     meaning "legacy" but document it on the typed field.
   - For the DB (`db.rs`): keep the TEXT column; on read, parse the stored string
     into the enum (error or legacy-default on unknown); on write, serialize the
     enum to the same string form it used before. The on-disk bytes must not change
     for existing values.
   - `tags`: parse the legacy JSON-array-`String` into `Vec<String>` on read;
     serialize back to the same JSON form on write (or migrate the column — but
     prefer same-on-disk).
4. Remove the now-redundant `InstallStatus::parse`/`as_str().to_string()` round
   trips at the call sites — they hold the enum directly now.
5. **Prove the migration:** add a test that loads a *legacy* profile (string
   status/method, JSON tags, and a `None`/absent status) and asserts it maps to the
   right enum values; and a round-trip test (write → read) that yields byte-identical
   stored representation for existing values.
6. Gate and commit (`refactor: typed EnabledMod status/method/tags`).

## Acceptance criteria

- [ ] `EnabledMod.install_status: Option<InstallStatus>`,
      `install_method: Option<InstallMethod>`, `tags: Vec<String>` (no
      `Option<String>` carriers for these three).
- [ ] A legacy profile/DB row (string `install_status`, TOML `install_method`, JSON
      `tags`, and an absent status) loads without error and maps to the correct
      typed values — covered by a new test.
- [ ] Round-trip (load → save) of an existing profile produces byte-identical
      stored strings (no silent on-disk migration of unchanged values) — covered by
      a test.
- [ ] The `InstallStatus::parse` / `as_str().to_string()` round trips at
      `db.rs:1678`, `deploy.rs:280`, `install.rs:842`, `app.rs:2516` are gone
      (values held as enums).
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` and
      `cargo test --workspace` green.

## Files likely touched

- `crates/modde-core/src/profile/mod.rs` (`EnabledMod` fields + serde),
  `crates/modde-core/src/db.rs` (column read/write mapping),
  `crates/modde-core/src/installer/types.rs` (maybe a `Deserialize` helper),
  `crates/modde-core/src/deploy.rs:280`.
- `crates/modde-cli/src/commands/install.rs:842`, `crates/modde-ui/src/app.rs:2516`.

## Pitfalls

- **Symptom:** existing user profiles fail to load after the change. **Cause:**
  deserialization no longer accepts the legacy string form. **Recovery:** the
  read path *must* accept both the new enum form and the old string form; test
  with a real legacy profile fixture.
- **Symptom:** stored values change on a no-op save. **Cause:** the enum serializes
  to a different string than `as_str()` produced. **Recovery:** make the
  serialization match the historical string exactly (or migrate deliberately and
  document it as a one-way migration with a version bump).
- **Symptom:** an unknown status string is silently dropped on read (`db.rs:1530`
  `_ => warn+skip` pattern). **Cause:** lenient parsing. **Recovery:** decide
  explicitly — error vs legacy-default — and make it visible, not a silent skip.

## Reference

- The enums: `crates/modde-core/src/installer/types.rs:204-239`
  (`InstallStatus`, `InstallMethod`).
- Read/write sites: `db.rs:1678`, `deploy.rs:280`, `install.rs:842`, `app.rs:2516`.
- Type-safety audit finding #4. Depends on [05](./05-nexus-id-newtypes.md).
