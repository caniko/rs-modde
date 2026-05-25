# Install Pipeline Rework

Multi-phase plan to rebuild modde's Wabbajack install pipeline so that
large modlists (Twisted Skyrim, Lorerim, Nordic Souls, Living Skyrim)
install in _slow-and-steady_ mode on commodity hardware: 16 GB RAM, no
swap thrash, predictable disk usage, no host-wide stalls.

The current pipeline can hit 30+ GB resident, 8 GB swap, 4 TB peak
working set, and multi-hour stalls on a 60 GB / 5 TB workstation. The
Wabbajack reference implementation does the same job in a few hundred
MB resident on Windows. This document is the gap analysis and the
remediation plan.

---

## Goals & success metrics

The plan is "done" when, on a 16 GB / 1 TB SSD reference box, installing
**Twisted Skyrim** (~6,000 archives, ~250 GB downloads, ~700 GB
deployed) satisfies:

| Metric                         | Current (observed)         | Target                   | Stretch           |
| ------------------------------ | -------------------------- | ------------------------ | ----------------- |
| Peak modde RSS                 | 30+ GB (cgroup-bounded)    | < 2 GB                   | < 512 MB          |
| Peak `7zz`/decoder RSS         | unbounded                  | n/a (native, in-process) | —                 |
| Peak disk over downloads       | +3.8 TB tmp                | + (downloads × 1.05)     | downloads only    |
| Apply throughput               | 12–230 dirs/min            | > 5,000 dirs/min         | > 20,000 dirs/min |
| Wall-clock for full install    | unbounded (never finished) | < 6 h                    | < 2 h             |
| Host swap usage during install | 11 GB peak                 | 0                        | 0                 |
| Cache fraction during install  | 75% of host RAM            | < 30%                    | < 10%             |
| Page-fault thrash events       | 1+ per session             | 0                        | 0                 |

These are the targets the rest of the document is justified by.

## Architectural reset

Before the phases, the four root causes we are correcting:

1. **Subprocess-per-file decompression.** modde shells out to `7zz` once
   per install directive. Twisted Skyrim has 681,301 directives. Each
   `execve` re-mmaps the archive, re-parses headers, and (for _solid_
   7z archives) re-decompresses everything before the requested file.
   Worst case: extracting 200 files from a solid archive does 200 full
   decompressions.
2. **Manifest-order traversal.** Directives are walked in the order
   the manifest emits them, which interleaves archives. The same
   archive is opened, decompressed, and closed thousands of times.
3. **`Vec<u8>` capture of decompressor stdout.** Every extracted entry,
   including 200 MB BSAs, is fully buffered in RAM before being
   `tokio::fs::write`-ten. Streaming benefit is lost.
4. **No reflink/hardlink awareness in deploy.** Stock Game is a
   25–35 GB copy of vanilla SSE+AE files. We `tokio::fs::copy` it
   even though source and destination usually share a filesystem.

Each phase below targets one of these. Order is by leverage: phase 1
alone already moves us from "dies overnight" to "runs to completion on
the reference box."

---

## Status legend

- ✅ **landed** — merged, tests passing on the dev machine
- 🟡 **in progress** — code written but not yet exercised end-to-end
- ⬜ **planned** — not started

## Phase 0 — Pause and stabilise

**Status: ✅ landed.** Install was killed cleanly, 3 TB free on
`/data/nvme0`, modde state preserved at
`/data/nvme0/can/modde-data/modde/{store,staging,downloads}`.

This phase exists so future operators know to stop the bleeding before
touching the codebase. Two reflexes:

- `systemctl --user stop run-p<PID>-i<INV>.scope` (works for any
  `systemd-run --user --scope` wrapper).
- `pkill -KILL -f 'target/release/modde install'`.

The downloaded archive store is durable across restarts. Killing only
loses the apply phase, which is the one we are about to rebuild.

---

## Phase 1 — Per-archive batching of apply directives

**Status: ✅ landed.** Per-archive batching is wired through
[crates/modde-core/src/manifest/wabbajack.rs](../crates/modde-core/src/manifest/wabbajack.rs)'s
`WabbajackManifest::install_directives_grouped_by_archive` and consumed
by [crates/modde-sources/src/wabbajack/installer.rs](../crates/modde-sources/src/wabbajack/installer.rs).
Archive-backed directives are now applied through batch-scoped native
extraction. Synthetic and fixture tests pass; full Twisted Skyrim
runtime metrics remain operational validation, not CI acceptance for
this pass.

**Goal:** every archive is opened _once_ during apply. All directives
that read from it run while the decoder is hot. Eliminates the
solid-archive amplification entirely.

**Why now:** zero new dependencies, biggest single throughput win,
foundation that every later phase depends on. Previously
[crates/modde-sources/src/wabbajack/installer.rs](../crates/modde-sources/src/wabbajack/installer.rs)'s
parallel apply walked `installs.iter().enumerate()`, which interleaved
archives.

### Design

```rust
struct ArchiveBatch<'m> {
    archive_hash: u64,
    archive_path: PathBuf,
    archive_size_bytes: u64,
    directives: Vec<&'m InstallDirective>, // sorted by inner_path
}

fn group_directives_by_archive<'m>(
    installs: &'m [InstallDirective],
) -> Vec<ArchiveBatch<'m>>
```

Apply pipeline becomes:

1. Group directives by `archive_hash` (tracking original index for
   progress reporting).
2. Process batches with bounded concurrency over **archives**, not
   directives. Default = `min(8, available_parallelism())`.
3. Within a batch: open decoder once, drive it forward through entries
   in order, write each output. Decoder dies with the batch.
4. `CreateBSA` directives are still a separate pass after FromArchive
   work that targets their `temp_id` is done.

### Files touched

- [crates/modde-sources/src/wabbajack/installer.rs](../crates/modde-sources/src/wabbajack/installer.rs)
  — replace `stream::iter(installs.iter().enumerate())` with batched stream.
- [crates/modde-core/src/manifest/wabbajack.rs](../crates/modde-core/src/manifest/wabbajack.rs)
  — add `WabbajackManifest::install_directives_grouped_by_archive`.

### Acceptance

- [ ] Each archive appears in `7zz` invocation list at most once per run.
- [x] Existing core/source test suites pass with no new skips.
- [x] Targeted Wabbajack test set passes:
      `nix develop . -c cargo test -p modde-sources wabbajack --no-default-features`.
- [x] New unit test: feed a manifest with N directives across M
      archives; assert exactly M archive batches.
- [ ] Peak RSS during apply drops to < 4 GB on Twisted Skyrim
      (measured with cgroup `memory.peak`).
- [ ] Apply throughput on Twisted Skyrim > 1,000 dirs/min on the dev
      machine.

### Estimated effort

1.5 days. Mostly mechanical — group, then thread the group through
the existing apply functions.

---

## Phase 2 — Native Rust decompression

**Status: ✅ landed.** `modde-sources::decompress` now exposes a
batch API:

```rust
ArchiveBatchExtractor::extract_selected(path, requests)
```

The API accepts `ArchiveRequest { directive_index, from, kind }`, where
`kind` is either direct file output or returned bytes for patch sources.
Default builds support zip, 7z, BSA, and BA2 with no external tools.
RAR is gated behind the `rar` feature and uses `unrar-ng`; without that
feature RAR is reported as an unsupported archive format instead of
falling back to a subprocess. The 7z reader uses
`sevenz_rust2::ArchiveReader::for_each_entries`, not `read_file`, so
solid archives are traversed once per batch.

**Goal:** replace the per-file `7zz e -so` subprocess with in-process
streaming decoders. One decoder per archive batch (phase 1), no
forking, no `Vec<u8>` capture of subprocess stdout.

**Why now:** with phase 1 in place, the per-archive decoder cost is
amortised across many directives, which makes the native crate
overhead (initialising LZMA2 state, etc.) cheap. Without phase 1,
moving to native crates without batching saves only the `execve`
overhead.

### Crate matrix

| Format    | Crate                                           | Notes                                                                                                |
| --------- | ----------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| zip       | already `zip = "*"`                             | Streaming per-entry already correct.                                                                 |
| 7z        | `sevenz-rust2`                                  | Pure Rust, no `unsafe`, supports LZMA/LZMA2/BCJ/BZip2/Deflate. The maintained fork of `sevenz-rust`. |
| rar       | `unrar-ng = "0.7.7"` behind crate alias `unrar` | Optional feature only; no subprocess fallback. Version `0.5.x` is yanked upstream.                   |
| BSA / BA2 | `modde_core::bethesda_archive` (already native) | Keep.                                                                                                |
| zstd      | `zstd = "0.13.3"`                               | Phase 7a compressed staging; built with `zstdmt`.                                                    |

7z is the dominant format on Nexus; covering it well is most of the win.

### Design

The batch extractor is intentionally batch-scoped, not per-entry:

```rust
ArchiveBatchExtractor::extract_selected(
    archive_path,
    &[ArchiveRequest {
        directive_index,
        from,
        kind: ArchiveRequestKind::WriteFile { to },
    }],
)?;
```

The streaming copy keeps memory at one ~1 MB buffer per active worker,
no matter how big the entry is.

### Files touched

- new: `crates/modde-sources/src/decompress/mod.rs`
- [crates/modde-sources/src/wabbajack/installer.rs](../crates/modde-sources/src/wabbajack/installer.rs)
  — replace `extract_one_streaming`, `extract_full_archive`, subprocess
  candidate lookup, and install-time full-archive temp extraction.
- [crates/modde-sources/Cargo.toml](../crates/modde-sources/Cargo.toml)
  — add `sevenz-rust2`, `unrar` (gated behind `rar` feature).

### Acceptance

- [x] No `7zz` / `7z` / `unrar` executable path remains in the Wabbajack
      apply path.
- [x] Default build compiles without external archive tools.
- [x] Unit fixtures cover zip batch extraction and traversal rejection;
      BSA/BA2 coverage remains in existing Bethesda archive tests.
- [ ] Peak RSS during apply drops to < 1 GB.
- [ ] Apply throughput on Twisted Skyrim > 5,000 dirs/min.

### Estimated effort

5 days. Most of the work is the test fixtures, not the wrappers — the
crates are mature.

---

## Phase 3 — Streaming I/O for large outputs

**Status: ✅ landed.** `FromArchive` requests write directly from the
native decoder to the final destination with a 1 MiB buffer. Patch
sources still become bytes because the existing patcher requires source
and patch buffers, but those bytes are bounded by the phase 6 cache
policy and scoped to patch work only.

**Goal:** any extracted entry over a small threshold is written directly
from the decoder to `tokio::fs::File` in 1 MB chunks. No intermediate
`Vec<u8>` for files of any size.

**Why now:** phase 2 already gives us `impl Read`; we just need to
plumb it to the writer instead of `read_to_end`-ing it into a vector.

### Design

```rust
const STREAM_THRESHOLD: u64 = 8 * 1024 * 1024; // 8 MiB
const COPY_BUFFER: usize = 1 << 20;            // 1 MiB

if entry.uncompressed_size > STREAM_THRESHOLD {
    let reader = archive.open_entry(&entry)?;
    let writer = tokio::fs::File::create(&output)?;
    tokio::io::copy(&mut tokio::io::BufReader::new(reader), &mut writer).await?;
} else {
    let mut buf = Vec::with_capacity(entry.uncompressed_size as usize);
    archive.open_entry(&entry)?.read_to_end(&mut buf)?;
    tokio::fs::write(&output, &buf).await?;
}
```

The small-file fast path is kept because `tokio::fs::write` is one
syscall and avoids the buffered-write overhead for sub-megabyte files,
which dominate by count (configs, ESPs).

### Files touched

- `crates/modde-sources/src/wabbajack/installer.rs::apply_from_archive`
- `crates/modde-sources/src/wabbajack/installer.rs::apply_patched_from_archive`
  — patch input is read once for `bsdiff`; same threshold rule.

### Acceptance

- [x] Archive writes use a fixed 1 MiB copy buffer.
- [ ] Extracting a 1 GB BSA never grows modde RSS by more than
      ~32 MiB (decoder window + copy buffer + slack).
- [ ] Patching a 500 MB source file uses < 200 MiB peak (bsdiff
      itself needs ~3× the source — see phase 6 to address).

### Estimated effort

0.5 days. Pure plumbing.

---

## Phase 4 — InlineFile zip index

**Status: ✅ landed.** `wabbajack::inline::InlineSource` opens the
`.wabbajack` zip once, builds a name index once, and serves reads
through a shared `Mutex<ZipArchive<File>>`. It is initialized lazily
only when `InlineFile` or patch blobs are present.

**Goal:** the `.wabbajack` zip is opened and its central directory
parsed exactly once at install start. Every InlineFile directive
reads from a long-lived `zip::ZipArchive` rather than re-opening the
file.

**Why:** Twisted Skyrim has 681,301 install directives, of which the
vast majority are `FromArchive` against the `.wabbajack` itself for
patches and inline data. Currently each directive opens and parses the
zip (the central directory is at the end of the file, so each open is
also a seek to EOF + read).

### Design

Replace the per-call `zip::ZipArchive::new(File::open(...))` inside
`apply_inline_file` and `apply_patched_from_archive` with a shared
`Arc<Mutex<zip::ZipArchive<File>>>` constructed once in `install()`
and passed to the apply pass.

For higher concurrency, pre-extract every InlineFile entry to a
memory-mapped store at install start (the `.wabbajack` is typically
< 5 GB, well under reasonable mmap limits). A `BTreeMap<String,
mmap::Range>` then services `apply_inline_file` without locking.

### Files touched

- `crates/modde-sources/src/wabbajack/installer.rs::WabbajackInstaller`
  — own a `Arc<InlineSource>` field.
- new: `crates/modde-sources/src/wabbajack/inline.rs` —
  `InlineSource::open(zip_path) -> Self` builds the index;
  `InlineSource::read(name) -> &[u8]` returns the slice.

### Acceptance

- [ ] `pidstat -d` shows the `.wabbajack` file is read at most twice
      during a complete install (once to index, once if mmap is
      ineligible).
- [ ] InlineFile-heavy apply phase throughput improves by an order of
      magnitude on lists with > 100k InlineFile directives.

### Estimated effort

1 day.

---

## Phase 5 — Hardlink / reflink Stock Game and deploy

**Status: ✅ landed.** `modde_core::link::{link_or_copy, LinkKind}` now
tries hardlink, then `reflink_copy::reflink`, then byte copy. Stock Game
snapshot and Wabbajack deploy callers use the helper and log the link
kind. Existing destination files are removed before linking.

**Goal:** Stock Game (a vanilla copy of SSE+AE living inside the
modlist) and the final deploy step never duplicate bytes when source
and destination share a filesystem.

**Why:** Stock Game is ~25–35 GB. Right now we `tokio::fs::copy`. On
the same filesystem that's gratuitous I/O and 25 GB of duplicate disk
allocation. Hardlinks (any POSIX FS) cost zero bytes; reflinks
(btrfs / xfs / bcachefs / APFS / ReFS) are CoW so destinations can
diverge without copying.

### Design

```rust
fn link_or_copy(src: &Path, dst: &Path) -> Result<LinkKind> {
    // 1. Same filesystem? -> hardlink (cheapest, always works)
    // 2. Same fs but operator opted out of hardlinks? -> reflink
    // 3. Different fs?    -> copy (only allocation that has to happen)
}

#[derive(Debug)]
enum LinkKind { Hard, Reflink, Copy }
```

Reflink syscall: `ioctl(FICLONE)` on Linux, `clonefile(3)` on macOS,
`CopyFileEx` with `COPY_FILE_REQUEST_BLOCK_REFCOUNT` on ReFS. The
`reflink-copy` crate covers all three.

For Stock Game specifically, the source is the user's Steam Skyrim
install. Hardlinking modifies the Steam install's link count but does
not modify the bytes. Steam's "verify integrity" still works
(unchanged inodes still hash to the same value). We document this.

### Files touched

- new: `crates/modde-core/src/link.rs` (move existing copy paths into
  a single API).
- `crates/modde-sources/src/wabbajack/installer.rs::apply_from_archive`
  — for GameFileSource directives whose `from` is a vanilla file,
  prefer `link_or_copy`.
- `crates/modde-sources/src/wabbajack/runner.rs::deploy_mo2_to_game`
  — same.
- [crates/modde-sources/Cargo.toml](../crates/modde-sources/Cargo.toml)
  — `reflink-copy = "0.1"`.

### Acceptance

- [ ] Twisted Skyrim's Stock Game folder consumes < 100 MB on disk
      when source and destination share a filesystem.
- [ ] `du --apparent-size` and `du` differ by ≥ 25 GB after Stock Game
      is created.
- [ ] Steam "verify integrity" still passes on the source install.
- [ ] Cross-FS path falls back to copy with an info log.

### Estimated effort

1.5 days. Most of the work is testing, not coding.

---

## Phase 6 — Bounded LRU file-byte cache

**Status: ✅ landed.** `modde-sources::cache::ByteLruCache` stores patch
source bytes as `bytes::Bytes`, keyed by `(archive_hash,
normalized_inner_path)`, with a default `MODDE_BYTE_CACHE_MIB=512`
budget. The cache is used only for patch source bytes.

**Goal:** memoise the bytes of recently-extracted entries, keyed by
`(archive_hash, inner_path)`, with a hard byte budget (default
512 MiB).

**Why:** PatchedFromArchive directives extract a source file then
patch it. That source is often the same vanilla DLL or texture used
as the basis for several patches. With phase 1 these are already in
the same batch but PatchedFromArchive workloads still benefit from
keeping the source bytes around for ~2–3 patches that follow it.

bsdiff also peaks at ~3× source size in RAM during patch application;
the cache lets the decoder release the source bytes early since the
cache holds a copy.

### Design

`crates/modde-sources/src/cache.rs`:

```rust
pub struct ByteLruCache {
    map: parking_lot::Mutex<lru::LruCache<(u64, String), Bytes>>,
    bytes_budget: AtomicU64,
    bytes_used: AtomicU64,
}

impl ByteLruCache {
    pub fn get_or_insert<F>(&self, key: Key, f: F) -> Bytes where F: FnOnce() -> Bytes;
    pub fn invalidate_archive(&self, archive_hash: u64);
}
```

`bytes::Bytes` is reference-counted, so cache entries can be cloned
into the apply path without deep-copying.

### Files touched

- new: `crates/modde-sources/src/cache.rs`.
- `crates/modde-sources/src/wabbajack/installer.rs::read_archive_source`.
- [crates/modde-sources/Cargo.toml](../crates/modde-sources/Cargo.toml)
  — `bytes`, `lru`, `parking_lot`.

### Acceptance

- [x] Cache size never exceeds the configured budget (verified by a
      forced-budget unit test).
- [ ] PatchedFromArchive throughput on Twisted Skyrim improves
      ≥ 30%.

### Estimated effort

1 day.

---

## Phase 7 — Compaction tier

**Status: ✅ 7a/7c landed; ⬜ 7b/7d deferred.**

The first Phase 7 pass implements zstd-compressed Wabbajack staging and
compressed-file-aware deploy. Chunk CAS and content trains remain
planned because they need a persistent store/index design rather than a
staging-local sweep.

### 7.a zstd-recompressed staging

Wabbajack archives use 7z LZMA: fast decompress, slow compress. After
extracting and writing to staging, modde now runs a post-apply,
post-CreateBSA compression sweep. Eligible regular files are rewritten
as `<logical-path>.modde-zst`; the logical source is removed only after
successful compression.

The staging layout is recorded in `_state/staging-layout.json`. Missing
or incompatible layout metadata is adopted in place by default: modde
writes the new layout metadata, rescues completed outputs into
sentinels when their logical files exist, and leaves existing files in
place. Use `modde install wabbajack --reset-staging ...` for an explicit
discard-and-rebuild. Compatible Phase-7 staging remains resumable. Plain
files remain valid inside the layout, so compression is selective and
interruption-safe.

Defaults:

- `MODDE_ZSTD_MIN_BYTES=1048576`
- `MODDE_ZSTD_LEVEL=9`
- `zstd = "0.13.3"` with `default-features = false` and `zstdmt`

The sweep skips `_state`, `meta.ini`, `meta.json`, executable/library
files, plugins, common config formats, and tiny files below the
threshold. Validators and preflight checks read through the logical
staging abstraction, so plain and compressed staging files are treated
identically.

### 7.b Content-addressed store with chunk dedup

Two modlists that both ship Realistic Water Two share ~3 GB of
identical archives. A CAS layer hashes archive content with FastCDC
(rolling-hash chunking, ~16 KB average chunk) and stores chunks
deduplicated. Two lists with 60% overlap share 60% of disk. Plays
well with phase 5's reflink fallback.

Deferred dependency targets observed during the Phase 7a/7c pass:
`fastcdc = "4.0.1"` for chunking and `blake3 = "1.8.5"` for chunk
identity.

### 7.c reflink-aware deploy on btrfs/xfs/APFS/ReFS

Phase 5 covers Stock Game. The deploy step now walks `mods/`, maps
`.modde-zst` files back to their logical destination names, and
decompresses them to the game directory. Plain staging files continue
through `modde_core::link::link_or_copy`, which tries hardlink,
reflink, then byte copy and logs the resulting `LinkKind`.

### 7.d zstd-content trains for archive cache

When a single archive is touched repeatedly (the .wabbajack itself),
keep its decompressed byte stream in zstd-recompressed memory blobs
keyed by offset range. Trades CPU for memory pressure.

This remains deferred with Phase 7b.

### Acceptance

Each sub-phase has its own benchmark target. The combined goal is to
take the on-disk store (~525 GB for one Skyrim AAA list) under
200 GB given two installed lists.

### Estimated effort

3–5 days per sub-phase, sequencable independently.

---

## Phase 8 — Streaming verification

**Status: ✅ landed.** `modde_core::hash` exposes streaming compatible
hash-copy support. Downloads now verify while writing for the common
simple path, direct/resumable HTTP, Google Drive, and Wabbajack authored
CDN chunks. Existing cached files still use the cheap existing-cache
verification path.

**Goal:** the verify pass currently re-hashes 525 GB sequentially after
download. Move verification inline with download so the file is hashed
as it's written, and re-verify only on suspicion (mtime change,
trusted-cache miss).

### Files touched

- `crates/modde-core/src/hash.rs` — already xxh64; expose a
  hashing-`Write` adapter.
- `crates/modde-sources/src/{nexus,direct,gdrive,...}` — wire the
  hashing writer.

### Acceptance

- [ ] Removing the standalone verify pass after download does not
      regress correctness on the test fixtures.
- [ ] Wall-clock for "all archives downloaded" → "ready to apply"
      drops by ~10 minutes on Twisted Skyrim.

### Estimated effort

1 day.

---

## Phase 9 — Resumable / incremental apply

**Status: ✅ landed.** Archive batches write JSON sentinels under
`<staging>/_state/archive-batches/<archive_hash>.json`; `CreateBSA`
writes separate sentinels under `<staging>/_state/create-bsa/`. A
sentinel is accepted only when the pipeline version, archive hash,
archive size, directive indices, and expected output files match the
current manifest. Failed or interrupted batches leave no sentinel and
rerun as a whole.

**Goal:** the apply phase is idempotent at the directive level and
records progress. A crash mid-apply restarts at the next pending
directive in the next pending archive batch, not at directive 0.

### Design

Per-batch sentinel file under
`<staging>/_state/archive-batches/<archive_hash>.json` written on batch
completion, listing the directive indices it touched. Restart logic:
load all matching sentinels, validate outputs, subtract, run the rest.

### Acceptance

- [x] Completed batches are skipped on restart when the sentinel and
      outputs match the current manifest.
- [ ] `kill -9` of modde mid-apply followed by rerunning install
      finishes the install without re-reading any archive whose
      sentinel exists in a full operational run.

### Estimated effort

2 days. Done after phase 1 because batches are the right granularity.

---

## Cross-cutting: ergonomics (the original promise)

Once the foundation is sound, the user-visible promises from the
earlier session land trivially:

- `modde install wabbajack-list "Twisted Skyrim"` — fetch + install in one shot.
- Stock-Game auto-detection (manifest contains `Stock Game/` → set
  `--no-deploy` automatically).
- Per-archive failures are soft-fail by default with a final summary.
- A GUI install wizard in modde-ui with phase-segmented progress bars
  (Download → Verify → Apply → Deploy), powered by the existing
  `InstallProgress` channel.

These are 1–2 days of CLI/UI work after phases 1–6.

---

## Sequencing

```
Phase 0  ─┐
Phase 1  ─┼─►  unblocks 2,3,6,9
Phase 2  ─┤   (replace subprocess)
Phase 3  ─┤   (streaming I/O)
Phase 4  ─┤   (inline zip index)
Phase 5  ─┤   (reflink/hardlink)
Phase 6  ─┤   (LRU cache)
Phase 7  ─┘   7a/7c landed; 7b/7d pickable independently
Phase 8        verify-on-download
Phase 9        resumable apply
Ergonomics     CLI/GUI wizards
```

Recommended ship order: **0 → 1 → 2 → 3 → 4 → 5 → 6 → 8 → 9 → 7a/7c**. The
rough wall-clock cost is ~3–4 weeks of focused work to land 0–6, after
which every modlist this size becomes installable on a 16 GB box.

## Test strategy

- Unit tests against tiny synthetic archives covering each format
  combination. Current landed coverage includes zip batch extraction,
  traversal rejection, BSA/BA2 archive round trips, byte-cache budget
  eviction, link replacement, and streaming hash mismatch detection.
- Integration test against the existing
  [3077.wabbajack](../3077.wabbajack) test fixture; expand to a
  small Skyrim fixture with mixed manifest types.
- A new `tests/large_install_smoke.rs` that downloads a small
  community modlist (~1 GB), installs it under
  `--data-dir tempdir`, and asserts: peak RSS, final disk usage, and
  apply wall-clock vs. budget.
- Run the smoke test under cgroup `MemoryHigh=2G` in CI to lock in
  memory bounds.

## Validation

Validated locally for this implementation pass:

- `nix develop . -c cargo check -p modde-sources --no-default-features`
- `nix develop . -c cargo check -p modde-sources --features rar`
- `nix develop . -c cargo check -p modde-cli --no-default-features`
- `nix develop . -c cargo test -p modde-core --no-default-features`
- `nix develop . -c cargo test -p modde-sources --no-default-features`
- `nix develop . -c cargo test -p modde-sources --features rar`
- `nix develop . -c cargo test -p modde-cli --no-default-features`
- `nix develop . -c cargo clippy -p modde-core -p modde-sources --all-targets --no-default-features -- -D warnings`
- `strace -f -e execve` against the synthetic Wabbajack pipeline test,
  followed by `rg '7zz|/7z|unrar'`, found no archive-tool subprocesses.
- `modde wabbajack assess ... --game-dir ...` against the local Twisted
  Skyrim manifest reports RAR enabled, all 223 game-file sources present,
  5,802/5,808 downloadable archives in store, and adoption of existing
  staging.
- The same Twisted assessment with `modde-cli --no-default-features`
  correctly hard-blocks because RAR archives are present.

`cargo clippy -p modde-cli --all-targets --no-default-features -- -D warnings`
still fails in pre-existing `modde-ui` lint debt (`doc_markdown`,
`return_self_not_must_use`, and excessive bool params), outside the
Wabbajack install path.

Targeted no-subprocess smoke command for a synthetic or repo fixture:

```bash
strace -f -e execve -o /tmp/modde-apply.execve.log \
  nix develop . -c cargo test -p modde-sources \
  wabbajack::installer::tests::synthetic_wabbajack_pipeline_reaches_late_directives \
  --no-default-features -- --exact
! rg '7zz|/7z|unrar' /tmp/modde-apply.execve.log
```

## Operational guidance during the transition

While phases land incrementally, operators can already:

- Run `modde wabbajack assess <list.wabbajack> --game-dir <game-dir>`
  before installing. The report prints exact store paths for missing
  archives, manual/Nexus source hints, staging layout action
  (`create`, `resume`, or `adopt`), sentinel progress, and whether the
  list is ready for validated deploy or only partial staging.
- For a rescue/stage-only run while archives are still missing, use
  `modde install wabbajack --no-deploy --continue-on-error --skip-validate
<list.wabbajack> --game-dir <game-dir>`. This preserves the live game
  directory and lets the next run resume from adopted sentinels.
- Use `--reset-staging` only when deliberately throwing away the
  profile's staging directory. The default path rescues existing files
  in place.
- Wrap installs in `systemd-run --user --scope -p MemoryHigh=24G -p
MemorySwapMax=4G -p IOWeight=50 -- modde install wabbajack ...` to
  contain blast radius.
- Set `MODDE_APPLY_MAX_IN_FLIGHT=8` until phase 1 lands.
- Set `MODDE_APPLY_RAM_FRACTION=0.5` until phase 2 lands.
- Use a separate filesystem for `--data-dir` (modde data) vs. the
  game install drive, until phase 5 lands.

These knobs all become unnecessary once the foundation is in place.
