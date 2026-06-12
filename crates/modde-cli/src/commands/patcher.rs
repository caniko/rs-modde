use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use modde_core::fs::{is_cross_device_error, walk_files_relative};
use modde_core::hash::sha256_hex;
use modde_core::profile::{Profile, ProfileManager};
use modde_core::{
    CommandSettings, ModdeDb, PatcherStageRow, PatcherStageSettings, SynthesisCliSettings, paths,
};

use super::{load_plugin_order, load_profile_or_default};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    sha256: String,
}

pub async fn handle_list(profile_name: Option<String>, game_id: Option<String>) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let profile_id = require_profile_id(&profile)?;
    let stages = pm.db().list_patcher_stages(profile_id).await?;

    if stages.is_empty() {
        println!("No patcher stages configured for {}.", profile.name);
        return Ok(());
    }

    println!("Patcher stages for {} ({}):", profile.name, profile.game_id);
    for stage in stages {
        println!(
            "  {} [{}] {} order={} -> {}",
            stage.name,
            stage.stage_kind.as_str(),
            if stage.enabled { "enabled" } else { "disabled" },
            stage.sort_index,
            stage.output_mod
        );
    }
    Ok(())
}

pub async fn handle_add_synthesis(
    name: &str,
    profile_name: Option<String>,
    game_id: Option<String>,
    executable: PathBuf,
    pipeline_settings: PathBuf,
    synthesis_profile: String,
    output_mod: String,
    order: i64,
    timeout_seconds: u64,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    validate_synthesis_game(&profile)?;
    let stage = PatcherStageRow::new(
        require_profile_id(&profile)?,
        name,
        true,
        order,
        PatcherStageSettings::SynthesisCli(SynthesisCliSettings {
            executable,
            pipeline_settings,
            synthesis_profile,
        }),
        output_mod,
    )?
    .with_timeout_seconds(timeout_seconds);
    validate_stage_definition(&stage)?;
    pm.db().save_patcher_stage(&stage).await?;
    println!(
        "Saved Synthesis patcher stage '{}' for {} at order {}",
        stage.name, profile.name, stage.sort_index
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_add_command(
    name: &str,
    profile_name: Option<String>,
    game_id: Option<String>,
    executable: PathBuf,
    working_dir: Option<PathBuf>,
    args: Vec<String>,
    environment: Vec<String>,
    output_mod: String,
    order: i64,
    timeout_seconds: u64,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let stage = PatcherStageRow::new(
        require_profile_id(&profile)?,
        name,
        true,
        order,
        PatcherStageSettings::Command(CommandSettings {
            executable,
            args,
            environment: parse_env(environment)?,
            working_dir,
        }),
        output_mod,
    )?
    .with_timeout_seconds(timeout_seconds);
    validate_stage_definition(&stage)?;
    pm.db().save_patcher_stage(&stage).await?;
    println!(
        "Saved command patcher stage '{}' for {} at order {}",
        stage.name, profile.name, stage.sort_index
    );
    Ok(())
}

pub async fn handle_remove(
    name: &str,
    profile_name: Option<String>,
    game_id: Option<String>,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let profile_id = require_profile_id(&profile)?;
    let removed = pm.db().delete_patcher_stage(profile_id, name).await?;
    if !removed {
        anyhow::bail!(
            "no patcher stage named '{name}' is configured for {}",
            profile.name
        );
    }

    pm.db()
        .replace_patcher_stage_outputs(profile_id, name, &[])
        .await?;
    let generated = stage_generated_dir(&profile, name);
    if generated.exists() {
        fs::remove_dir_all(&generated)?;
    }
    println!("Removed patcher stage '{name}' from {}", profile.name);
    Ok(())
}

pub async fn handle_set_enabled(
    name: &str,
    profile_name: Option<String>,
    game_id: Option<String>,
    enabled: bool,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let changed = pm
        .db()
        .set_patcher_stage_enabled(require_profile_id(&profile)?, name, enabled)
        .await?;
    if !changed {
        anyhow::bail!(
            "no patcher stage named '{name}' is configured for {}",
            profile.name
        );
    }
    println!(
        "{} patcher stage '{name}' for {}",
        if enabled { "Enabled" } else { "Disabled" },
        profile.name
    );
    Ok(())
}

pub async fn handle_reorder(
    profile_name: Option<String>,
    game_id: Option<String>,
    names: Vec<String>,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    pm.db()
        .reorder_patcher_stages(require_profile_id(&profile)?, &names)
        .await?;
    println!("Updated patcher stage order for {}", profile.name);
    Ok(())
}

pub async fn handle_validate(profile_name: Option<String>, game_id: Option<String>) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let game_plugin =
        modde_games::resolve_game_plugin(profile.game_id.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "unsupported game: '{}'\nSupported games: {}",
                profile.game_id,
                modde_games::supported_game_ids().join(", ")
            )
        })?;
    let stages = pm
        .db()
        .list_patcher_stages(require_profile_id(&profile)?)
        .await?;
    for stage in stages.iter().filter(|stage| stage.enabled) {
        validate_stage_definition(stage)?;
        validate_stage_runtime(stage, &profile, game_plugin)?;
    }
    println!(
        "Validated {} enabled patcher stage(s) for {}",
        stages.iter().filter(|stage| stage.enabled).count(),
        profile.name
    );
    Ok(())
}

pub async fn handle_run_stage(
    name: &str,
    profile_name: Option<String>,
    game_id: Option<String>,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let game_plugin =
        modde_games::resolve_game_plugin(profile.game_id.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "unsupported game: '{}'\nSupported games: {}",
                profile.game_id,
                modde_games::supported_game_ids().join(", ")
            )
        })?;
    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;
    let profile_id = require_profile_id(&profile)?;
    let stage = pm
        .db()
        .load_patcher_stage(profile_id, name)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no patcher stage named '{name}' is configured for {}",
                profile.name
            )
        })?;
    let game_mod_dir = game_plugin
        .mod_root(&install_dir)
        .context("failed to resolve game mod root for patcher pipeline")?;
    let stages = pm.db().list_patcher_stages(profile_id).await?;
    let mut manifests = load_stage_manifests(pm.db(), profile_id, &stages).await?;
    run_stage(
        pm.db(),
        &profile,
        game_plugin,
        &install_dir,
        &game_mod_dir,
        &stage,
        &mut manifests,
    )
    .await?;
    println!("Ran patcher stage '{name}' for {}", profile.name);
    Ok(())
}

pub async fn handle_run(profile_name: Option<String>, game_id: Option<String>) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let game_plugin =
        modde_games::resolve_game_plugin(profile.game_id.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "unsupported game: '{}'\nSupported games: {}",
                profile.game_id,
                modde_games::supported_game_ids().join(", ")
            )
        })?;
    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;

    let ran = run_enabled_pipeline(&pm, &profile, game_plugin, &install_dir).await?;
    println!("Ran {ran} patcher stage(s) for {}", profile.name);
    Ok(())
}

pub async fn run_enabled_for_deploy(
    pm: &ProfileManager,
    profile: &Profile,
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
) -> Result<usize> {
    run_enabled_pipeline(pm, profile, game_plugin, install_dir).await
}

async fn run_enabled_pipeline(
    pm: &ProfileManager,
    profile: &Profile,
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
) -> Result<usize> {
    let profile_id = match profile.id {
        Some(id) => id,
        None => return Ok(0),
    };
    let game_mod_dir = game_plugin
        .mod_root(install_dir)
        .context("failed to resolve game mod root for patcher pipeline")?;
    let stages = pm.db().list_patcher_stages(profile_id).await?;
    let enabled: Vec<PatcherStageRow> = stages.into_iter().filter(|stage| stage.enabled).collect();
    let original_manifests = load_stage_manifests(pm.db(), profile_id, &enabled).await?;
    let backup = backup_generated_outputs(profile, &enabled)?;

    reset_managed_outputs(
        pm.db(),
        profile,
        &enabled,
        game_plugin,
        install_dir,
        &game_mod_dir,
    )
    .await?;

    let mut manifests = original_manifests.clone();
    for stage in &enabled {
        if let Err(error) = run_stage(
            pm.db(),
            profile,
            game_plugin,
            install_dir,
            &game_mod_dir,
            stage,
            &mut manifests,
        )
        .await
        {
            restore_patcher_pipeline(
                pm.db(),
                profile,
                &enabled,
                game_plugin,
                install_dir,
                &game_mod_dir,
                &original_manifests,
                &backup,
            )
            .await?;
            return Err(error);
        }
    }
    remove_dir_if_exists(&backup.root)?;

    Ok(enabled.len())
}

#[derive(Debug)]
struct PatcherPipelineBackup {
    root: PathBuf,
    stage_dirs: HashMap<String, PathBuf>,
}

fn backup_generated_outputs(
    profile: &Profile,
    stages: &[PatcherStageRow],
) -> Result<PatcherPipelineBackup> {
    let root = paths::modde_cache_dir()
        .join("patchers")
        .join("backups")
        .join(format!("{}-{}", profile.name, patcher_time_id()));
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(&root)?;
    let mut stage_dirs = HashMap::new();
    for stage in stages {
        let generated = stage_generated_dir(profile, &stage.name);
        if generated.exists() {
            let backup_dir = root.join(&stage.name);
            copy_dir_recursive(&generated, &backup_dir)?;
            stage_dirs.insert(stage.name.clone(), backup_dir);
        }
    }
    Ok(PatcherPipelineBackup { root, stage_dirs })
}

async fn restore_patcher_pipeline(
    db: &ModdeDb,
    profile: &Profile,
    stages: &[PatcherStageRow],
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
    game_mod_dir: &Path,
    manifests: &HashMap<String, Vec<String>>,
    backup: &PatcherPipelineBackup,
) -> Result<()> {
    for stage in stages {
        let generated = stage_generated_dir(profile, &stage.name);
        if generated.exists() {
            fs::remove_dir_all(&generated)?;
        }
        if let Some(backup_dir) = backup.stage_dirs.get(&stage.name) {
            copy_dir_recursive(backup_dir, &generated)?;
        }
        db.replace_patcher_stage_outputs(
            require_profile_id(profile)?,
            &stage.name,
            manifests
                .get(&stage.name)
                .map(Vec::as_slice)
                .unwrap_or_default(),
        )
        .await?;
    }
    reset_managed_outputs(db, profile, stages, game_plugin, install_dir, game_mod_dir).await?;
    remove_dir_if_exists(&backup.root)
}

async fn load_stage_manifests(
    db: &ModdeDb,
    profile_id: i64,
    stages: &[PatcherStageRow],
) -> Result<HashMap<String, Vec<String>>> {
    let mut manifests = HashMap::new();
    for stage in stages {
        let rows = db
            .list_patcher_stage_outputs(profile_id, &stage.name)
            .await?;
        manifests.insert(
            stage.name.clone(),
            rows.into_iter().map(|row| row.rel_path).collect(),
        );
    }
    Ok(manifests)
}

async fn reset_managed_outputs(
    db: &ModdeDb,
    profile: &Profile,
    stages: &[PatcherStageRow],
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
    game_mod_dir: &Path,
) -> Result<()> {
    if let Some(profile_id) = profile.id {
        let existing = db.list_all_patcher_stage_outputs(profile_id).await?;
        let mut rel_paths = existing
            .into_iter()
            .map(|row| row.rel_path)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        rel_paths.sort();
        remove_rel_paths(game_mod_dir, &rel_paths)?;
    }

    for stage in stages {
        let generated = stage_generated_dir(profile, &stage.name);
        if generated.exists() {
            game_plugin
                .deploy_to_install(&generated, install_dir)
                .with_context(|| {
                    format!(
                        "failed to project managed patcher output for stage '{}'",
                        stage.name
                    )
                })?;
        }
    }
    Ok(())
}

async fn run_stage(
    db: &ModdeDb,
    profile: &Profile,
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
    game_mod_dir: &Path,
    stage: &PatcherStageRow,
    manifests: &mut HashMap<String, Vec<String>>,
) -> Result<()> {
    validate_stage_definition(stage)?;
    validate_stage_runtime(stage, profile, game_plugin)?;

    let before = snapshot_dir(game_mod_dir).await?;
    let execution = execute_stage(profile, install_dir, game_mod_dir, stage).await?;
    if execution.skipped {
        println!("Skipped patcher stage '{}' (cache hit)", stage.name);
        return Ok(());
    }
    let after = snapshot_dir(game_mod_dir).await?;

    let own_before = manifests
        .get(&stage.name)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();
    let other_owned = manifests
        .iter()
        .filter(|(stage_name, _)| *stage_name != &stage.name)
        .flat_map(|(_, rel_paths)| rel_paths.iter().cloned())
        .collect::<HashSet<_>>();

    let next_manifest = compute_next_manifest(&before, &after, &own_before, &other_owned, stage)?;
    install_managed_output(
        db,
        profile,
        game_plugin,
        install_dir,
        game_mod_dir,
        stage,
        &own_before,
        &next_manifest,
        &execution.cache_key,
    )
    .await?;
    manifests.insert(stage.name.clone(), next_manifest);
    Ok(())
}

fn validate_synthesis_game(profile: &Profile) -> Result<()> {
    let game_plugin = modde_games::resolve_game_plugin(profile.game_id.as_str())
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{}'", profile.game_id))?;
    if !game_plugin.has_plugin_system() {
        anyhow::bail!(
            "synthesis-cli patchers are only supported on Bethesda plugin-order games in v1; '{}' does not expose plugins.txt-style load order",
            profile.game_id
        );
    }
    Ok(())
}

fn validate_stage_definition(stage: &PatcherStageRow) -> Result<()> {
    match &stage.settings {
        PatcherStageSettings::SynthesisCli(settings) => {
            validate_file(
                &settings.executable,
                "Synthesis CLI executable",
                "Synthesis release zip",
                "download and unpack Synthesis.zip outside the game/Data and modde store directories",
                format!("{} --help", settings.executable.display()),
            )?;
            validate_file(
                &settings.pipeline_settings,
                "Synthesis pipeline settings",
                "Synthesis UI or Synthesis CLI profile export",
                "create or export PipelineSettings.json with Synthesis",
                format!("test -r {}", settings.pipeline_settings.display()),
            )?;
            if settings.synthesis_profile.trim().is_empty() {
                anyhow::bail!(
                    "patcher stage '{}' is missing synthesis_profile",
                    stage.name
                );
            }
        }
        PatcherStageSettings::Command(settings) => {
            validate_file(
                &settings.executable,
                "patcher command executable",
                "the user-authored patcher package or script",
                "install/build the command and update this stage with modde patcher add-command",
                format!("{} --help", settings.executable.display()),
            )?;
        }
        PatcherStageSettings::RustNative => {
            anyhow::bail!(
                "patcher stage '{}' uses rust-native, but rust-native patchers are reserved for a future release",
                stage.name
            );
        }
    }
    Ok(())
}

fn validate_stage_runtime(
    stage: &PatcherStageRow,
    profile: &Profile,
    game_plugin: &dyn modde_games::GamePlugin,
) -> Result<()> {
    if matches!(stage.settings, PatcherStageSettings::SynthesisCli(_))
        && !game_plugin.has_plugin_system()
    {
        anyhow::bail!(
            "patcher stage '{}' uses synthesis-cli, but '{}' does not expose a Bethesda-style plugin load order",
            stage.name,
            profile.game_id
        );
    }
    Ok(())
}

fn validate_file(
    path: &Path,
    label: &str,
    producer: &str,
    fix: &str,
    validation: String,
) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }
    anyhow::bail!(
        "missing required {label}: {}. Why required: enabled patcher stages are deployment artifacts and must be reproducible. Upstream producer: {producer}. Regenerate/fix by: {fix}. Validate with: {validation}",
        path.display()
    )
}

async fn execute_stage(
    profile: &Profile,
    install_dir: &Path,
    game_mod_dir: &Path,
    stage: &PatcherStageRow,
) -> Result<StageExecution> {
    let work_dir = paths::modde_cache_dir()
        .join("patchers")
        .join(profile.game_id.as_str())
        .join(&profile.name)
        .join(&stage.name);
    fs::create_dir_all(&work_dir)?;
    let load_order_path = work_dir.join("load-order.txt");
    write_load_order(profile, &load_order_path).await?;
    let load_order = fs::read_to_string(&load_order_path)?;
    let cache_key = patcher_cache_key(stage, &load_order)?;
    let generated = stage_generated_dir(profile, &stage.name);
    if stage.last_cache_key.as_deref() == Some(cache_key.as_str()) && generated.exists() {
        return Ok(StageExecution {
            cache_key,
            skipped: true,
        });
    }
    if work_dir.exists() {
        fs::remove_dir_all(&work_dir)?;
    }
    fs::create_dir_all(&work_dir)?;
    fs::write(&load_order_path, load_order)?;

    match &stage.settings {
        PatcherStageSettings::SynthesisCli(settings) => {
            run_synthesis_stage(
                settings,
                game_mod_dir,
                &load_order_path,
                install_dir,
                &stage.name,
                &work_dir,
                stage.timeout_seconds,
            )?;
        }
        PatcherStageSettings::Command(settings) => {
            run_command_stage(
                settings,
                game_mod_dir,
                &load_order_path,
                install_dir,
                profile,
                &stage.name,
                &work_dir,
                stage.timeout_seconds,
            )?;
        }
        PatcherStageSettings::RustNative => unreachable!("validated before execution"),
    }
    Ok(StageExecution {
        cache_key,
        skipped: false,
    })
}

#[derive(Debug, Clone)]
struct StageExecution {
    cache_key: String,
    skipped: bool,
}

fn run_synthesis_stage(
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

fn run_command_stage(
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

fn run_patcher_command(
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

async fn write_load_order(profile: &Profile, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database for patcher load order")?;
    let plugins = load_plugin_order(&pm, profile).await?;
    let mut content = String::new();
    for plugin in plugins.iter().filter(|plugin| plugin.enabled) {
        content.push('*');
        content.push_str(&plugin.plugin_name);
        content.push('\n');
    }
    fs::write(path, content)?;
    Ok(())
}

async fn snapshot_dir(root: &Path) -> Result<HashMap<String, FileFingerprint>> {
    let mut snapshot = HashMap::new();
    for (rel_path, abs_path) in walk_files_relative(root)? {
        let sha256 = modde_core::hash::hash_file_sha256(&abs_path)
            .await
            .with_context(|| format!("failed to hash {}", abs_path.display()))?;
        snapshot.insert(rel_path, FileFingerprint { sha256 });
    }
    Ok(snapshot)
}

fn compute_next_manifest(
    before: &HashMap<String, FileFingerprint>,
    after: &HashMap<String, FileFingerprint>,
    own_before: &HashSet<String>,
    other_owned: &HashSet<String>,
    stage: &PatcherStageRow,
) -> Result<Vec<String>> {
    let mut next_manifest = own_before
        .iter()
        .filter(|rel_path| after.contains_key(*rel_path))
        .cloned()
        .collect::<Vec<_>>();

    for rel_path in changed_non_owned_paths(before, after, own_before) {
        if before.contains_key(&rel_path) {
            let owner = if other_owned.contains(&rel_path) {
                "another managed stage"
            } else {
                "the base deployment"
            };
            anyhow::bail!(
                "patcher stage '{}' modified '{}' owned by {}; patcher stages may only create new files or rewrite their own prior outputs",
                stage.name,
                rel_path,
                owner
            );
        }
        if after.contains_key(&rel_path) {
            next_manifest.push(rel_path);
        }
    }

    next_manifest.sort();
    next_manifest.dedup();
    Ok(next_manifest)
}

fn changed_non_owned_paths(
    before: &HashMap<String, FileFingerprint>,
    after: &HashMap<String, FileFingerprint>,
    own_before: &HashSet<String>,
) -> Vec<String> {
    let mut changed = HashSet::new();
    for rel_path in before.keys().chain(after.keys()) {
        if own_before.contains(rel_path) {
            continue;
        }
        if before.get(rel_path) != after.get(rel_path) {
            changed.insert(rel_path.clone());
        }
    }
    let mut changed = changed.into_iter().collect::<Vec<_>>();
    changed.sort();
    changed
}

async fn install_managed_output(
    db: &ModdeDb,
    profile: &Profile,
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
    game_mod_dir: &Path,
    stage: &PatcherStageRow,
    own_before: &HashSet<String>,
    next_manifest: &[String],
    cache_key: &str,
) -> Result<()> {
    let generated = stage_generated_dir(profile, &stage.name);
    let mut old_paths = own_before.iter().cloned().collect::<Vec<_>>();
    old_paths.sort();
    remove_rel_paths(game_mod_dir, &old_paths)?;

    if generated.exists() {
        fs::remove_dir_all(&generated)?;
    }
    fs::create_dir_all(&generated)?;

    for rel_path in next_manifest {
        let src = game_mod_dir.join(rel_path);
        let dst = generated.join(rel_path);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        move_file(&src, &dst)?;
    }

    db.replace_patcher_stage_outputs(require_profile_id(profile)?, &stage.name, next_manifest)
        .await?;

    if !next_manifest.is_empty() {
        game_plugin
            .deploy_to_install(&generated, install_dir)
            .with_context(|| {
                format!(
                    "failed to project generated output for patcher stage '{}'",
                    stage.name
                )
            })?;
    }
    db.mark_patcher_stage_cache_success(require_profile_id(profile)?, &stage.name, cache_key)
        .await?;
    Ok(())
}

fn remove_rel_paths(root: &Path, rel_paths: &[String]) -> Result<()> {
    for rel_path in rel_paths {
        let path = root.join(rel_path);
        if path.symlink_metadata().is_ok() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove managed output {}", path.display()))?;
            remove_empty_parents(root, path.parent());
        }
    }
    Ok(())
}

fn patcher_cache_key(stage: &PatcherStageRow, load_order: &str) -> Result<String> {
    let settings_json = serde_json::to_string(&stage.settings)
        .context("failed to serialize patcher settings for cache key")?;
    let payload = serde_json::json!({
        "stage_name": &stage.name,
        "stage_kind": stage.stage_kind.as_str(),
        "settings": settings_json,
        "output_mod": &stage.output_mod,
        "load_order": load_order,
    });
    Ok(sha256_hex(payload.to_string().as_bytes()))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src).with_context(|| format!("failed to read {}", src.display()))? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn patcher_time_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}{:09}", now.as_secs(), now.subsec_nanos())
}

fn remove_empty_parents(root: &Path, mut current: Option<&Path>) {
    while let Some(dir) = current {
        if dir == root {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

fn move_file(src: &Path, dst: &Path) -> Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(err) if is_cross_device_error(&err) => {
            fs::copy(src, dst).with_context(|| {
                format!("failed to copy {} to {}", src.display(), dst.display())
            })?;
            fs::remove_file(src).with_context(|| format!("failed to remove {}", src.display()))?;
            Ok(())
        }
        Err(err) => Err(err)
            .with_context(|| format!("failed to move {} to {}", src.display(), dst.display())),
    }
}

fn stage_generated_dir(profile: &Profile, stage_name: &str) -> PathBuf {
    paths::generated_dir()
        .join(profile.game_id.as_str())
        .join(&profile.name)
        .join(stage_name)
}

fn parse_env(entries: Vec<String>) -> Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    for entry in entries {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("environment entries must be KEY=VALUE"))?;
        if key.is_empty() {
            anyhow::bail!("environment variable key cannot be empty");
        }
        env.insert(key.to_string(), value.to_string());
    }
    Ok(env)
}

fn require_profile_id(profile: &Profile) -> Result<i64> {
    profile
        .id
        .ok_or_else(|| anyhow::anyhow!("profile '{}' is not persisted", profile.name))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
