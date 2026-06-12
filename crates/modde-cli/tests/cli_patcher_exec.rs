//! End-to-end execution tests for the patcher pipeline: a real command
//! stage runs against a fixture game install, produces a managed output,
//! records its manifest in the DB, and is skipped on a cache hit.

#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};

use common::Fixture;
use modde_core::settings::AppSettings;
use modde_core::{GameId, ModdeDb};

const GAME: &str = "skyrim-se";
const PROFILE: &str = "main";
const STAGE: &str = "build-patch";
const OUTPUT_FILE: &str = "generated-patch.esp";

fn create_profile(fx: &Fixture) {
    let output = fx
        .cmd()
        .args(["profile", "create", PROFILE, "--game", GAME])
        .output()
        .expect("spawn modde profile create");
    assert!(
        output.status.success(),
        "profile create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Point the fixture's `settings.toml` at a fake game install so
/// `detect_install()` resolves inside the sandbox.
fn configure_install(fx: &Fixture) -> PathBuf {
    let install = fx.root().join("install");
    std::fs::create_dir_all(install.join("Data")).unwrap();
    let mut settings = AppSettings::default();
    settings.set_game_path(&GameId::from(GAME), install.clone());
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));
    install
}

/// A patcher script that reads the load-order file, bumps an invocation
/// counter, and writes one new plugin into the managed data dir.
fn patcher_script(fx: &Fixture, count_file: &Path) -> PathBuf {
    let script = fx.root().join("patcher.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -eu\n\
             cat \"$MODDE_PATCHER_LOAD_ORDER_FILE\" > /dev/null\n\
             printf 'run\\n' >> \"{}\"\n\
             printf 'patched\\n' > \"$MODDE_PATCHER_DATA_DIR/{OUTPUT_FILE}\"\n",
            count_file.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn add_command_stage(fx: &Fixture, script: &Path) {
    let add = fx
        .cmd()
        .args([
            "patcher",
            "add-command",
            STAGE,
            "--profile",
            PROFILE,
            "--game",
            GAME,
            "--executable",
            script.to_str().unwrap(),
            "--output-mod",
            "generated-patch",
        ])
        .output()
        .expect("spawn modde patcher add-command");
    assert!(
        add.status.success(),
        "patcher add-command failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
}

fn run_pipeline(fx: &Fixture) -> String {
    let run = fx
        .cmd()
        .args(["patcher", "run", "--profile", PROFILE, "--game", GAME])
        .output()
        .expect("spawn modde patcher run");
    assert!(
        run.status.success(),
        "patcher run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn invocation_count(count_file: &Path) -> usize {
    std::fs::read_to_string(count_file)
        .unwrap_or_default()
        .lines()
        .count()
}

#[test]
fn patcher_run_executes_stage_records_manifest_and_caches() {
    let fx = Fixture::new();
    create_profile(&fx);
    let install = configure_install(&fx);
    let count_file = fx.root().join("invocations.log");
    let script = patcher_script(&fx, &count_file);
    add_command_stage(&fx, &script);

    // First run: the stage actually executes and produces the output.
    let stdout = run_pipeline(&fx);
    assert!(
        stdout.contains("Ran 1 patcher stage(s)"),
        "unexpected run output:\n{stdout}"
    );
    assert_eq!(invocation_count(&count_file), 1, "stage should run once");

    // Output is captured into the managed generated dir and projected
    // back into the game's Data dir.
    let generated = fx
        .data_dir()
        .join("generated")
        .join(GAME)
        .join(PROFILE)
        .join(STAGE)
        .join(OUTPUT_FILE);
    assert!(
        generated.is_file(),
        "managed output missing: {}",
        generated.display()
    );
    let deployed = install.join("Data").join(OUTPUT_FILE);
    assert!(
        deployed.exists(),
        "deployed output missing: {}",
        deployed.display()
    );

    // Second run with unchanged inputs: cache hit, patcher not re-invoked.
    let stdout = run_pipeline(&fx);
    assert!(
        stdout.contains(&format!("Skipped patcher stage '{STAGE}' (cache hit)")),
        "expected cache-hit skip:\n{stdout}"
    );
    assert_eq!(
        invocation_count(&count_file),
        1,
        "cache hit must not re-invoke the patcher"
    );

    // DB state: the stage's output manifest records the file and the
    // cache key was persisted by the successful run.
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = ModdeDb::open_at(&fx.data_dir().join("modde.db"))
            .await
            .expect("open fixture db");
        let profile = db
            .load_profile(PROFILE, &GameId::from(GAME))
            .await
            .expect("load profile");
        let profile_id = profile.id.expect("persisted profile id");
        let outputs = db
            .list_patcher_stage_outputs(profile_id, STAGE)
            .await
            .expect("list stage outputs");
        let rel_paths: Vec<&str> = outputs.iter().map(|row| row.rel_path.as_str()).collect();
        assert_eq!(
            rel_paths,
            vec![OUTPUT_FILE],
            "stage output manifest should record the generated file"
        );
        let stage = db
            .load_patcher_stage(profile_id, STAGE)
            .await
            .expect("load stage")
            .expect("stage exists");
        assert!(
            stage.last_cache_key.is_some(),
            "successful run should persist last_cache_key"
        );
    });
}
