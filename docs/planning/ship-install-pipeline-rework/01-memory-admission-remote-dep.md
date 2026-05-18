# Phase 1 — Convert `memory-admission` to a remote dep

> **Recommended Codex model: GPT 5.5 medium**
>
> The edit itself is one line in
> [crates/modde-sources/Cargo.toml](../../../crates/modde-sources/Cargo.toml)
> plus a Cargo.lock refresh. What lifts this above `low` is the
> cross-repo coordination: the local sibling
> `/data/nvme0/can/Projects/rs-memory-admission` may have unpushed
> commits, and pinning to a non-existent rev produces a `cargo fetch`
> error during CI rather than during this phase. A `low`-tier model
> tends to skip the "push the sibling first / verify version parity"
> sub-steps and produces a broken pin. `medium` has the headroom for
> the cross-repo dance plus the judgement call on whether to also wire
> rs-memory-admission as a flake input (defer if not blocking, per
> step 8).
>
> Routed as **leaf × moderate** in the `gpt-plan-routing` matrix.

## Working tree

`/data/nvme0/can/Projects/rs-modde`

Touches the sibling repo at `/data/nvme0/can/Projects/rs-memory-admission`
for read-only verification and optionally a `git push` to its
`ssh://git@codeberg.org/caniko/rs-memory-admission.git` origin.

## Goal

A fresh clone of `rs-modde` (CI runner, outside contributor, the
Forgejo Actions job on atlas) can run `cargo build --workspace` with
no sibling repos on disk. The `memory-admission` crate resolves via
git fetch from codeberg.org/caniko/rs-memory-admission at a pinned
revision that matches the local sibling's behaviour.

## Why this matters now

`crates/modde-sources/Cargo.toml` currently contains:

```toml
memory-admission = { path = "../../../rs-memory-admission", default-features = false, features = ["async"] }
```

The Forgejo Actions CI workflow runs the Cargo checks inside `nix develop`:

```yaml
- nix develop --command cargo fmt --all -- --check
- nix develop --command cargo clippy --workspace -- -D warnings
- nix develop --command just coverage-ci
- nix develop --command cargo build --workspace --release
```

All four fail at the `cargo` step on a fresh checkout because the
`../../../rs-memory-admission` directory doesn't exist on the runner.
Every push to trunk would go red until this is fixed.

The blast radius is also wider than CI: any contributor who clones
`rs-modde` (humans or future agents) hits the same wall.

## Out of scope

- Publishing `memory-admission` to crates.io. (Possible follow-up,
  but a git dep is enough to unblock CI and contributors.)
- Refactoring the `memory_admission::weighted::*` call sites in
  [crates/modde-sources/src/wabbajack/installer.rs](../../../crates/modde-sources/src/wabbajack/installer.rs).
  The dep migration is API-preserving.
- Adding any *new* features to memory-admission. If a missing feature
  is discovered (e.g., `async` not exported correctly remotely), file
  it as a follow-up; do not block this phase on it.
- Setting up a flake input + crane override (deferred to a follow-up
  in the same phase only if a CI/Nix reproducibility issue is hit).

## Plan

1. Verify the local sibling's state vs its remote:
   ```
   git -C /data/nvme0/can/Projects/rs-memory-admission status
   git -C /data/nvme0/can/Projects/rs-memory-admission log --oneline origin/main..HEAD
   ```
   If there are unpushed commits, push them first
   (`git -C ... push origin main`). The pin must reference a commit
   reachable from the public origin.
2. Capture the target commit SHA:
   ```
   git -C /data/nvme0/can/Projects/rs-memory-admission rev-parse origin/main
   ```
3. Verify the sibling's published version matches what
   `Cargo.lock` records (`0.1.6`):
   ```
   grep '^version' /data/nvme0/can/Projects/rs-memory-admission/Cargo.toml
   ```
   If mismatched, bump the sibling's version + commit + push before
   pinning.
4. Edit [crates/modde-sources/Cargo.toml](../../../crates/modde-sources/Cargo.toml):
   ```diff
   -memory-admission = { path = "../../../rs-memory-admission", default-features = false, features = ["async"] }
   +memory-admission = { git = "https://codeberg.org/caniko/rs-memory-admission.git", rev = "<sha-from-step-2>", default-features = false, features = ["async"] }
   ```
   Use `https://` not `ssh://` so CI containers without an SSH key
   can fetch.
5. Refresh the lockfile:
   ```
   cargo update -p memory-admission
   ```
   `cargo` will rewrite the `[[package]]` entry from a bare path entry
   to a git-source entry (you should see `source = "git+https://...#<sha>"`
   appear).
6. Verify resolution and behaviour:
   ```
   cargo check --workspace --all-targets
   cargo test --workspace --tests --no-fail-fast
   ```
7. Independence test — temporarily rename the sibling and re-check:
   ```
   mv /data/nvme0/can/Projects/rs-memory-admission /data/nvme0/can/Projects/rs-memory-admission.away
   cargo check --workspace --all-targets   # must still succeed
   mv /data/nvme0/can/Projects/rs-memory-admission.away /data/nvme0/can/Projects/rs-memory-admission
   ```
   This proves CI will succeed.
8. *Deferred unless blocking:* add `rs-memory-admission` as a flake
   input + crane override in
   [flake.nix](../../../flake.nix) for Nix-build reproducibility.
   Skip if step 7 passes; revisit only if a `nix develop` run hits a
   network-fetch issue.

## Acceptance criteria

- [ ] `grep 'path = ' crates/modde-sources/Cargo.toml` returns no
      matches for `memory-admission` (the only path dep currently in
      the worktree).
- [ ] `Cargo.lock` records a `source = "git+https://codeberg.org/caniko/rs-memory-admission.git?rev=<sha>#<sha>"`
      line under the `memory-admission` package.
- [ ] `cargo check --workspace --all-targets` exits 0 with
      `/data/nvme0/can/Projects/rs-memory-admission` renamed away
      (step 7).
- [ ] `cargo test --workspace --tests --no-fail-fast` reports the
      baseline **1,563 passed / 0 failed / 1 ignored**. Any deviation
      indicates the remote pin doesn't match local behaviour.

## Files likely touched

- [crates/modde-sources/Cargo.toml](../../../crates/modde-sources/Cargo.toml) — 1 line
- [Cargo.lock](../../../Cargo.lock) — `[[package]] memory-admission` entry gains a `source = "..."` line
- *(deferred)* [flake.nix](../../../flake.nix) — only if step 8 fires

In sibling repo `/data/nvme0/can/Projects/rs-memory-admission`:

- *Possibly:* `git push` only (no file edits expected)

## Pitfalls

- **Sibling has unpushed commits.** Symptom: step 6 `cargo` succeeds
  locally (path-style resolution still works because git deps fall
  through to registry on lookup), but CI errors with
  `error: failed to load source for dependency 'memory-admission'`
  or a fetch-rev failure. Recovery: step 1's push catches this; the
  independence test in step 7 also catches this.
- **Version mismatch.** The lockfile pins `0.1.6` but the public
  origin still ships `0.1.5`. Symptom: API drift compile errors in
  `installer.rs`. Recovery: bump the sibling's Cargo.toml version,
  commit, push, then re-pin.
- **Feature missing remotely.** `async` feature exists locally but
  was added in an unpushed commit. Symptom: `cargo` errors with
  "feature `async` does not exist". Recovery: again, push the sibling
  first.
- **`ssh://` URL in CI.** Forgejo runner doesn't have the user's SSH
  key. Symptom: `cargo fetch` hangs or fails with auth error.
  Recovery: always use the `https://` form in the pin.

## Reference

- Originating diagnosis: chat session of 2026-05-17 — local readiness
  audit identified the path dep as the only fresh-clone blocker.
- Sibling repo: `ssh://git@codeberg.org/caniko/rs-memory-admission.git`
  (HTTPS-equivalent: `https://codeberg.org/caniko/rs-memory-admission`).
- Forgejo Actions CI workflow:
  [.forgejo/workflows/ci.yml](../../../.forgejo/workflows/ci.yml) — the
  commands this phase unblocks.
- Call-site that needs the dep:
  [crates/modde-sources/src/wabbajack/installer.rs:290](../../../crates/modde-sources/src/wabbajack/installer.rs#L290)
  (`memory_admission::weighted::WeightedConfig`).
