# simit integration decisions

## simit owns versioning; rs-modde keeps bespoke CI

`cargo xtask release` is the maintainer entry point for releases, but the version bump, changelog edit, commit, tag, and release planning logic live in `simit release`. We keep that release path because it standardizes semver handling and changelog updates, while leaving rs-modde's CI and flake wiring under direct project control.

## We do not run `simit init-ci --check`

rs-modde's Forgejo CI is intentionally bespoke. The generator in `simit/src/render/ci.rs` emits a generic `ci.yaml`, `publish-crate.yaml`, and optional `release-artifacts.yaml`; it does not model this repository's project-specific jobs and policies. Running `simit init-ci --check` here would report intentional customization as drift.

In particular, the generated workflow does not preserve the current rs-modde CI shape:

- It does not build `.#flatpak-manifest`, `.#appimage-cli`, `.#appimage-ui`, `.#modde-windows`, or `.#docs`.
- It does not push build closures to Attic.
- It does not run `nix flake check --keep-going --print-build-logs` as rs-modde does today.
- It defaults Forgejo jobs to `codeberg-small`; rs-modde runs on the self-hosted `atlas` runner.
- It runs plain `cargo test` inside `nix develop`; rs-modde requires `cargo xtask coverage --ci`.

Because the generator is not the source of truth for this repository's CI, `simit init-ci --check` would be a permanent false-positive. We do not suppress that noise and we do not reshape bespoke CI to satisfy the generator.

## We do not run `simit init-flake --check`

rs-modde's `flake.nix` is intentionally rs-harbor-driven and heavily customized. It wires in rs-harbor toolchains, cross-compilation, website/docs/site outputs, Flatpak and AppImage packaging, Windows artifacts, and a simit-pinned devShell. The `init-flake` path in simit targets a much narrower crane-based flake template plus generated git-hook wiring, and `--check` treats that generated structure as the expected baseline.

That makes `simit init-flake --check` the wrong contract for this repository. The flake is not a vanilla simit-managed template, and making it one would discard behavior we rely on.

## Revisit triggers

Revisit this decision only if one of these conditions becomes true:

- simit gains enough configurability to express rs-modde's extra jobs, packages, Attic push, coverage command, and runner selection without local patching;
- rs-harbor publishes a helper such as `mkSimitCi` that makes simit-compatible CI generation preserve the current project requirements; or
- rs-modde deliberately simplifies its CI and flake so the generated simit defaults become the intended source of truth.

Until one of those happens, rs-modde keeps simit for release orchestration and keeps CI/flake generation out of simit's self-check path.
