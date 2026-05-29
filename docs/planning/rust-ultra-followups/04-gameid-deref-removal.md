# Phase 04 — Type safety: remove `GameId`/`ModId`'s `Deref<str>` and thread the newtypes

> **Recommended Codex model: GPT 5.5 max**
>
> Frontier complexity at an orchestrator role. The `GameId`/`ModId` newtypes
> exist but a `Deref<Target=str>` impl lets raw `&str` flow through ~170
> signatures across all six crates; removing it turns every silent coercion into
> a compile error you must resolve one-by-one with a judgment call (is this site a
> genuine game id, or an unrelated string that happened to coerce?). It's a
> compile-driven cascade with a real chance of mis-typing a boundary or
> introducing a swap bug while "fixing" errors. Mediocre work ships a subtle
> regression here; the blast radius and the per-site judgment justify `max`. This
> phase **runs solo** (Wave 2) — it conflicts with every other phase.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. **This phase must run alone**
— it edits `game_id`/`mod_id` call sites in `modde-core`, `modde-sources`,
`modde-games`, `modde-cli`, and `modde-ui`. Do not run it concurrently with any
other phase; everything downstream (05, 06, 07) rebases on its result.

## Goal

`GameId` and `ModId` are real types that stop silently decaying to `&str`:
function signatures that mean "a game id" take `&GameId`/`GameId` (or
`impl AsRef<GameId>`), the `Deref<Target=str>` impl on the id-newtype macro is
gone, and the only `&str`↔id conversions are explicit `.as_str()` / `GameId::from`
at genuine boundaries (parsing, serialization, external APIs). Passing an
arbitrary `&str` (a profile name, a tool id) where a game id is expected becomes a
**compile error**. No observable behavior changes — this is purely a typing
tightening.

## Why this matters now

The type-safety audit's headline finding: `GameId`/`ModId` are defined in
`crates/modde-core/src/resolver/mod.rs:90-103` via a `define_id_newtype!` macro
that impls `Deref<Target = str>` (around lines 57-62). Because of that `Deref`,
`&profile.game_id` coerces to `&str` everywhere, so functions take `&str` and the
newtype evaporates at the call boundary — ~170 `game_id: &str|String` signatures
vs ~25 typed, ~52 raw `mod_id` vs ~15 typed, with **36 decay sites** and **27
`GameId::from` re-wrap sites**. The worst are adjacent same-typed params that are
trivially transposable:

- `cli/commands/tool.rs:234/444/495/514/576/649/689` — `(tool_id: &str, game_id: &str)`.
- `core/db.rs:1682/1898/1932/1973/1987` — `(game_id: &str, tool_id: &str)` (opposite
  order from tool.rs — a caller bridging the two layers can swap them and it compiles).
- `core/db.rs:516/685` — `load_profile/delete_profile(name: &str, game_id: &str)`.
- `games/backup.rs:146` — `restore_plugin_order(profile: &str, game: &str)`.

Removing `Deref` is the lever that converts all of these from "compiles, maybe
wrong" to "must be the right type".

## Out of scope

- `ModId` for Nexus numeric ids (`mod_id: u64`/`file_id: u64`) — that's Phase 05
  (`NexusModId`/`NexusFileId`). This phase is the *string* game/mod ids only.
- `EnabledMod` `Option<String>` → enum storage — Phase 06.
- Hash newtype (`Xxh64`/`ContentHash`) — recommend-only, not this phase.
- Any logic, control-flow, or message change. Pure type threading.

## Risk profile

- **R1 — Mis-typed boundary.** A `&str` that is *not* a game id (profile name, tool
  id, path segment) gets "fixed" by wrapping it in `GameId` to silence a compile
  error, hiding a real type confusion instead of surfacing it.
- **R2 — Swap latent bug made permanent.** While fixing `(tool_id, game_id)` vs
  `(game_id, tool_id)` sites you could "fix" the compile error by reordering args
  at a call site to match, masking an existing transposition rather than correcting it.
- **R3 — Serialization/DB drift.** `GameId` round-trips through serde/rusqlite as a
  string; if you change how it's stored/parsed you can silently change on-disk
  profile/DB data. The stored representation must stay byte-identical.
- **R4 — Macro fallout.** `define_id_newtype!` may impl traits (`Borrow`, `AsRef`,
  `Display`, `Hash`, serde) that other code relies on; removing `Deref` may break
  `HashMap<GameId, _>` key lookups done with `&str`.
- **R5 — Cascade fatigue.** Hundreds of compile errors; the temptation is to
  blanket-`.as_str()` everything, which defeats the purpose (the point is to make
  the *signatures* typed, not to sprinkle `.as_str()` at call sites).

## Strategy

Commit ladder (each step compiles + tests green before the next; cheap to revert):

1. **Inventory commit (no behavior):** add `as_str()`/`AsRef<str>`/`Borrow<str>`
   to the macro if not present, so explicit conversion is available *before*
   removing `Deref`. Keep `Deref` for now. Verify green.
2. **Convert signatures crate by crate, leaf-first:** start in `modde-core`
   (resolver/db/profile/settings), changing `game_id: &str` → `game_id: &GameId`
   on functions that genuinely take a game id, adding `.as_str()` only at true
   external boundaries (SQL bind, serde, format strings, external APIs). Compile
   `-p modde-core` green, commit. Then `modde-games`, `modde-sources`,
   `modde-cli`, `modde-ui` — one crate per commit where feasible.
3. **Remove `Deref<Target = str>`** from `define_id_newtype!`. This is the
   destructive step — it produces the remaining compile errors. Fix each by
   *typing the signature*, not by wrapping at the call site. Decide per error
   whether the value is truly a game id (type it) or not (leave as `&str` — that's
   a site the newtype correctly *doesn't* cover). Verify the full workspace green.
4. **Audit the swap-prone signatures** (`tool.rs`, `db.rs` lists above): once both
   params are typed (`&GameId` vs `&ToolId`/`&str`), confirm no call site was
   silently reordered to compile; a real transposition should now be a type error,
   not a successful reorder.

Revert cost: steps 1-2 are additive (revert = drop the commit). Step 3 is the
risky one — if it spirals, `git reset --hard` to the step-2 tip and reassess scope
(you can ship the typed signatures *without* removing `Deref` as a partial win).

## Rollback drill

Before step 3 (the `Deref` removal), practice the abort:

```
git tag pre-deref-removal            # mark the safe point (step-2 tip)
# ... attempt the Deref removal ...
# if it spirals:
git reset --hard pre-deref-removal   # back to typed-signatures-with-Deref
cargo check --all-targets            # must be green within ~30s
```

SLA: you must be able to get back to a green `pre-deref-removal` tree in under one
minute. If step 3's error count is unmanageable, ship steps 1-2 (typed signatures,
`Deref` retained) as the deliverable and record the `Deref` removal as still-open.

## Failure modes and recoveries

- **F1 — `HashMap<GameId, V>` lookups by `&str` break** after `Deref` removal.
  *Symptom:* `the trait Borrow<str> is not implemented`. *Cause:* code did
  `map.get(some_str)` relying on `Borrow<str>` via `Deref`. *Recovery:* keep an
  explicit `impl Borrow<str> for GameId` (or look up with `GameId::from`/`&GameId`).
- **F2 — serde representation changes.** *Symptom:* a profile/DB round-trip test
  fails, or stored strings change. *Cause:* you altered the macro's `Serialize`/
  `Deserialize`/`Display`. *Recovery:* the on-disk form must be the bare string;
  diff a serialized profile before/after.
- **F3 — `.as_str()` sprawl.** *Symptom:* signatures still `&str`, call sites
  littered with `.as_str()`. *Cause:* fixing errors at call sites instead of
  typing signatures. *Recovery:* push the type *into* the signature; `.as_str()`
  belongs only at SQL/serde/format/external-API boundaries.
- **F4 — Genuine `&str` mis-wrapped as `GameId`.** *Symptom:* a profile name or
  tool id is now a `GameId`. *Cause:* silencing R1. *Recovery:* if the value isn't
  a game id, leave the param `&str`; that site is correctly outside the newtype.

## Acceptance criteria

- [ ] `define_id_newtype!` no longer impls `Deref<Target = str>` for the id types
      (`rg -n 'impl.*Deref' crates/modde-core/src/resolver/mod.rs` shows none for
      the id newtypes).
- [ ] Functions that take a game id take `&GameId`/`GameId`/`impl AsRef<GameId>`,
      not `&str`; the same for mod ids. The swap-prone signatures in `tool.rs` and
      `db.rs` are typed so a transposition is a compile error.
- [ ] `.as_str()`/`GameId::from` appear only at genuine boundaries (SQL bind,
      serde, format strings, external-API calls) — not scattered at internal call sites.
- [ ] On-disk representation unchanged: a serialized profile and a DB row store the
      bare game-id string exactly as before (add/keep a round-trip test).
- [ ] `cargo clippy --all-targets --workspace -- -D warnings`, `cargo check
      --all-targets`, and `cargo test --workspace` all green (≥ 1609 passed, 0 failed).

## Files likely touched

All crates. Concentrations: `modde-core/src/{resolver/mod.rs (the macro),
db.rs, profile/mod.rs, settings.rs, backup.rs}`, `modde-games/src/{registry.rs,
tools/mod.rs, backup.rs, and per-game modules}`, `modde-sources/src/{nexus/*,
scanner.rs, manifest/wabbajack.rs}`, `modde-cli/src/commands/{tool.rs, install.rs,
profile.rs, game.rs}`, `modde-ui/src/app.rs` and views.

## Reference

- The macro + id defs: `crates/modde-core/src/resolver/mod.rs:90-103` (and the
  `Deref` impl ~57-62).
- Type-safety audit finding #1 (rust-ultra design stage) — the leak/re-wrap counts
  and the swap-prone signature list.
- This phase gates [05](./05-nexus-id-newtypes.md), [06](./06-enabledmod-typed-enums.md),
  and [07](./07-app-rs-decomposition.md). Run it solo (README Wave 2).
