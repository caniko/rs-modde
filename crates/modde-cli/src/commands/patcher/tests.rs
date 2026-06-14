use super::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use modde_core::{CommandSettings, PatcherStageRow, PatcherStageSettings};

use super::fs_ops::compute_next_manifest;
use super::process::run_patcher_command;

fn fp(seed: &str) -> FileFingerprint {
    FileFingerprint {
        sha256: seed.to_string(),
    }
}

fn stage_named(name: &str) -> PatcherStageRow {
    PatcherStageRow::new(
        1,
        name,
        true,
        0,
        PatcherStageSettings::Command(CommandSettings {
            executable: PathBuf::from("/bin/true"),
            args: Vec::new(),
            environment: HashMap::new(),
            working_dir: None,
        }),
        format!("{name}-output"),
    )
    .unwrap()
}

#[cfg(unix)]
#[test]
fn run_patcher_command_kills_process_on_timeout() {
    let work_dir = tempfile::tempdir().unwrap();
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "sleep 30"]);

    let started = Instant::now();
    let err = run_patcher_command(&mut command, "slow-stage", work_dir.path(), 1).unwrap_err();
    let elapsed = started.elapsed();

    let message = err.to_string();
    assert!(
        message.contains("exceeded timeout of 1s"),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("slow-stage"),
        "error should name the stage: {message}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "timed-out process should be killed promptly, took {elapsed:?}"
    );
}

#[cfg(unix)]
#[test]
fn run_patcher_command_captures_stdout_and_stderr_logs() {
    let work_dir = tempfile::tempdir().unwrap();
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "echo out-marker; echo err-marker >&2"]);

    let status = run_patcher_command(&mut command, "log-stage", work_dir.path(), 30).unwrap();
    assert!(status.success(), "stage command should exit zero");

    let stdout = fs::read_to_string(work_dir.path().join("stdout.log")).unwrap();
    let stderr = fs::read_to_string(work_dir.path().join("stderr.log")).unwrap();
    assert!(
        stdout.contains("out-marker"),
        "stdout.log missing marker:\n{stdout}"
    );
    assert!(
        stderr.contains("err-marker"),
        "stderr.log missing marker:\n{stderr}"
    );
    assert!(
        !stdout.contains("err-marker"),
        "stderr leaked into stdout.log:\n{stdout}"
    );
}

#[test]
fn compute_next_manifest_allows_new_and_own_files() {
    let before = HashMap::from([
        ("base.txt".to_string(), fp("a")),
        ("own.txt".to_string(), fp("b")),
    ]);
    let after = HashMap::from([
        ("base.txt".to_string(), fp("a")),
        ("own.txt".to_string(), fp("c")),
        ("new.txt".to_string(), fp("d")),
    ]);
    let own_before = HashSet::from(["own.txt".to_string()]);
    let other_owned = HashSet::new();

    let manifest = compute_next_manifest(
        &before,
        &after,
        &own_before,
        &other_owned,
        &stage_named("stage"),
    )
    .unwrap();

    assert_eq!(manifest, vec!["new.txt".to_string(), "own.txt".to_string()]);
}

#[test]
fn compute_next_manifest_rejects_base_modification() {
    let before = HashMap::from([("base.txt".to_string(), fp("a"))]);
    let after = HashMap::from([("base.txt".to_string(), fp("b"))]);
    let err = compute_next_manifest(
        &before,
        &after,
        &HashSet::new(),
        &HashSet::new(),
        &stage_named("stage"),
    )
    .unwrap_err();

    assert!(err.to_string().contains("base deployment"));
}

#[test]
fn compute_next_manifest_rejects_other_stage_modification() {
    let before = HashMap::from([("shared.txt".to_string(), fp("a"))]);
    let after = HashMap::from([("shared.txt".to_string(), fp("b"))]);
    let err = compute_next_manifest(
        &before,
        &after,
        &HashSet::new(),
        &HashSet::from(["shared.txt".to_string()]),
        &stage_named("stage"),
    )
    .unwrap_err();

    assert!(err.to_string().contains("another managed stage"));
}
