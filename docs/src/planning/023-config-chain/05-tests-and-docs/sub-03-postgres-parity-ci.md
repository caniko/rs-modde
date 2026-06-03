# Phase 05 · Sub-layer 03 — run the PostgreSQL parity suite in CI

> **Recommended Codex model: GPT 5.5 medium**
>
> Leaf-level CI wiring, but it touches real infrastructure: a Forgejo Actions job
> on the self-hosted atlas runner with a PostgreSQL service/container, the right
> runner label, and the Attic cache conventions. That infra specificity (and the
> need to make the job *fail* when the suite silently no-ops) puts it at
> `medium`, not `low`. No design ambiguity; fully independent of every code phase.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. **Independent of all code
phases** — the parity test file already exists; this only makes it run. Can be
pulled into Wave 0. Touches CI workflow files only — disjoint from the other
sub-layers. (Note: `.forgejo/workflows/release.yml` is bespoke and hand-hardened
— do **not** regenerate it; add a *separate* CI workflow or a job, edited
targeted.)

## Goal

CI provisions a PostgreSQL server, sets `MODDE_TEST_PG_URL`, and runs
`crates/modde-core/tests/postgres_parity.rs` against it on every push/PR — and the
job **fails** if `MODDE_TEST_PG_URL` is set but zero parity tests actually
executed (so the suite can't silently no-op again).

## Why this matters now

`postgres_parity.rs` early-returns when `MODDE_TEST_PG_URL` is unset
(lines 88-93), and no CI workflow provisions PostgreSQL — so the suite compiles
but **never runs** anywhere (G10). The multi-backend guarantee (RETURNING id,
BOOLEAN round-trip, ON CONFLICT upsert, ON DELETE CASCADE, the discrete-field
path after Phase 01) is therefore unverified in CI; a backend regression would
ship green.

## Out of scope

- Editing the bespoke `release.yml` (hand-hardened; see project memory
  "release.yml is bespoke — don't simit-regen").
- The Rust test bodies (sub-01) and CLI tests (sub-02).
- Adding new parity assertions (the existing ones suffice; extend only if Phase
  01's discrete path warrants a discrete-connection parity case — optional).

## Plan

1. **Pick the runner + PG provisioning.** This repo's CI runs on the self-hosted
   atlas Forgejo runner (labels `atlas` for cargo-in-container, `atlas-nix-trusted`
   for nix-store writes; Attic cache `https://attic.candee.baby/canix`). Add the
   parity job to the existing CI workflow (the test/clippy/fmt one), **not**
   `release.yml`. Provision PostgreSQL either via a service container
   (`postgres:16`) or a `nix run nixpkgs#postgresql` ephemeral cluster in the job,
   whichever the atlas runner supports cleanly. Mirror the `MODDE_TEST_PG_URL`
   one-liner documented at the top of `postgres_parity.rs`.
2. **Wire the env + run.** Export `MODDE_TEST_PG_URL=postgres://modde:modde@
   localhost:5432/modde` (matching the spun-up server) and run `cargo test -p
   modde-core --test postgres_parity` (default features, so `postgres` is on).
3. **Guard against silent no-op.** The current test prints "skipping" and returns
   when the URL is unset. Make CI fail if the var is set but the suite didn't
   actually run a parity test — e.g. run with `--no-fail-fast` and assert via
   `cargo test ... -- --format json` / a grep that the parity tests reported
   `ok` (non-zero count), or add a tiny guard test that `panic!`s if
   `MODDE_TEST_PG_URL` is set but a sentinel wasn't reached. Document the chosen
   mechanism in the workflow.
4. **Health-gate the server.** Wait for `pg_isready` before running tests so the
   job doesn't flake on a not-yet-ready server.
5. **Keep it green offline.** Local `cargo test` without `MODDE_TEST_PG_URL` must
   still pass (the early-return path) — don't make the parity test mandatory
   outside CI.

## Acceptance criteria

- [ ] A CI workflow job starts PostgreSQL, sets `MODDE_TEST_PG_URL`, and runs the
  parity suite on push/PR; the job is green on a healthy run and the parity tests
  report a non-zero passed count.
- [ ] If `MODDE_TEST_PG_URL` is set but no parity test executes, the job **fails**
  (proven once by temporarily renaming the test fn / breaking the gate).
- [ ] `release.yml` is untouched; the parity job lives in the CI workflow.
- [ ] Local `cargo test -p modde-core` without the env var still passes (parity
  early-returns).

## Files likely touched

- `.forgejo/workflows/<ci-workflow>.yml` (the existing test/clippy workflow, or a
  new `postgres-parity.yml`) — **not** `release.yml`.

## Pitfalls

- **Symptom:** the parity job is green but ran zero parity tests. **Cause:** the
  env var didn't reach the test process, or the early-return path was taken.
  **Recovery:** the silent-no-op guard (step 3); verify the test log shows the
  parity test names with `ok`.
- **Symptom:** flaky "connection refused". **Cause:** tests ran before PostgreSQL
  was ready. **Recovery:** `pg_isready` poll / service health check before the
  test step.
- **Symptom:** the job accidentally edits or duplicates `release.yml`. **Cause:**
  reusing the release workflow as a template. **Recovery:** add a dedicated CI
  job/workflow; leave the bespoke release pipeline alone.
- **Symptom:** runner can't pull `postgres:16` / no container runtime. **Cause:**
  the bare atlas runner image lacks it. **Recovery:** use the nix ephemeral
  postgres cluster on `atlas-nix-trusted`, or the runner's supported service
  mechanism (consult the atlas-runner conventions).

## Reference

- Phase README + merge plan: [README.md](./README.md).
- The test that needs a server: `crates/modde-core/tests/postgres_parity.rs`
  (the `MODDE_TEST_PG_URL` contract + the podman one-liner are documented at its top).
- Atlas runner + Attic conventions: the `atlas-runner` / `forgejo-atlas-ci` skills.
- Do-not-touch: `.forgejo/workflows/release.yml` (project memory: bespoke).
