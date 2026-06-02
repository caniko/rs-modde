# Phase 01 — Fix the `modde-windows` cross-build (`PowrProf.h` case mismatch)

> **Recommended Codex model: GPT 5.5 medium**
>
> This is a focused fix in one repo, but a genuinely tricky one: it sits at the
> intersection of nixpkgs mingw cross-compilation, the `cc-rs` build-script env
> protocol, and crane's `buildDepsOnly` phase semantics, with three plausible
> fix strategies to choose between and a long local build to verify each. That
> is complex work in a sub-agent/leaf role where a wrong-but-plausible fix burns
> a real release. A smaller model would likely "fix" it in a way that compiles
> the dummy `buildDepsOnly` source but still misses the real unrar translation
> units, or pick the brittlest of the three strategies. `medium` effort is
> enough — the search space is bounded once the mechanism is understood.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Single repo, single file
(`flake.nix`) plus possibly a new patch under `nix/patches/`. No dependency on
other phases. This is the critical-path blocker; everything in Wave 2+ waits on
it.

## Goal

`nix build .#modde-windows` completes successfully on a case-sensitive Linux
filesystem, producing `modde.exe` + `modde-ui.exe`, with no
`PowrProf.h: No such file or directory` (or any other missing-header) error
from the vendored unrar C++ compiled inside the `modde-windows-deps`
derivation.

## Why this matters now

The Windows cross-build is the **primary hard blocker** for the 0.2.1 release.
Run 198 (head `8a042f1`, the very commit that *added* the current fix attempt)
failed:

```
> cargo:warning=In file included from vendor/unrar/qopen.cpp:1:
> cargo:warning=vendor/unrar/os.hpp:54:10: fatal error: PowrProf.h: No such file or directory
>    54 | #include <PowrProf.h>
> error occurred in cc-rs: command did not execute successfully … x86_64-w64-mingw32-gcc-wrapper-15.2.0/bin/x86_64-…
error: Cannot build '/nix/store/…-modde-windows-deps-0.2.1.drv'. Reason: builder failed with exit code 101.
error: Build failed due to failed dependency
```

unrar's `os.hpp:54` does `#include <PowrProf.h>` (capitalised). mingw-w64 ships
the header lowercase as `powrprof.h`. On Windows (case-insensitive FS) this is
fine; on nix (case-sensitive) the include fails. The current workaround at
`flake.nix:346-352` creates case-corrected symlinks and exports
`CFLAGS_x86_64_pc_windows_gnu` / `CXXFLAGS_x86_64_pc_windows_gnu` with
`-I$PWD/.mingw-case-headers` from `preBuild` — but it is **ineffective**: the
failing derivation is `windowsCargoArtifacts = craneLib.buildDepsOnly
windowsArgs` (`flake.nix:357`), and the include flag is not reaching the
`cc-rs`-driven `x86_64-w64-mingw32-g++` invocation that compiles unrar there.

Until this builds, no release run can pass the "Build release artifacts" step
(build order: x86_64 → aarch64 → **windows** → darwin → appimage → flatpak).

## Outcome — 2026-06-02

The `PowrProf.h` case-mismatch workaround (`flake.nix:346-352`) **holds** —
`.#modde-windows` no longer fails on any unrar/mingw header. The blocker actually
encountered was different: an **incomplete `db.rs` → `db/` module refactor left
untracked**. `crates/modde-core/src/db/{mod,backend,migrate,tests}.rs`,
`crates/modde-cli/src/commands/config.rs`, and
`crates/modde-core/tests/postgres_parity.rs` were never `git add`-ed, so the
flake's git-filtered `lib.fileset.toSource` (`flake.nix:182`) omitted them, leaving
`lib.rs`'s `pub mod db;` with no backing file → `error[E0583]: file not found for
module db`. Working tree compiled fine (cargo reads on-disk files). **Fix:** staged
the new source files (no commit). `nix build .#modde-windows -L --no-link` then
exits 0 with `modde.exe`/`modde-ui.exe`/`modde-xtask.exe` + `libmcfgthread-2.dll`.
Handed to Phase 02.

## Out of scope

- Do **not** disable/skip the Windows target to "fix" the release. Windows is a
  shipped asset; it must build.
- Do **not** touch the SIGTERM/runner issue (Phase 03), secrets (Phase 04), or
  the simit generator (Phase 05).
- Do **not** re-tag or trigger any CI run — that's Phases 06/07.
- Do **not** bump the version or edit `Cargo.*`.

## Plan

1. **Reproduce.** From the repo root: `nix build .#modde-windows -L 2>&1 | tee
   /tmp/win-build.log`. Confirm the same `PowrProf.h: No such file or directory`
   from a `vendor/unrar/*.cpp` translation unit inside `modde-windows-deps`. If
   it now builds, STOP — the blocker is gone; record that and hand back to
   Phase 02.
2. **Understand why the current workaround misses.** Read `flake.nix:196-205`
   (the `unrar-ng-sys` cargo patch), `flake.nix:240-356` (windows target,
   `windowsArgs`, `preBuild`, `windowsTargetSuffix`), and `flake.nix:357`
   (`buildDepsOnly`). Inspect the failed builder log:
   `nix log /nix/store/…-modde-windows-deps-0.2.1.drv` and confirm whether the
   `x86_64-w64-mingw32-g++` command line carries `-I…/.mingw-case-headers`.
   Likely causes (verify, don't assume):
   - `cc-rs` reads `CXXFLAGS_<target>` but the value set in `preBuild` isn't in
     the environment of the actual `cargo`/`cc` invocation (phase ordering, or
     crane's depsOnly runs the compile before/around `preBuild`, or a subshell).
   - `cc-rs` is keying off the hyphenated triple form
     (`CXXFLAGS_x86_64-pc-windows-gnu`) and the underscored export isn't matched
     in this `cc` version, or the build uses `HOST_*`/`TARGET_*`-prefixed vars.
   - The unrar build script clears/overrides flags, so env CXXFLAGS never apply.
3. **Choose a fix strategy** (pick the most robust, justify in the commit):
   - **(A) Patch unrar `os.hpp`** to include the header case-insensitively
     (e.g. `#include <powrprof.h>` or a guarded alias), extending the existing
     `nix/patches/unrar-ng-sys-target-windows-cross.patch`. Most direct —
     removes the case dependency entirely; no reliance on the include-shim
     reaching `cc-rs`. Mirror it for the other capitalised headers the shim
     currently aliases (`Sddl.h`, `Wbemidl.h`) if unrar includes them.
   - **(B) Route the case-headers include via a channel `cc-rs` honours in
     `buildDepsOnly`** — e.g. set it on `windowsArgs` itself (not just
     `preBuild`) as `env`/`CXXFLAGS_x86_64_pc_windows_gnu` and the hyphenated
     form, or via `NIX_CXXFLAGS_COMPILE_x86_64_w64_mingw32` / the mingw
     `cc`-wrapper, so the dummy-source depsOnly compile and the real compile both
     see it. Confirm the symlink dir is created in a path that exists in the
     depsOnly sandbox.
   - **(C) Provide the capitalised headers on the compiler's default search
     path** (a derivation that symlinks the capitalised names into an
     `include/` dir added to the mingw sysroot/`-isystem`), so no per-target env
     var is needed.
4. **Implement** the chosen strategy with the smallest viable change. Keep the
   existing `unrar-ng-sys` patch and the mingw thread/link wiring intact.
5. **Verify locally** (the authoritative gate): `nix build .#modde-windows -L`
   succeeds; `ls -l result/bin/` shows `modde.exe`, `modde-ui.exe` (and
   `libmcfgthread-2.dll` per the `postInstall`). Run twice (once cold for the
   deps drv, once to confirm reproducibility / no IFD flakiness).
6. **Sanity-check no collateral damage:** `nix build .#modde -L` (x86_64 Linux)
   still builds, and `nix flake check` (or at least `nix eval .#modde.version`)
   is unaffected.
7. **Commit** on `trunk` (or a topic branch the maintainer merges) with a
   message explaining the case-mismatch root cause and why the chosen strategy
   reaches `buildDepsOnly`. Do **not** push a tag.

## Acceptance criteria

- [ ] `nix build .#modde-windows -L` exits 0 with no `PowrProf.h` (or other
      missing-header) error in the build log.
- [ ] `result/bin/` contains `modde.exe` and `modde-ui.exe`.
- [ ] `nix build .#modde -L` (x86_64 Linux) still succeeds — no regression to
      the native build from the change.
- [ ] The fix is durable across `buildDepsOnly` and `buildPackage` (a clean
      `nix build .#modde-windows` from a GC'd/cold deps drv succeeds, not just an
      incremental rebuild).
- [ ] Change committed; no tag pushed; `flake.nix` `nixConfig`/version untouched.

## Files likely touched

- `/data/nvme0/can/Projects/rs-modde/flake.nix` — windows build args
  (`windowsArgs`/`preBuild`/`buildDepsOnly` wiring), lines ~196-360.
- `/data/nvme0/can/Projects/rs-modde/nix/patches/unrar-ng-sys-target-windows-cross.patch`
  — if strategy (A) is chosen, extend this patch to fix the include casing.

## Pitfalls

- **"It builds incrementally but not cold."** `buildDepsOnly` compiles unrar
  against a dummy source set; an env var that only lands in `buildPackage`'s
  phase leaves the deps drv broken. Symptom: deps drv fails, package drv never
  runs. Recovery: verify from a cold deps drv (`nix store delete` the deps path
  or build on a fresh checkout), and set the include on `windowsArgs` so both
  derivations inherit it.
- **`cc-rs` env-var form mismatch.** `cc-rs` looks up `<VAR>_<target>` trying
  the literal triple (`x86_64-pc-windows-gnu`) and the underscored
  (`x86_64_pc_windows_gnu`) form, plus `TARGET_<VAR>`/`<VAR>`. Set both forms
  (or use the patch strategy) rather than guessing which one this `cc` honours.
- **Fixing the wrong header.** unrar may include several capitalised Windows
  headers; the shim already aliases `Sddl.h`/`Wbemidl.h`. If you patch only
  `PowrProf.h`, the next translation unit fails on the next one. Grep
  `vendor/unrar/*.{hpp,cpp}` for `#include <[A-Z]` and handle all of them.
- **`$PWD` in `preBuild` points at the wrong dir.** The symlink dir is created
  relative to the source root at `preBuild` time; if crane's compile runs in a
  different working dir, an absolute path is required. Strategy (A)/(C) avoids
  this entirely.
- **Long build masks success/failure.** A cold windows cross-build is several
  minutes; don't interpret an early `cargo:warning` line as fatal — read to the
  final `error:`/`Finished`.

## Reference

- Failure evidence: run 198 job log, `modde-windows-deps-0.2.1.drv` builder
  exit 101, `os.hpp:54` `PowrProf.h` (fetch with the basic-auth log command in
  the plan README).
- Current workaround: `flake.nix:346-352` (`.mingw-case-headers`), applied to
  `windowsCargoArtifacts = craneLib.buildDepsOnly windowsArgs` at `flake.nix:357`.
- unrar cargo patch: `nix/patches/unrar-ng-sys-target-windows-cross.patch`.
- Next phase that depends on this: [02-verify-all-cross-targets.md](./02-verify-all-cross-targets.md).
