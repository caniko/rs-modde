# Phase 06 — Remove `cargo-release` from the devShell and finalise docs

> **Recommended Codex model: gpt-5.4-mini, effort medium**
>
> Two-line flake edit, three-line TODO update, README/CONTRIBUTING wording tweaks.
> Leaf node — all prior phases have proven the new path; this is closing out the
> deprecation. mini is right-sized; medium effort buys the care to grep for stray
> references before deleting the dev-shell entry.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Depends on **Phase 04** (xtask backend swap) and
**Phase 05** (CI smoke job). Do not start this phase until at least one tag has been
cut through `cargo xtask release` on `trunk` — that's the in-production proof that
the simit path works, and the precondition for removing the fallback.

## Goal

The `cargo-release` package is removed from rs-modde's devShell, all in-repo
references to it are gone (except in the historical
`docs/planning/xtask-tooling-cli-uplift/DECISION.md` and
`docs/planning/simit-integration/DECISION.md` which record the migration), the
top-level [TODO.md](../../TODO.md) is updated to reflect that the simit integration
is complete, and CONTRIBUTING.md / README.md release sections accurately describe
the simit-driven flow without leftover cargo-release wording.

After this phase, `nix develop` no longer ships cargo-release; anyone who needs it
can `cargo install cargo-release` themselves, but the project considers it
unsupported tooling.

## Why this matters now

cargo-release lingered through Phase 02-05 as a safety net. By the time this
phase runs, a real release has flowed through simit successfully (acceptance
criterion below). Keeping the unused tool in the devShell:

- Adds build/closure size to every contributor's `nix develop` for no benefit.
- Implies cargo-release is a supported fallback, which it isn't.
- Risks the "two ways to do it" failure mode where new contributors pick the
  wrong path.

Deleting it now closes the migration cleanly. The rollback path remains
trivial: revert this commit.

## Out of scope

- Deleting `release.toml` — Phase 04 already did that.
- Modifying `harbor_xtask` to remove its `run_release` helper. That helper still
  has out-of-tree consumers; the rs-modde decision is just to not call it.
  Removal would be a rs-harbor PR.
- Adding new simit-driven xtask subcommands. The integration is done at this
  point; further enrichment is a separate effort.
- Removing the prior `xtask-tooling-cli-uplift/DECISION.md` even though its
  primary decision (cargo-release only) is now reversed. Historical decisions
  stay as the record of *why* the project moved the way it did; the new
  `simit-integration/DECISION.md` from Phase 05 is the current state of record.

## Plan

1. **Confirm the precondition** — at least one tag has been cut via simit on
   `trunk` since Phase 04 landed:
   ```
   git tag --sort=-creatordate | head -5
   git log --format='%H %s' -1 -- CHANGELOG.md
   ```
   If no simit-cut tag exists yet, **stop and cut one first** (or wait for the
   next natural release). This phase is the post-cutover cleanup, not the
   cutover itself.
2. **Remove `cargo-release` from the devShell** in
   [flake.nix:720](../../flake.nix#L720). Find the line, delete it, leave
   surrounding whitespace clean.
3. **Sweep for stray references**:
   ```
   rg -n 'cargo-release\|cargo_release\|cargo release' \
     --glob '!target' --glob '!docs/planning' --glob '!flake.lock'
   ```
   Anything in `crates/`, CONTRIBUTING.md, README.md, or .forgejo/ should be
   updated to reference simit or removed entirely. Allowed exceptions:
   - `docs/planning/` historical docs.
   - `flake.lock` if it still has a transitive reference (it shouldn't after
     the next `nix flake update`, but the lock entry is harmless if present).
4. **Update [README.md](../../README.md)**: if the README has an installation or
   release section that mentions cargo-release, rewrite it to point at `cargo
   xtask release`. Keep wording short — one paragraph.
5. **Update [CONTRIBUTING.md](../../CONTRIBUTING.md)**: the "Release tooling"
   section added in Phase 05 should already be authoritative. Verify nothing
   contradicts it and remove any obsolete cargo-release mentions elsewhere in
   the file.
6. **Update [TODO.md](../../TODO.md)**: find the simit-integration line items (if
   any exist) and mark them done. Add a one-line entry under a "Completed
   migrations" or equivalent section pointing at
   `docs/planning/simit-integration/`.
7. **Rebuild the devShell** to ensure nothing else depends on cargo-release
   being present:
   ```
   nix develop --command bash -c 'which simit; which cargo-release || echo gone'
   nix develop --command cargo xtask release patch --dry-run -m "post-cutover smoke"
   ```
   First command should print simit's path and `gone`. Second must exit 0.
8. **Commit**:
   ```
   git add flake.nix README.md CONTRIBUTING.md TODO.md
   git commit -m 'chore: drop cargo-release from devshell after simit cutover'
   ```

## Acceptance criteria

- [ ] At least one git tag with bare-semver format (e.g. `0.1.1` or `0.2.0`) exists on `trunk` and predates this commit — confirms the simit path has produced a real release.
- [ ] `cargo-release` is absent from the devShell packages list in [flake.nix](../../flake.nix).
- [ ] `rg cargo-release crates/ Cargo.toml flake.nix CONTRIBUTING.md README.md TODO.md 2>/dev/null` returns no matches.
- [ ] `nix develop --command cargo-release --version` and `nix develop --command cargo release --version` both fail with `command not found`.
- [ ] `nix develop --command cargo xtask release patch --dry-run -m "post-cutover smoke"` exits 0.
- [ ] `nix flake check --keep-going --print-build-logs` exits 0.
- [ ] [TODO.md](../../TODO.md) reflects the completed integration (entry added or existing entry marked done).
- [ ] [README.md](../../README.md) and [CONTRIBUTING.md](../../CONTRIBUTING.md) describe `cargo xtask release` as *the* release entry point, with no contradictory cargo-release wording.
- [ ] Commit message is `chore: drop cargo-release from devshell after simit cutover`.

## Files likely touched

- [flake.nix](../../flake.nix) — remove the `cargo-release` entry from the devShell packages list.
- [README.md](../../README.md) — release section wording (likely a single line/paragraph).
- [CONTRIBUTING.md](../../CONTRIBUTING.md) — purge any stale cargo-release references that contradict the Phase 05 "Release tooling" section.
- [TODO.md](../../TODO.md) — mark the simit-integration line done; cross-reference the plan dir.

## Pitfalls

- **Phase 06 before a real release**: the precondition matters. If the project gets impatient and merges this before a simit-cut release exists, a regression in the simit path would leave no fallback. Don't skip step 1.
- **Hidden `cargo-release` callers in scripts**: there's a [scripts/](../../scripts/) directory with `deploy-pages.sh`, `update-3077-fixture.sh`, etc. Grep them in step 3 — they currently don't reference cargo-release per the inventory at plan time, but verify in case something new landed.
- **flake.lock leftover**: removing the `cargo-release` package from the devShell does not by itself drop transitive references in `flake.lock`. They'll get pruned on the next `nix flake update`; until then, the closure is unchanged. Don't run `nix flake update` for this purpose alone — the cost (cache miss across the world) isn't worth a few KB of lock-file noise.
- **TODO.md format**: rs-modde's [TODO.md:121-122](../../TODO.md#L121-L122) uses GFM checkbox lists per section. Stick to the existing convention; don't introduce a new format.
- **Don't delete the `xtask-tooling-cli-uplift/DECISION.md`**: it documents the *previous* decision. Deleting it would lose the record of why the project once chose cargo-release. The contradiction with the new DECISION.md is informative, not a problem.

## Reference

- Phase 04 (the swap): [04-swap-xtask-release-backend.md](./04-swap-xtask-release-backend.md)
- Phase 05 (CI hook + DECISION.md): [05-wire-simit-into-ci.md](./05-wire-simit-into-ci.md)
- devShell call site: [flake.nix:713](../../flake.nix#L713) — `rs-harbor.lib.mkDevShells`.
- Prior decision (now reversed; kept for record): [docs/planning/xtask-tooling-cli-uplift/DECISION.md](../xtask-tooling-cli-uplift/DECISION.md)
- New decision (current state of record): [docs/planning/simit-integration/DECISION.md](./DECISION.md)
