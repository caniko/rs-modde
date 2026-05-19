# Phase 02 — Add `simit` to the rs-modde devShell as a pinned flake input

> **Recommended Codex model: gpt-5.4, effort medium**
>
> Touches the flake input graph, which is the project's most cross-cutting wiring
> (every devShell user, every CI job, every Nix package consumes this). Needs to
> preserve the rs-harbor `follows`-chain so we don't introduce a second nixpkgs
> evaluation. Sub-agent — bounded scope (one file, one lock update) but the
> blast radius of getting it wrong is the whole flake. mini at high would also
> work; 5.4/medium is the cheaper match for the judgment density.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Independent of Phase 01 (different files). No
prerequisite ordering — can run in parallel with Phase 01 and Phase 03.

## Goal

`simit` is reachable on `$PATH` from `nix develop` at a pinned, reproducible version
sourced from the simit flake at `git+https://codeberg.org/caniko/simit.git`. The flake
input shares rs-modde's already-pinned `nixpkgs` (via rs-harbor follows-chain) so no
second copy of nixpkgs lands in the lockfile. `simit --version` matches the upstream
release tag we pinned. The existing devShell `cargo-release` package stays in place —
Phase 04 removes it once the cutover is proven.

## Why this matters now

Every subsequent phase invokes `simit`. Until it's in the devShell, contributors and CI
must install it ad-hoc (`cargo install --git ... simit`), which defeats the
reproducibility we already have for every other tool in this repo. Phases 03, 04, and 05
all assume a known-good `simit` is one `nix develop` away.

The follows-chain matters: rs-modde's flake already routes
[nixpkgs/rust-overlay/crane/flake-utils through rs-harbor](../../flake.nix#L18-L21).
simit's own flake declares its own nixpkgs. Without
`inputs.simit.inputs.nixpkgs.follows = "rs-harbor/nixpkgs"`, every fetch will
double-instantiate nixpkgs — visible as a duplicated `nixpkgs_NN` entry in flake.lock
and a slower first-evaluation cost.

## Out of scope

- Removing `cargo-release` from the devShell (Phase 04 owns that).
- Replacing any xtask code paths that call cargo-release (Phase 04 again).
- Reshaping CHANGELOG.md or invoking `simit release` for real (Phase 03 / Phase 04).
- Using `simit init-flake` or `simit init-ci` against rs-modde — both would clobber the
  bespoke flake and workflows. Explicitly skipped; the decision is recorded in Phase 05.

## Plan

1. **Inspect simit's flake to confirm the input name and outputs**:
   ```
   nix flake metadata git+https://codeberg.org/caniko/simit.git
   nix flake show git+https://codeberg.org/caniko/simit.git
   ```
   Confirm: the package output is `packages.x86_64-linux.default` (matches simit's own
   [publish-crate workflow line](../../../../simit/.forgejo/workflows/ci.yaml#L31)).
   Confirm the input named `nixpkgs` exists at the top level (this is the follows
   target).
2. **Add the input to [flake.nix](../../flake.nix)** in the `inputs` block, alongside
   the existing `rs-harbor` / `nix-appimage` entries:
   ```nix
   simit = {
     url = "git+https://codeberg.org/caniko/simit.git";
     inputs.nixpkgs.follows = "rs-harbor/nixpkgs";
   };
   ```
   If simit's flake exposes additional inputs (rust-overlay, crane, flake-utils), add
   matching `inputs.<name>.follows = "rs-harbor/<name>"` lines for each one that exists
   upstream — verify via `nix flake metadata` output before adding, do not guess.
3. **Add `simit` to the outputs function argument list**:
   ```nix
   outputs = {
     self,
     rs-harbor,
     rs-harbor-macos-sdk-pin,
     simit,
     ...
   }: ...
   ```
4. **Add the binary to the devShell**: the existing devShell is built by
   `rs-harbor.lib.mkDevShells` at [flake.nix:713](../../flake.nix#L713). Find the
   `extraPackages` (or equivalent) parameter — read the rs-harbor source if unclear:
   ```
   grep -rn 'mkDevShells\|extraPackages\|packages = ' \
     /data/nvme0/can/Projects/rs-harbor/crates 2>/dev/null
   ```
   Append `simit.packages.${system}.default` to the existing list. Leave
   `cargo-release` in place.
5. **Update the lockfile**:
   ```
   nix flake update simit
   ```
   (Not a full `nix flake update` — we're adding one input, not refreshing the world.)
6. **Verify follows landed correctly**: inspect flake.lock for a `simit` node; confirm
   its `inputs.nixpkgs` resolves to the same node as `rs-harbor.inputs.nixpkgs`. Run
   `nix flake metadata --json | jq '.locks.nodes.simit.inputs'` to confirm.
7. **Smoke-test from devShell**:
   ```
   nix develop --command bash -c 'which simit && simit --version'
   nix develop --command bash -c 'cargo-release --version || cargo release --version'
   ```
   Both must succeed.
8. **Commit**:
   ```
   git add flake.nix flake.lock
   git commit -m 'feat(devshell): add simit binary alongside cargo-release'
   ```

## Acceptance criteria

- [ ] `flake.nix` contains a `simit` input with `inputs.nixpkgs.follows = "rs-harbor/nixpkgs"`.
- [ ] `flake.lock` has exactly one `simit` node whose `inputs.nixpkgs` references the same node as `rs-harbor.inputs.nixpkgs` (verify with `jq`).
- [ ] `nix develop --command which simit` resolves inside the dev shell.
- [ ] `nix develop --command simit --version` prints the pinned simit version, matching the upstream tag chosen in flake.lock.
- [ ] `nix develop --command cargo release --version` still works — Phase 04 has not landed yet.
- [ ] `nix flake check --keep-going --print-build-logs` exits 0.
- [ ] No new `nixpkgs_NN` entries in flake.lock beyond what existed pre-phase (count with `jq '.nodes | with_entries(select(.key | startswith("nixpkgs"))) | length' flake.lock` before and after).
- [ ] Commit message is `feat(devshell): add simit binary alongside cargo-release`.

## Files likely touched

- [flake.nix](../../flake.nix) — `inputs.simit`, outputs args, devShell packages list.
- `flake.lock` — auto-updated by `nix flake update simit`.

## Pitfalls

- **simit's flake may not name its input `nixpkgs`**. Some flakes use `nixpkgs-unstable` or `nixpkgs-stable`. Run `nix flake metadata git+https://codeberg.org/caniko/simit.git --json | jq '.locks.nodes.root.inputs'` first — the follows target name must match.
- **Double-instantiation symptom**: a `nixpkgs_2` (or similar) node appears in `flake.lock`. If you see it, the follows declaration is wrong — fix the input name in step 2 and re-run `nix flake update simit`.
- **rs-harbor `mkDevShells` may not accept arbitrary packages**. If the helper only takes a fixed param list, the workaround is to override the resulting shell with `pkgs.symlinkJoin` or to add `simit` via `nativeBuildInputs` at the devShell call site. Read the harbor source before assuming an API; do not invent a parameter that doesn't exist.
- **simit upstream might be on `main`, not `trunk`**. The git+https URL resolves to the default branch — confirm by checking `simit.locked.rev` in flake.lock and cross-referencing the commit graph at codeberg.org/caniko/simit. If a specific simit version is required (because Phase 03 needs the `## [Unreleased]` parser), pin a tag explicitly: `url = "git+https://codeberg.org/caniko/simit.git?ref=0.3.1"`.
- **macOS impure SDK**: rs-modde sets `--impure` for nix builds touching macOS SDK paths (see [memory: feedback_impure_osxcross.md](../../../../../home/can/.claude/projects/-data-nvme0-can-Projects-rs-modde/memory/feedback_impure_osxcross.md)). simit doesn't need that, but the verification commands above should retain `--impure` if any of them touch the cross-build path. The `nix develop` invocations in this phase do not.

## Reference

- rs-modde flake structure: [flake.nix](../../flake.nix) lines 14-25 (inputs), 41-50 (outputs args), 713 (`mkDevShells` call).
- simit upstream flake: `git+https://codeberg.org/caniko/simit.git`, see [simit/flake.nix](../../../../simit/flake.nix).
- rs-harbor `mkDevShells` source: in the rs-harbor workspace; search there before guessing the API.
- rs-harbor follows-chain pattern: [flake.nix:18-21](../../flake.nix#L18-L21).
