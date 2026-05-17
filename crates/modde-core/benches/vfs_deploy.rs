//! Benchmarks for the symlink farm hot path.
//!
//! Two kinds of work get measured separately because they have very
//! different cost profiles:
//!
//! * `build` — pure in-memory: walks the resolved load order, picks
//!   the winner per relative path. CPU-only, no I/O.
//! * `materialize_and_deploy` — touches the filesystem: creates the
//!   staging tree of symlinks, then deploys it into a target dir.
//!   This is the realistic bottleneck for large profiles.
//!
//! Run with `just coverage`-equivalent:
//!
//!     cargo bench -p modde-core --bench vfs_deploy
//!
//! Criterion writes HTML reports under `target/criterion/`.

use std::collections::HashMap;
use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use modde_core::resolver::{ModId, ResolvedLoadOrder};
use modde_core::vfs::SymlinkFarm;

/// Build a synthetic `(resolved_order, mod_files)` fixture: `n_mods`
/// mods each contributing `files_per_mod` distinct files. No
/// collisions — every relative path has a single owner. Sources point
/// at real on-disk files inside `store_root` so `materialize` can
/// actually symlink them.
fn make_fixture(
    store_root: &std::path::Path,
    n_mods: usize,
    files_per_mod: usize,
) -> (ResolvedLoadOrder, HashMap<ModId, Vec<(String, PathBuf)>>) {
    let mut order = Vec::with_capacity(n_mods);
    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::with_capacity(n_mods);

    for m in 0..n_mods {
        let mod_id_str = format!("mod_{m:05}");
        let mod_id = ModId::from(mod_id_str.as_str());
        let mod_dir = store_root.join(&mod_id_str);
        std::fs::create_dir_all(&mod_dir).expect("create mod dir");

        let mut files = Vec::with_capacity(files_per_mod);
        for f in 0..files_per_mod {
            let rel = format!("data/{mod_id_str}/file_{f:05}.dat");
            let src = mod_dir.join(format!("file_{f:05}.dat"));
            // Tiny payload — the bench is about path/symlink throughput,
            // not byte volume.
            std::fs::write(&src, b"x").expect("write source");
            files.push((rel, src));
        }
        mod_files.insert(mod_id.clone(), files);
        order.push(mod_id);
    }
    (ResolvedLoadOrder { order }, mod_files)
}

fn bench_build(c: &mut Criterion) {
    // Pure in-memory work. Scale up to 50k files to lock in
    // expectations for the upper bound called out in the TODO.
    let mut group = c.benchmark_group("symlink_farm_build");
    let store = tempfile::TempDir::new().unwrap();

    for &(n_mods, files_per_mod) in &[(100, 10), (100, 100), (500, 100)] {
        let total = n_mods * files_per_mod;
        let (resolved, mod_files) = make_fixture(store.path(), n_mods, files_per_mod);
        group.throughput(Throughput::Elements(total as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{n_mods}m_x_{files_per_mod}f")),
            &(resolved, mod_files),
            |b, (resolved, mod_files)| {
                b.iter(|| {
                    let _farm =
                        SymlinkFarm::build("bench", resolved, mod_files, None, None).unwrap();
                });
            },
        );
    }
    group.finish();
}

fn bench_materialize_and_deploy(c: &mut Criterion) {
    // Filesystem-touching path. Capped at 10k files because each
    // iteration recreates the staging + target trees from scratch and
    // criterion needs many samples — 50k blows the wall-clock budget
    // on slow filesystems. Run manually with a custom harness if you
    // need 50k numbers.
    let mut group = c.benchmark_group("symlink_farm_materialize_deploy");
    group.sample_size(10); // I/O-bound; default 100 is excessive

    for &(n_mods, files_per_mod) in &[(50, 20), (100, 100)] {
        let total = n_mods * files_per_mod;
        group.throughput(Throughput::Elements(total as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{n_mods}m_x_{files_per_mod}f")),
            &(n_mods, files_per_mod),
            |b, &(n_mods, files_per_mod)| {
                // Fresh tempdir per iteration so we measure cold-deploy
                // cost, not "remove + recreate" cost on top of the prior
                // run's leftovers.
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                b.iter_with_setup(
                    || {
                        let store = tempfile::TempDir::new().unwrap();
                        let target = tempfile::TempDir::new().unwrap();
                        let staging = tempfile::TempDir::new().unwrap();
                        let (resolved, mod_files) =
                            make_fixture(store.path(), n_mods, files_per_mod);
                        let built =
                            SymlinkFarm::build("bench", &resolved, &mod_files, None, None).unwrap();
                        let farm = SymlinkFarm::from_links(
                            staging.path().to_path_buf(),
                            built.links.clone(),
                        );
                        // Hold tempdirs alive for the duration of the iter.
                        (farm, target, store, staging)
                    },
                    |(farm, target, _store, _staging)| {
                        rt.block_on(async {
                            let materialized = farm.materialize().await.unwrap();
                            materialized.deploy_to(target.path()).await.unwrap();
                        });
                    },
                );
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_build, bench_materialize_and_deploy);
criterion_main!(benches);
