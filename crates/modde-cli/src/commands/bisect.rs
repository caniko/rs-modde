use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::ValueEnum;

use modde_core::bisect::{apply_result, next_candidate_with_dependencies};
use modde_core::performance::mod_snapshot;
use modde_core::profile::{ActivateResult, Profile, ProfileManager, ProfileSource};
use modde_core::save::SaveFingerprint;
use modde_core::{
    BisectOracle, BisectResult, BisectSaveSafety, BisectSession, BisectStatus, GameId,
    NewBisectSession, NewBisectStep, NewPerformanceRun, PerformanceSummary,
};

use super::{compute_fingerprint, load_plugin_order, resolve_save_dir, supports_save_profiles};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BisectOracleArg {
    Manual,
    Crash,
    Perf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BisectResultArg {
    Good,
    Bad,
}

impl From<BisectResultArg> for BisectResult {
    fn from(value: BisectResultArg) -> Self {
        match value {
            BisectResultArg::Good => Self::Good,
            BisectResultArg::Bad => Self::Bad,
        }
    }
}

pub async fn handle_start(
    game: String,
    profile_name: String,
    oracle: BisectOracleArg,
    baseline_run: Option<String>,
    crash_dir: Option<PathBuf>,
    force_save_risk: bool,
    keep_profiles: bool,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let game_id = GameId::from(game.as_str());
    let profile = pm.load(&profile_name, Some(&game_id)).await?;
    if matches!(profile.source, ProfileSource::Wabbajack { .. }) {
        anyhow::bail!(
            "modde bisect cannot safely operate on Wabbajack-source profile '{profile_name}' yet. \
             Why required: Wabbajack deploy currently materializes staging directly and ignores per-mod enabled flags. \
             Upstream producer: Wabbajack deployment profile model. \
             Regenerate/fix by: add enabled-aware Wabbajack candidate deployment or import the list into a normal resolver-backed profile. \
             Validate with: modde deploy --profile <candidate> --game {game}"
        );
    }
    let profile_id = profile
        .id
        .ok_or_else(|| anyhow::anyhow!("profile '{profile_name}' is not stored in the database"))?;
    let suspect_mod_ids = profile
        .mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.mod_id.clone())
        .collect::<Vec<_>>();
    if suspect_mod_ids.len() < 2 {
        anyhow::bail!("bisect requires at least two enabled mods");
    }

    let oracle = match oracle {
        BisectOracleArg::Manual => BisectOracle::Manual,
        BisectOracleArg::Crash => {
            let crash_dir = match crash_dir {
                Some(path) => path,
                None => super::crash::first_existing_crash_log_dir(&game)?,
            };
            if !crash_dir.is_dir() {
                anyhow::bail!("crash directory does not exist: {}", crash_dir.display());
            }
            BisectOracle::Crash { crash_dir }
        }
        BisectOracleArg::Perf => {
            let baseline_run = baseline_run
                .ok_or_else(|| anyhow::anyhow!("--baseline-run is required for perf oracle"))?;
            let baseline = pm.db().load_performance_run(&baseline_run).await?;
            if baseline.status != "complete" {
                anyhow::bail!(
                    "baseline performance run '{baseline_run}' is not complete (status: {})",
                    baseline.status
                );
            }
            if baseline.game_id != game_id {
                anyhow::bail!(
                    "baseline performance run '{baseline_run}' belongs to '{}' not '{game}'",
                    baseline.game_id
                );
            }
            BisectOracle::Perf { baseline_run }
        }
    };

    let session_id = format!("b{}", time_id());
    pm.db()
        .create_bisect_session(&NewBisectSession {
            session_id: session_id.clone(),
            game_id,
            source_profile_id: profile_id,
            source_profile_name: profile_name.clone(),
            oracle,
            suspect_mod_ids,
            save_safety: if force_save_risk {
                BisectSaveSafety::Force
            } else {
                BisectSaveSafety::Refuse
            },
            keep_profiles,
        })
        .await?;

    println!("Started bisect session: {session_id}");
    println!("Source profile: {profile_name} ({game})");
    println!("Run the first candidate with: modde bisect run {session_id}");
    Ok(())
}

pub async fn handle_run(session_id: String) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    ensure_runnable(&session)?;
    if session.current_step_id.is_some() {
        anyhow::bail!(
            "session '{session_id}' already has a pending candidate; mark it with: modde bisect mark {session_id} good|bad"
        );
    }

    let source = pm
        .load(&session.source_profile_name, Some(&session.game_id))
        .await?;
    let dependencies = bisect_dependency_map(&pm, &source).await?;
    let Some(plan) = next_candidate_with_dependencies(&session, &dependencies) else {
        finish_without_candidate(&pm, &session).await?;
        return Ok(());
    };
    let step_index = pm.db().list_bisect_steps(&session_id).await?.len() + 1;
    let candidate_name = format!("__bisect_{}_{}", session.session_id, step_index);
    let candidate =
        create_candidate_profile(&pm, &source, &candidate_name, &plan.disabled_mod_ids).await?;
    enforce_save_safety(&session, &source, &candidate)?;

    let step_id = pm
        .db()
        .create_bisect_step(&NewBisectStep {
            session_id: session_id.clone(),
            step_index,
            candidate_profile: candidate_name.clone(),
            candidate_mod_ids: plan.candidate_mod_ids,
            enabled_mod_ids: plan.enabled_mod_ids,
            disabled_mod_ids: plan.disabled_mod_ids,
        })
        .await?;
    pm.db()
        .update_bisect_session_state(
            &session_id,
            BisectStatus::Waiting,
            &session.suspect_mod_ids,
            &session.known_good_mod_ids,
            &session.known_bad_mod_ids,
            Some(step_id),
            Some(&candidate_name),
        )
        .await?;

    match session.oracle.clone() {
        BisectOracle::Manual => {
            launch_candidate(&pm, &candidate_name, &session.game_id, false).await?;
            println!("Candidate launched: {candidate_name}");
            println!("Mark result with: modde bisect mark {session_id} good|bad");
        }
        BisectOracle::Crash { crash_dir } => {
            let started = SystemTime::now();
            let status = launch_candidate(&pm, &candidate_name, &session.game_id, false).await?;
            if status.is_none() {
                println!("Candidate launched via fire-and-forget launcher: {candidate_name}");
                println!("Mark result after testing with: modde bisect mark {session_id} good|bad");
                return Ok(());
            }
            let new_log = newest_file_after(&crash_dir, started)?;
            let (result, signal) = if let Some(log) = new_log {
                analyze_crash_log(&pm, &candidate, &log).await?;
                (
                    BisectResult::Bad,
                    Some(format!("new crash log: {}", log.display())),
                )
            } else {
                (
                    BisectResult::Good,
                    Some("no new crash log observed".to_string()),
                )
            };
            complete_and_advance(&pm, session_id, step_id, result, signal, None).await?;
        }
        BisectOracle::Perf { baseline_run } => {
            let run_id = run_perf_candidate(&pm, &candidate, &session.game_id, step_id).await?;
            let Some(run_id) = run_id else {
                println!("Candidate launched via fire-and-forget launcher: {candidate_name}");
                println!(
                    "After ingesting perf data, mark manually: modde bisect mark {session_id} good|bad"
                );
                return Ok(());
            };
            let baseline = pm.db().load_performance_run(&baseline_run).await?;
            let candidate_run = pm.db().load_performance_run(&run_id).await?;
            let bad = perf_regressed(&baseline.summary, &candidate_run.summary);
            complete_and_advance(
                &pm,
                session_id,
                step_id,
                if bad {
                    BisectResult::Bad
                } else {
                    BisectResult::Good
                },
                Some(format!("performance run: {run_id}")),
                None,
            )
            .await?;
        }
    }
    Ok(())
}

async fn bisect_dependency_map(
    pm: &ProfileManager,
    source: &Profile,
) -> Result<HashMap<String, Vec<String>>> {
    let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();
    for rule in &source.load_order_rules {
        match rule {
            modde_core::resolver::LoadOrderRule::LoadAfter { mod_id, after } => {
                dependencies
                    .entry(mod_id.to_string())
                    .or_default()
                    .push(after.to_string());
            }
            modde_core::resolver::LoadOrderRule::LoadBefore { mod_id, before } => {
                dependencies
                    .entry(before.to_string())
                    .or_default()
                    .push(mod_id.to_string());
            }
            modde_core::resolver::LoadOrderRule::Incompatible { .. } => {}
        }
    }

    if !matches!(
        source.game_id.as_str(),
        "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" | "starfield"
    ) {
        return Ok(dependencies);
    }

    let Some(profile_id) = source.id else {
        return Ok(dependencies);
    };
    let installed_files = pm.db().installed_files_for_profile(profile_id).await?;
    let mut plugin_owner_by_lower: HashMap<String, String> = HashMap::new();
    let mut plugin_path_by_lower: HashMap<String, PathBuf> = HashMap::new();
    for (mod_id, file) in installed_files {
        let rel_path = Path::new(&file.rel_path);
        let Some(file_name) = rel_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if is_bethesda_plugin_name(file_name) {
            let lower = file_name.to_ascii_lowercase();
            plugin_owner_by_lower.insert(lower.clone(), mod_id);
            plugin_path_by_lower.insert(lower, PathBuf::from(file.rel_path));
        }
    }
    for enabled_mod in source.mods.iter().filter(|m| m.enabled) {
        if let Some(plugin_name) = enabled_mod.mod_id.strip_prefix("plugin/")
            && is_bethesda_plugin_name(plugin_name)
        {
            plugin_owner_by_lower
                .entry(plugin_name.to_ascii_lowercase())
                .or_insert_with(|| enabled_mod.mod_id.clone());
            plugin_path_by_lower
                .entry(plugin_name.to_ascii_lowercase())
                .or_insert_with(|| PathBuf::from(plugin_name));
        }
    }

    let staging = ProfileManager::staging_dir(&source.name);
    for (plugin_lower, mod_id) in plugin_owner_by_lower.clone() {
        let Some(rel_path) = plugin_path_by_lower.get(&plugin_lower) else {
            continue;
        };
        let plugin_path = staging.join(rel_path);
        let header = modde_games::bethesda::plugin_header::parse_plugin_header(&plugin_path)
            .with_context(|| format!("failed to parse plugin header {}", plugin_path.display()));
        let Ok(header) = header else {
            continue;
        };
        for master in header.masters {
            if let Some(master_owner) = plugin_owner_by_lower.get(&master.to_ascii_lowercase())
                && master_owner != &mod_id
            {
                dependencies
                    .entry(mod_id.clone())
                    .or_default()
                    .push(master_owner.clone());
            }
        }
    }

    for deps in dependencies.values_mut() {
        deps.sort();
        deps.dedup();
    }
    Ok(dependencies)
}

fn is_bethesda_plugin_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".esp") || lower.ends_with(".esm") || lower.ends_with(".esl")
}

pub async fn handle_mark(
    session_id: String,
    result: BisectResultArg,
    notes: Option<String>,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    let step_id = session
        .current_step_id
        .ok_or_else(|| anyhow::anyhow!("session '{session_id}' has no pending candidate"))?;
    complete_and_advance(
        &pm,
        session_id,
        step_id,
        result.into(),
        Some("manual mark".to_string()),
        notes,
    )
    .await
}

pub async fn handle_status(session_id: String) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    let steps = pm.db().list_bisect_steps(&session_id).await?;
    println!("Bisect session: {}", session.session_id);
    println!("Game: {}", session.game_id);
    println!("Source profile: {}", session.source_profile_name);
    println!("Oracle: {}", session.oracle.as_str());
    println!("Status: {}", session.status.as_str());
    println!("Suspects: {}", session.suspect_mod_ids.len());
    if let Some(candidate) = &session.current_candidate_profile {
        println!("Current candidate: {candidate}");
    }
    for step in steps {
        let result = step.result.map(BisectResult::as_str).unwrap_or("pending");
        println!(
            "  #{} {} result={} disabled={}",
            step.step_index,
            step.candidate_profile,
            result,
            step.disabled_mod_ids.join(", ")
        );
    }
    print_completion_if_any(&pm, &session).await
}

pub async fn handle_history(session_id: String) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    let steps = pm.db().list_bisect_steps(&session_id).await?;
    println!("Bisect session: {}", session.session_id);
    println!("Status: {}", session.status.as_str());
    println!("Current suspects: {}", session.suspect_mod_ids.join(", "));
    for step in steps {
        let result = step.result.map(BisectResult::as_str).unwrap_or("pending");
        println!(
            "#{} {} result={result}",
            step.step_index, step.candidate_profile
        );
        println!(
            "  candidate disabled: {}",
            step.candidate_mod_ids.join(", ")
        );
        println!("  enabled half: {}", step.enabled_mod_ids.join(", "));
        println!("  disabled half: {}", step.disabled_mod_ids.join(", "));
        if let Some(signal) = step.observed_signal {
            println!("  signal: {signal}");
        }
        if let Some(notes) = step.notes {
            println!("  notes: {notes}");
        }
    }
    Ok(())
}

pub async fn handle_retry(session_id: String) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    let candidate = session
        .current_candidate_profile
        .as_deref()
        .ok_or_else(|| {
            anyhow::anyhow!("session '{session_id}' has no pending candidate to retry")
        })?;
    if session.current_step_id.is_none() || session.status != BisectStatus::Waiting {
        anyhow::bail!(
            "session '{session_id}' is not waiting on a pending candidate (status: {})",
            session.status.as_str()
        );
    }
    launch_candidate(&pm, candidate, &session.game_id, false).await?;
    println!("Retried candidate: {candidate}");
    println!("Mark result with: modde bisect mark {session_id} good|bad");
    Ok(())
}

pub async fn handle_abort(session_id: String) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    cleanup_candidates(&pm, &session).await?;
    pm.db()
        .update_bisect_session_state(
            &session_id,
            BisectStatus::Aborted,
            &session.suspect_mod_ids,
            &session.known_good_mod_ids,
            &session.known_bad_mod_ids,
            None,
            None,
        )
        .await?;
    restore_source_profile(&pm, &session).await?;
    println!("Aborted bisect session: {session_id}");
    Ok(())
}

fn ensure_runnable(session: &BisectSession) -> Result<()> {
    match session.status {
        BisectStatus::Active => Ok(()),
        BisectStatus::Waiting => anyhow::bail!(
            "session '{}' is waiting for a result; run: modde bisect mark {} good|bad",
            session.session_id,
            session.session_id
        ),
        _ => anyhow::bail!(
            "session '{}' is not active (status: {})",
            session.session_id,
            session.status.as_str()
        ),
    }
}

async fn create_candidate_profile(
    pm: &ProfileManager,
    source: &Profile,
    candidate_name: &str,
    disabled_mod_ids: &[String],
) -> Result<Profile> {
    let disabled: BTreeSet<&str> = disabled_mod_ids.iter().map(String::as_str).collect();
    let mut candidate = source.clone();
    candidate.id = None;
    candidate.name = candidate_name.to_string();
    candidate.overrides = ProfileManager::default_overrides(candidate_name);
    for m in &mut candidate.mods {
        if m.enabled && disabled.contains(m.mod_id.as_str()) {
            m.enabled = false;
        }
    }
    let source_id = source
        .id
        .ok_or_else(|| anyhow::anyhow!("source profile '{}' has no database ID", source.name))?;
    let candidate_id = pm.create(&candidate).await?;
    pm.db()
        .copy_profile_auxiliary_state(source_id, candidate_id)
        .await?;
    pm.load(candidate_name, Some(&source.game_id))
        .await
        .map_err(Into::into)
}

fn enforce_save_safety(
    session: &BisectSession,
    source: &Profile,
    candidate: &Profile,
) -> Result<()> {
    if session.save_safety == BisectSaveSafety::Force {
        return Ok(());
    }
    if !matches!(supports_save_profiles(session.game_id.as_str()), Ok(true)) {
        return Ok(());
    }
    let Some(game_plugin) = modde_games::resolve_game_plugin(session.game_id.as_str()) else {
        return Ok(());
    };
    let source_staging = ProfileManager::staging_dir(&source.name);
    let source_fp = SaveFingerprint::compute(&source.mods, |mod_id| {
        game_plugin
            .classify_mod(&source_staging.join(mod_id))
            .affects_saves()
    });
    let candidate_fp = SaveFingerprint::compute(&candidate.mods, |mod_id| {
        game_plugin
            .classify_mod(&source_staging.join(mod_id))
            .affects_saves()
    });
    if source_fp.hash == candidate_fp.hash {
        return Ok(());
    }
    let source_set: BTreeSet<_> = source_fp.mod_ids.iter().cloned().collect();
    let candidate_set: BTreeSet<_> = candidate_fp.mod_ids.iter().cloned().collect();
    let removed = source_set
        .difference(&candidate_set)
        .cloned()
        .collect::<Vec<_>>();
    let added = candidate_set
        .difference(&source_set)
        .cloned()
        .collect::<Vec<_>>();
    anyhow::bail!(
        "refusing unsafe bisect step because candidate '{}' changes the save-affecting mod set. removed=[{}] added=[{}]. Re-run start with --force-save-risk to allow this.",
        candidate.name,
        removed.join(", "),
        added.join(", ")
    );
}

async fn launch_candidate(
    pm: &ProfileManager,
    profile_name: &str,
    game_id: &GameId,
    no_deploy: bool,
) -> Result<Option<std::process::ExitStatus>> {
    let save_dir = resolve_save_dir(game_id.as_str());
    let fp = compute_fingerprint(pm, profile_name, game_id.as_str()).await;
    match pm
        .activate_with_fingerprint(profile_name, game_id, save_dir.as_deref(), fp.as_ref())
        .await?
    {
        ActivateResult::Activated => {}
        ActivateResult::AdoptionRequired { save_count } => {
            anyhow::bail!(
                "Found {save_count} unadopted save(s) in the game directory. Run `modde save adopt --game {game_id} --profile {profile_name}` first."
            );
        }
    }
    if !no_deploy {
        super::deploy::handle(Some(profile_name.to_string()), Some(game_id.to_string())).await?;
    }
    let detected = modde_games::find_detected_game(game_id)
        .ok_or_else(|| anyhow::anyhow!("could not detect launcher for '{game_id}'"))?;
    println!(
        "Launching candidate profile '{profile_name}' via {}...",
        detected.source
    );
    detected.source.launch()
}

async fn complete_and_advance(
    pm: &ProfileManager,
    session_id: String,
    step_id: i64,
    result: BisectResult,
    observed_signal: Option<String>,
    notes: Option<String>,
) -> Result<()> {
    let session = pm.db().load_bisect_session(&session_id).await?;
    let step = pm.db().load_bisect_step(step_id).await?;
    pm.db()
        .complete_bisect_step(
            step_id,
            result,
            observed_signal.as_deref(),
            notes.as_deref(),
        )
        .await?;

    let suspects = apply_result(&step, result);
    let mut known_good = session.known_good_mod_ids.clone();
    let mut known_bad = session.known_bad_mod_ids.clone();
    match result {
        BisectResult::Good => known_bad.extend(step.disabled_mod_ids.clone()),
        BisectResult::Bad => known_good.extend(step.disabled_mod_ids.clone()),
    }
    known_good.sort();
    known_good.dedup();
    known_bad.sort();
    known_bad.dedup();

    let status = if suspects.is_empty() {
        BisectStatus::Inconclusive
    } else if suspects.len() == 1 {
        BisectStatus::Complete
    } else {
        BisectStatus::Active
    };
    pm.db()
        .update_bisect_session_state(
            &session_id,
            status,
            &suspects,
            &known_good,
            &known_bad,
            None,
            None,
        )
        .await?;
    let updated = pm.db().load_bisect_session(&session_id).await?;
    if matches!(status, BisectStatus::Complete | BisectStatus::Inconclusive) {
        cleanup_candidates(pm, &updated).await?;
        restore_source_profile(pm, &updated).await?;
    }
    print_completion_if_any(pm, &updated).await?;
    if status == BisectStatus::Active {
        println!("Next candidate: modde bisect run {session_id}");
    }
    Ok(())
}

async fn finish_without_candidate(pm: &ProfileManager, session: &BisectSession) -> Result<()> {
    let status = if session.suspect_mod_ids.is_empty() {
        BisectStatus::Inconclusive
    } else {
        BisectStatus::Complete
    };
    pm.db()
        .update_bisect_session_state(
            &session.session_id,
            status,
            &session.suspect_mod_ids,
            &session.known_good_mod_ids,
            &session.known_bad_mod_ids,
            None,
            None,
        )
        .await?;
    let updated = pm.db().load_bisect_session(&session.session_id).await?;
    cleanup_candidates(pm, &updated).await?;
    restore_source_profile(pm, &updated).await?;
    print_completion_if_any(pm, &updated).await
}

async fn print_completion_if_any(pm: &ProfileManager, session: &BisectSession) -> Result<()> {
    match session.status {
        BisectStatus::Complete if session.suspect_mod_ids.len() == 1 => {
            let profile = pm
                .load(&session.source_profile_name, Some(&session.game_id))
                .await?;
            let id = &session.suspect_mod_ids[0];
            let entry = profile.mods.iter().find(|m| &m.mod_id == id);
            let display = entry
                .and_then(|m| m.display_name.as_deref())
                .unwrap_or(id.as_str());
            let version = entry
                .and_then(|m| m.version.as_deref())
                .unwrap_or("unknown");
            println!("Culprit: {display} ({id}, version: {version})");
        }
        BisectStatus::Complete => {
            println!(
                "Minimal suspect set: {}",
                session.suspect_mod_ids.join(", ")
            );
        }
        BisectStatus::Inconclusive => {
            println!("Bisect inconclusive: no reproducing suspect set remains.");
        }
        _ => {}
    }
    Ok(())
}

async fn cleanup_candidates(pm: &ProfileManager, session: &BisectSession) -> Result<()> {
    if session.keep_profiles {
        return Ok(());
    }
    for profile in pm
        .db()
        .bisect_candidate_profiles(&session.session_id)
        .await?
    {
        if profile.starts_with("__bisect_") {
            let _ = pm.delete(&profile, Some(&session.game_id)).await;
        }
    }
    Ok(())
}

async fn restore_source_profile(pm: &ProfileManager, session: &BisectSession) -> Result<()> {
    let save_dir = resolve_save_dir(session.game_id.as_str());
    let fp = compute_fingerprint(pm, &session.source_profile_name, session.game_id.as_str()).await;
    let _ = pm
        .activate_with_fingerprint(
            &session.source_profile_name,
            &session.game_id,
            save_dir.as_deref(),
            fp.as_ref(),
        )
        .await;
    Ok(())
}

async fn analyze_crash_log(pm: &ProfileManager, profile: &Profile, log_path: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(log_path)
        .with_context(|| format!("failed to read crash log {}", log_path.display()))?;
    let profile_id = profile.id.ok_or_else(|| {
        anyhow::anyhow!("candidate profile '{}' has no database ID", profile.name)
    })?;
    let active_plugins = load_plugin_order(pm, profile).await?;
    let enabled_mods = profile
        .mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.mod_id.as_str())
        .collect::<BTreeSet<_>>();
    let installed_files = pm
        .db()
        .installed_files_for_profile(profile_id)
        .await?
        .into_iter()
        .filter(|(mod_id, _)| enabled_mods.contains(mod_id.as_str()))
        .collect();
    let tool_files = pm.db().load_all_applied_files(&profile.game_id).await?;
    let report = modde_core::crash::correlate_crash_log(
        log_path,
        &raw,
        modde_core::crash::CrashLogFormat::Auto,
        modde_core::crash::CrashCorrelationInput {
            game_id: profile.game_id.to_string(),
            profile: profile.clone(),
            active_plugins,
            installed_files,
            tool_files,
        },
    );
    pm.db()
        .record_crash_log(Some(profile_id), &report, &raw)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

fn newest_file_after(dir: &Path, after: SystemTime) -> Result<Option<PathBuf>> {
    let mut newest = None;
    collect_new_files(dir, after, &mut newest)?;
    Ok(newest.map(|(_, path)| path))
}

fn collect_new_files(
    dir: &Path,
    after: SystemTime,
    newest: &mut Option<(SystemTime, PathBuf)>,
) -> Result<()> {
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            collect_new_files(&path, after, newest)?;
        } else if meta.is_file() {
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            if modified > after && newest.as_ref().is_none_or(|(old, _)| modified > *old) {
                *newest = Some((modified, path));
            }
        }
    }
    Ok(())
}

async fn run_perf_candidate(
    pm: &ProfileManager,
    profile: &Profile,
    game_id: &GameId,
    step_id: i64,
) -> Result<Option<String>> {
    let run_id = format!("bisect-{}-{step_id}", time_id());
    pm.db()
        .create_performance_run(&NewPerformanceRun {
            run_id: run_id.clone(),
            game_id: game_id.clone(),
            profile_id: profile.id,
            profile_name: profile.name.clone(),
            mod_snapshot: mod_snapshot(&profile.mods),
            experiment_depth: 0,
            label: Some(format!("bisect step {step_id}")),
        })
        .await?;
    let perf_dir = modde_core::paths::modde_data_dir()
        .join("performance")
        .join(game_id.as_str())
        .join(&run_id);
    std::fs::create_dir_all(&perf_dir)
        .with_context(|| format!("failed to create {}", perf_dir.display()))?;
    let config_path = perf_dir.join("MangoHud.conf");
    write_mangohud_config(&config_path, &perf_dir, &run_id, 300)?;
    let expected_csv = perf_dir.join(format!("{run_id}.csv"));
    let env_vars = vec![
        ("MANGOHUD".to_string(), "1".to_string()),
        (
            "MANGOHUD_CONFIG".to_string(),
            config_path.to_string_lossy().to_string(),
        ),
    ];
    let detected = modde_games::find_detected_game(game_id)
        .ok_or_else(|| anyhow::anyhow!("could not detect launcher for '{game_id}'"))?;
    println!("Performance candidate run: {run_id}");
    if let Some(status) = detected.source.launch_with_env(&env_vars)? {
        let csv_path = find_mangohud_csv(&perf_dir, &run_id).unwrap_or(expected_csv);
        let parsed = modde_core::parse_mangohud_csv_file(&csv_path)?;
        pm.db()
            .complete_performance_run(
                &run_id,
                &csv_path,
                status.code().map(i64::from),
                &parsed.summary,
                &parsed.samples,
            )
            .await?;
        Ok(Some(run_id))
    } else {
        pm.db()
            .mark_performance_run_pending(&run_id, Some(&expected_csv))
            .await?;
        Ok(None)
    }
}

fn perf_regressed(baseline: &PerformanceSummary, candidate: &PerformanceSummary) -> bool {
    let p99_bad = baseline
        .p99_frame_time_ms
        .zip(candidate.p99_frame_time_ms)
        .is_some_and(|(b, c)| c >= b * 1.15);
    let low_bad = baseline
        .one_percent_low_fps
        .zip(candidate.one_percent_low_fps)
        .is_some_and(|(b, c)| c <= b * 0.85);
    p99_bad || low_bad
}

fn write_mangohud_config(
    path: &Path,
    output_folder: &Path,
    run_id: &str,
    duration: u64,
) -> Result<()> {
    let content = format!(
        "\
no_display
fps
frametime
frame_timing
fps_metrics=0.01,0.001
benchmark_percentiles=97,AVG,1,0.1
autostart_log=1
log_duration={duration}
output_folder={}
output_file={run_id}
log_versioning=0
",
        output_folder.display()
    );
    std::fs::write(path, content)
        .with_context(|| format!("failed to write MangoHud config {}", path.display()))
}

fn find_mangohud_csv(dir: &Path, run_id: &str) -> Option<PathBuf> {
    let exact = dir.join(format!("{run_id}.csv"));
    if exact.is_file() {
        return Some(exact);
    }
    let mut candidates = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|e| e.to_str()) == Some("csv")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| name.contains(run_id))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
}

fn time_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}{:09}", now.as_secs(), now.subsec_nanos())
}
