# Phase 09 — Dependency audit: machete / audit / deny / msrv

> **Recommended Codex model: GPT 5.5 medium**
>
> Tool-driven but not mechanical: running the four tools is trivial, but
> *triaging* their output is judgment work — deciding whether an unused-dep
> report is a real removal or a `cfg`/feature/macro false positive, whether an
> advisory needs a bump or a `deny.toml` exception, and whether an MSRV finding
> is real. `low` would rubber-stamp false positives; `medium` is right. Leaf role.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Touches only `Cargo.toml`
files and possibly `deny.toml` — disjoint from the code phases; safe in Wave 0
alongside 01 and 02.

## Goal

The workspace passes a clean dependency-hygiene gate: `cargo machete` reports no
unused dependencies (or each remaining report is justified), `cargo deny check`
and `cargo audit` pass, and `cargo msrv` confirms the declared `rust-version =
"1.85"` is accurate — with any genuine findings fixed (unused deps removed,
advisories bumped or explicitly waived).

## Why this matters now

`rust-ultra` deferred the ToolDriven dependency concerns because the tools were
not installed locally. They are now installed (`cargo install cargo-deny
cargo-audit cargo-machete cargo-msrv` succeeded this session). The repo already
ships a `deny.toml` and `about.toml`, and the release workflow runs deny/audit in
CI — so this phase aligns the local/CI gate and removes any dead weight from the
large `[workspace.dependencies]` table (serde, tokio, reqwest, rusqlite, git2,
image, zip, sevenz, zstd, keyring, …).

## Out of scope

- Upgrading dependencies for features (only bump if an advisory or MSRV requires).
- Changing `deny.toml` policy beyond adding a justified, commented exception for a
  specific advisory that can't be bumped.
- `cargo update` / `Cargo.lock` churn unrelated to a finding.
- Adopting new dependencies (that's a separate concern, not requested).

## Plan

1. **Unused deps.** Run `cargo machete` (or `cargo +nightly udeps --workspace` if
   you prefer and nightly is available). For each reported unused dependency,
   verify it's truly unused — `rg` the crate name across that crate's `src`,
   accounting for `#[cfg(feature=...)]`, macro-only use, and `dev-dependencies`.
   Remove genuinely unused entries from the relevant `Cargo.toml`
   (`[dependencies]` or `[workspace.dependencies]`). Leave (and note) any
   false-positive that machete can't see through.
2. **Advisories.** Run `cargo audit`. For each advisory: bump the dependency if a
   compatible fixed version exists and the workspace still builds + tests green;
   otherwise add a commented, dated `[advisories] ignore` entry to the audit
   config (or `deny.toml`) explaining why it's unexploitable here.
3. **deny.** Run `cargo deny check` (uses the existing `deny.toml`). Resolve
   license/ban/advisory findings consistently with the existing policy (the repo
   already allows `BSD-2-Clause` and limits deny to policy checks — match that
   intent). Do not loosen policy to silence a real problem.
4. **MSRV.** Run `cargo msrv verify` (or `cargo msrv find` if verify is
   unavailable) against the workspace. Confirm `rust-version = "1.85"` in the root
   `Cargo.toml` is correct given edition 2024 (which itself mandates ≥1.85). If a
   dependency or code feature actually requires newer, bump `rust-version` and say
   so; if 1.85 holds, just record the confirmation.
5. After any `Cargo.toml`/`Cargo.lock` change, run the full gate. Commit
   (`chore(deps): ...` or `build(deps): ...`), with the audit/deny/machete/msrv
   outcomes summarized in the body.

## Acceptance criteria

- [ ] `cargo machete` reports zero unused dependencies, or every remaining report
      is annotated in the commit body as a verified false positive with the reason.
- [ ] `cargo audit` exits clean (no un-waived advisories); any waiver is a dated,
      commented entry.
- [ ] `cargo deny check` exits 0.
- [ ] `cargo msrv verify` confirms the toolchain floor matches `rust-version`
      (or `rust-version` is updated to the proven floor).
- [ ] `cargo build --workspace` and `cargo test --workspace` still green after any
      dependency change.

## Files likely touched

- `crates/*/Cargo.toml` and/or root `Cargo.toml` `[workspace.dependencies]`
  (unused-dep removals, any advisory bump).
- `Cargo.lock` (only as a consequence of a justified bump).
- Possibly `deny.toml` / audit config (only for a justified, commented waiver).

## Pitfalls

- **Symptom:** removing a dep breaks a `#[cfg(feature = "...")]`-gated build.
  **Cause:** machete can miss feature-gated/macro/`build.rs` usage. **Recovery:**
  build with `--all-features` and `--no-default-features` before trusting a
  removal; `rg` for the crate path including in macros and attributes.
- **Symptom:** an advisory has no fixed version. **Cause:** upstream unpatched.
  **Recovery:** assess exploitability in this app's context; waive with a dated
  comment rather than force an incompatible bump.
- **Symptom:** `cargo msrv` rebuilds the world repeatedly. **Cause:** it bisects
  toolchains. **Recovery:** use `cargo msrv verify` (single check against the
  declared version) rather than a full `find`.

## Reference

- Existing policy: `deny.toml`, `about.toml`, and the release workflow's deny/audit
  job (recent commits `f319c8c`, `823d4e9`, `f07aaa8`).
- MSRV claim: root `Cargo.toml` `rust-version = "1.85"`, `edition = "2024"`.
- Runs in Wave 0 with [01](./01-perf-zip-and-filter.md) and
  [02](./02-observability-library-output.md).
