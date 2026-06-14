//! Patcher subprocess execution helpers.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use modde_core::profile::Profile;
use modde_core::{CommandSettings, SynthesisCliSettings};

pub(super) fn run_synthesis_stage(
    settings: &SynthesisCliSettings,
    game_mod_dir: &Path,
    load_order_path: &Path,
    install_dir: &Path,
    stage_name: &str,
    work_dir: &Path,
    timeout_seconds: u64,
) -> Result<()> {
    let mut command = Command::new(&settings.executable);
    command
        .current_dir(install_dir)
        .arg("run-pipeline")
        .arg("--OutputDirectory")
        .arg(game_mod_dir)
        .arg("--PipelineSettingsPath")
        .arg(&settings.pipeline_settings)
        .arg("--ProfileIdentifier")
        .arg(&settings.synthesis_profile)
        .arg("--DataFolderPath")
        .arg(game_mod_dir)
        .arg("--LoadOrderFilePath")
        .arg(load_order_path);
    let status = run_patcher_command(&mut command, stage_name, work_dir, timeout_seconds)
        .with_context(|| format!("failed to execute synthesis-cli stage '{stage_name}'"))?;
    if !status.success() {
        anyhow::bail!(
            "synthesis-cli stage '{}' exited with status {:?}; stdout={} stderr={}",
            stage_name,
            status.code(),
            work_dir.join("stdout.log").display(),
            work_dir.join("stderr.log").display(),
        );
    }
    Ok(())
}

pub(super) fn run_command_stage(
    settings: &CommandSettings,
    game_mod_dir: &Path,
    load_order_path: &Path,
    install_dir: &Path,
    profile: &Profile,
    stage_name: &str,
    work_dir: &Path,
    timeout_seconds: u64,
) -> Result<()> {
    let mut command = Command::new(&settings.executable);
    command.args(&settings.args);
    command.current_dir(settings.working_dir.as_deref().unwrap_or(install_dir));
    command.env("MODDE_PATCHER_DATA_DIR", game_mod_dir);
    command.env("MODDE_PATCHER_LOAD_ORDER_FILE", load_order_path);
    command.env("MODDE_PATCHER_PROFILE", &profile.name);
    command.env("MODDE_PATCHER_GAME", profile.game_id.as_str());
    command.env("MODDE_PATCHER_STAGE_NAME", stage_name);
    for (key, value) in &settings.environment {
        command.env(key, value);
    }
    let status = run_patcher_command(&mut command, stage_name, work_dir, timeout_seconds)
        .with_context(|| format!("failed to execute command patcher stage '{stage_name}'"))?;
    if !status.success() {
        anyhow::bail!(
            "command patcher stage '{}' exited with status {:?}; stdout={} stderr={}",
            stage_name,
            status.code(),
            work_dir.join("stdout.log").display(),
            work_dir.join("stderr.log").display(),
        );
    }
    Ok(())
}

pub(super) fn run_patcher_command(
    command: &mut Command,
    stage_name: &str,
    work_dir: &Path,
    timeout_seconds: u64,
) -> Result<std::process::ExitStatus> {
    let stdout_path = work_dir.join("stdout.log");
    let stderr_path = work_dir.join("stderr.log");
    let stdout = fs::File::create(&stdout_path)
        .with_context(|| format!("failed to create {}", stdout_path.display()))?;
    let stderr = fs::File::create(&stderr_path)
        .with_context(|| format!("failed to create {}", stderr_path.display()))?;
    let mut child = command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds.max(1));
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait();
            anyhow::bail!(
                "patcher stage '{}' exceeded timeout of {}s; stdout={} stderr={}",
                stage_name,
                timeout_seconds,
                stdout_path.display(),
                stderr_path.display()
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}
