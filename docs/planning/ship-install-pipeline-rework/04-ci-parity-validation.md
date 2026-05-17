# Phase 4 — Verify CI parity locally before push

> **Recommended Codex model: GPT 5.5 low**
>
> Four commands, run in sequence, in a known shell. No design, no
> file edits expected. If a step fails, the recovery is "open a
> follow-up commit" — but the *diagnostic* step is straightforward
> because the four steps map 1:1 to a single failing command in CI.
> A `low` model is sufficient and cost-appropriate.
>
> Routed as **leaf × trivial** in the `gpt-plan-routing` matrix.

## Working tree

`/data/nvme0/can/Projects/rs-modde`

**Depends on Phase 3** — runs after the two-commit split is finalised.
The validation targets the post-split state.

## Goal

The four commands that
[.woodpecker/check.yml](../../../.woodpecker/check.yml) runs on the
Forgejo runner all exit 0 against the local tree. After this phase,
the push in Phase 5 is high-confidence to land green on CI without
requiring a back-and-forth fixup.

## Why this matters now

Woodpecker minutes on the atlas runner are bounded. Pushing a broken
trunk burns those minutes and trains commit-spam patterns (push,
fail, push fixup, fail, push fixup of fixup…). The local CI parity
run takes 5–15 minutes wall clock and catches the same failure modes
the runner would.

The four commands in
[.woodpecker/check.yml](../../../.woodpecker/check.yml) are:

```yaml
- nix develop --command cargo fmt --all -- --check
- nix develop --command cargo clippy --workspace -- -D warnings
- nix develop --command just coverage-ci
- nix develop --command cargo build --workspace --release
```

These are deliberately the same four that local validation runs.

## Out of scope

- Pushing the commits (Phase 5).
- Changing CI configuration. If `just coverage-ci` baseline behaviour
  is wrong, file a follow-up — don't tune `FAIL_UNDER` mid-phase.
- Running the broader test suite beyond what CI runs. The full
  `cargo test --workspace --tests --no-fail-fast` is a nice-to-have
  but not part of the CI gate.
- Re-validating against the pre-split working tree. If Phase 3
  produced an empty index, this phase's run is meaningless — return
  to Phase 3 first.

## Plan

1. Enter the dev shell. The user's memory says to use `--impure`
   when osxcross is on:
   ```
   nix develop --impure
   ```
   On a cold cache this can take ~15 minutes; on a warm cache,
   seconds.
2. Confirm the shell has `cargo-llvm-cov` available (Phase 3's
   commits installed nothing new in the devShell — it was already
   wired by the prior coverage-tooling work):
   ```
   command -v cargo-llvm-cov
   ```
3. Run the four CI commands in sequence — the same order as
   `.woodpecker/check.yml`. Inside the dev shell:
   ```
   cargo fmt --all -- --check
   cargo clippy --workspace -- -D warnings
   just coverage-ci
   cargo build --workspace --release
   ```
   Track elapsed wall time per step — a step that runs >5× slower
   than expected often indicates a cache miss (recompile from
   scratch), not a correctness issue.
4. Capture `just coverage-ci`'s reported percentage. `FAIL_UNDER` is
   currently 0 (per Phase 2's reconciled state), so the gate passes
   trivially, but the *measured* baseline informs the optional
   Phase-2 follow-up of setting a real threshold.
5. If any step fails:
   - **fmt:** unexpected — Phase 3 finished with `cargo fmt --check`
     clean. Likely a hook or merge artifact. Run `cargo fmt --all`,
     commit as a `chore(fmt)` follow-up.
   - **clippy:** also unexpected, since the worktree was clean on
     `-D warnings` pre-split. Read the warning, decide if it's a
     `#[allow]` or a fix. Land as a fixup commit.
   - **coverage-ci:** check the `FAIL_UNDER` env var. If `coverage-ci`
     wraps `cargo llvm-cov` with a percentage gate, the failure
     means coverage dropped below the gate. Land a follow-up that
     either adds tests (preferred) or lowers `FAIL_UNDER` with a
     justification.
   - **cargo build --release:** most likely a `#[cfg(...)]`
     macro-expansion difference between `debug` and `release`
     profiles (often around `tracing` macros or `dbg!`). Fix
     forward.
6. Optional: also run `cargo test --workspace --tests --no-fail-fast`
   to confirm the `1,563 / 0 / 1` baseline still holds after the
   split. CI does *not* run this (the check.yml above is the entire
   surface), but it's cheap insurance.

## Acceptance criteria

- [ ] `cargo fmt --all -- --check` exits 0 with no output.
- [ ] `cargo clippy --workspace -- -D warnings` exits 0 with no
      `warning:` lines.
- [ ] `just coverage-ci` exits 0; the printed coverage % is recorded
      for the optional Phase 2 follow-up.
- [ ] `cargo build --workspace --release` exits 0 and produces
      `target/release/modde` (or whatever the bin name is) without
      missing-symbol errors.
- [ ] *(Optional)* `cargo test --workspace --tests --no-fail-fast`
      reports `1,563 passed / 0 failed / 1 ignored`.

## Files likely touched

None expected. If a step fails, a fixup commit may touch the file
named in the failure — but fixup commits land *after* the two
planned commits, never amending them.

## Pitfalls

- **Cold `nix develop`.** First-time entry can take 15+ minutes on
  a fresh shell. Symptom: looks hung. Recovery: be patient. Use
  `nix log <path>` from another terminal to monitor.
- **`just coverage-ci` is slow.** llvm-cov instrumentation roughly
  doubles compile time. A first run on the post-split state will
  recompile most of the workspace. Recovery: budget 5–10 minutes.
- **Confusing "no warnings" with "warnings as errors".** `clippy
  -D warnings` makes warnings fatal *and silent* on success. A
  clean exit is correct; don't expect "passing" output.
- **`cargo build --release` triggers a separate target dir if
  `CARGO_TARGET_DIR` isn't set.** Different from the `check` /
  `test` artefacts. Symptom: looks like everything recompiles.
  This is normal — release builds use `target/release/`, debug
  uses `target/debug/`.
- **`cargo-llvm-cov` not on PATH.** If step 2 fails, the dev shell
  isn't active. Re-enter with `nix develop --impure`.

## Reference

- CI config: [.woodpecker/check.yml](../../../.woodpecker/check.yml).
- Coverage recipe: [justfile](../../../justfile) (`coverage-ci`
  recipe).
- Memory pin reminder: `/home/can/.claude/projects/-data-nvme0-can-Projects-rs-modde/memory/feedback_impure_osxcross.md`
  — keep osxcross on, use `--impure` for nix commands.
- Companion phases: [03](./03-commit-split.md) (prerequisite),
  [05](./05-push-and-monitor.md) (next).
