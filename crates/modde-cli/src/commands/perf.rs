use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use modde_core::db::NewPerformanceRun;
use modde_core::performance::{PerformanceModSnapshot, mod_snapshot};
use modde_core::profile::{ActivateResult, ProfileManager};
use modde_core::resolver::GameId;
use modde_core::save::SaveManager;

use super::{compute_fingerprint, resolve_save_dir, supports_save_profiles};

pub async fn handle_run(
    profile_name: Option<String>,
    game_id: String,
    duration: u64,
    label: Option<String>,
    warmup_seconds: f64,
    no_deploy: bool,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let game = GameId::from(game_id.as_str());

    let target_profile = match profile_name {
        Some(name) => name,
        None => pm
            .active(&game)
            .await?
            .map(|info| info.profile.name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no active profile for game '{game_id}'. Specify one: modde perf run <profile> --game {game_id}"
                )
            })?,
    };

    let already_active = pm
        .active(&game)
        .await?
        .is_some_and(|info| info.profile.name == target_profile);
    if !already_active {
        let save_dir = resolve_save_dir(&game_id);
        let fp = compute_fingerprint(&pm, &target_profile, &game_id).await;
        match pm
            .activate_with_fingerprint(&target_profile, &game, save_dir.as_deref(), fp.as_ref())
            .await?
        {
            ActivateResult::Activated => println!("Switched to profile: {target_profile}"),
            ActivateResult::AdoptionRequired { save_count } => {
                anyhow::bail!(
                    "Found {save_count} unadopted save(s) in the game directory.\nRun `modde save adopt --game {game_id} --profile {target_profile}` first."
                );
            }
        }
    }

    if !no_deploy {
        super::deploy::handle(Some(target_profile.clone()), Some(game_id.clone())).await?;
    }

    let active = pm
        .active(&game)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no active profile for game '{game_id}' after switch"))?;
    let profile = active.profile;
    let snapshot = mod_snapshot(&profile.mods);
    let run_id = new_run_id(&game_id);
    pm.db()
        .create_performance_run(&NewPerformanceRun {
            run_id: run_id.clone(),
            game_id: game.clone(),
            profile_id: profile.id,
            profile_name: profile.name.clone(),
            mod_snapshot: snapshot,
            experiment_depth: active.experiment_depth,
            label,
        })
        .await?;

    let perf_dir = modde_core::paths::modde_data_dir()
        .join("performance")
        .join(&game_id)
        .join(&run_id);
    std::fs::create_dir_all(&perf_dir)
        .with_context(|| format!("failed to create {}", perf_dir.display()))?;
    let config_path = perf_dir.join("MangoHud.conf");
    write_mangohud_config(&config_path, &perf_dir, &run_id, duration)?;
    let expected_csv = perf_dir.join(format!("{run_id}.csv"));

    let detected = modde_games::find_detected_game(&game)
        .ok_or_else(|| anyhow::anyhow!("could not detect launcher for '{game_id}'"))?;
    let env_vars = vec![
        ("MANGOHUD".to_string(), "1".to_string()),
        (
            "MANGOHUD_CONFIG".to_string(),
            config_path.to_string_lossy().to_string(),
        ),
    ];

    println!("Performance run: {run_id}");
    println!("Launching via {}...", detected.source);
    let exit_status = detected.source.launch_with_env(&env_vars)?;
    if let Some(status) = exit_status {
        let csv_path = find_mangohud_csv(&perf_dir, &run_id).unwrap_or(expected_csv);
        let parsed = modde_core::parse_mangohud_csv_file_with_warmup(&csv_path, warmup_seconds)?;
        pm.db()
            .complete_performance_run(
                &run_id,
                &csv_path,
                status.code().map(i64::from),
                &parsed.summary,
                &parsed.samples,
            )
            .await?;
        println!("Captured: {}", csv_path.display());
        print_summary(&parsed.summary);
        capture_saves_after_run(&pm, &profile.name, &game_id).await;
    } else {
        pm.db()
            .mark_performance_run_pending(&run_id, Some(&expected_csv))
            .await?;
        println!("Game launched via Steam (fire-and-forget).");
        println!(
            "Run is pending. After MangoHud writes the CSV, ingest it with:\n  modde perf ingest --run {run_id} --csv {}",
            expected_csv.display()
        );
    }

    Ok(())
}

pub async fn handle_ingest(run_id: String, csv: PathBuf, warmup_seconds: f64) -> Result<()> {
    let db = modde_core::ModdeDb::open()
        .await
        .context("failed to open database")?;
    let parsed = modde_core::parse_mangohud_csv_file_with_warmup(&csv, warmup_seconds)?;
    db.complete_performance_run(&run_id, &csv, None, &parsed.summary, &parsed.samples)
        .await?;
    println!("Ingested performance run: {run_id}");
    print_summary(&parsed.summary);
    Ok(())
}

pub async fn handle_list(game_id: String, profile: Option<String>, limit: usize) -> Result<()> {
    let db = modde_core::ModdeDb::open()
        .await
        .context("failed to open database")?;
    let rows = db
        .list_performance_runs(&GameId::from(game_id.as_str()), profile.as_deref(), limit)
        .await?;
    if rows.is_empty() {
        println!("No performance runs found.");
        return Ok(());
    }
    for row in rows {
        let one_low = format_opt(row.summary.one_percent_low_fps, " fps");
        println!(
            "{}  {}  {}  profile={}  1% low={}",
            row.run_id, row.started_at, row.status, row.profile_name, one_low
        );
        if let Some(label) = row.label {
            println!("  label: {label}");
        }
    }
    Ok(())
}

pub async fn handle_show(run_id: String) -> Result<()> {
    let db = modde_core::ModdeDb::open()
        .await
        .context("failed to open database")?;
    let row = db.load_performance_run(&run_id).await?;
    println!("Run: {}", row.run_id);
    println!("Game: {}", row.game_id);
    println!("Profile: {}", row.profile_name);
    println!("Status: {}", row.status);
    println!("Started: {}", row.started_at);
    if let Some(path) = row.mangohud_csv_path {
        println!("CSV: {}", path.display());
    }
    if let Some(label) = row.label {
        println!("Label: {label}");
    }
    print_summary(&row.summary);
    Ok(())
}

pub async fn handle_compare(baseline: String, candidate: String) -> Result<()> {
    let db = modde_core::ModdeDb::open()
        .await
        .context("failed to open database")?;
    let baseline = db.load_performance_run(&baseline).await?;
    let candidate = db.load_performance_run(&candidate).await?;

    println!("Baseline:  {} ({})", baseline.run_id, baseline.profile_name);
    println!(
        "Candidate: {} ({})",
        candidate.run_id, candidate.profile_name
    );
    print_metric_delta(
        "Median FPS",
        baseline.summary.median_fps,
        candidate.summary.median_fps,
        "fps",
    );
    print_metric_delta(
        "Average FPS",
        baseline.summary.average_fps,
        candidate.summary.average_fps,
        "fps",
    );
    print_metric_delta(
        "1% low FPS",
        baseline.summary.one_percent_low_fps,
        candidate.summary.one_percent_low_fps,
        "fps",
    );
    print_metric_delta(
        "0.1% low FPS",
        baseline.summary.point_one_percent_low_fps,
        candidate.summary.point_one_percent_low_fps,
        "fps",
    );
    print_metric_delta(
        "p99 frame time",
        baseline.summary.p99_frame_time_ms,
        candidate.summary.p99_frame_time_ms,
        "ms",
    );
    print_mod_diff(&baseline.mod_snapshot_json, &candidate.mod_snapshot_json)?;
    Ok(())
}

fn write_mangohud_config(
    path: &Path,
    output_folder: &Path,
    run_id: &str,
    duration: u64,
) -> Result<()> {
    let content = format!(
        "\
# Generated by modde perf; do not edit.
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

async fn capture_saves_after_run(pm: &ProfileManager, profile_name: &str, game_id: &str) {
    let Some(save_dir) = resolve_save_dir(game_id) else {
        return;
    };
    if matches!(supports_save_profiles(game_id), Ok(false)) {
        return;
    }
    let sm = SaveManager::new(pm.db());
    let fp = compute_fingerprint(pm, profile_name, game_id).await;
    if let Err(e) =
        sm.capture_with_fingerprint(&GameId::from(game_id), profile_name, &save_dir, fp.as_ref())
    {
        eprintln!("Warning: save auto-capture failed: {e}");
    }
}

fn print_summary(summary: &modde_core::PerformanceSummary) {
    println!("Samples: {}", summary.sample_count);
    println!("Median FPS: {}", format_opt(summary.median_fps, " fps"));
    println!("Average FPS: {}", format_opt(summary.average_fps, " fps"));
    println!(
        "1% low FPS: {}",
        format_opt(summary.one_percent_low_fps, " fps")
    );
    println!(
        "0.1% low FPS: {}",
        format_opt(summary.point_one_percent_low_fps, " fps")
    );
    println!(
        "p99 frame time: {}",
        format_opt(summary.p99_frame_time_ms, " ms")
    );
}

fn print_metric_delta(name: &str, baseline: Option<f64>, candidate: Option<f64>, unit: &str) {
    match (baseline, candidate) {
        (Some(b), Some(c)) if b != 0.0 => {
            let delta = c - b;
            let pct = delta / b * 100.0;
            println!("{name}: {b:.2} -> {c:.2} {unit} ({delta:+.2}, {pct:+.1}%)");
        }
        (Some(b), Some(c)) => println!("{name}: {b:.2} -> {c:.2} {unit}"),
        _ => println!("{name}: unavailable"),
    }
}

fn print_mod_diff(baseline_json: &str, candidate_json: &str) -> Result<()> {
    let baseline = decode_mods(baseline_json)?;
    let candidate = decode_mods(candidate_json)?;
    let base_ids: BTreeSet<_> = baseline.keys().cloned().collect();
    let cand_ids: BTreeSet<_> = candidate.keys().cloned().collect();

    let added: Vec<_> = cand_ids.difference(&base_ids).cloned().collect();
    let removed: Vec<_> = base_ids.difference(&cand_ids).cloned().collect();
    let changed: Vec<_> = base_ids
        .intersection(&cand_ids)
        .filter(|id| baseline.get(*id) != candidate.get(*id))
        .cloned()
        .collect();

    println!("Mod changes:");
    print_mod_list("Added", &added, &candidate);
    print_mod_list("Removed", &removed, &baseline);
    if changed.is_empty() {
        println!("  Changed: none");
    } else {
        println!("  Changed:");
        for id in changed {
            println!(
                "    {}: {} -> {}",
                id,
                baseline
                    .get(&id)
                    .and_then(|v| v.as_deref())
                    .unwrap_or("(none)"),
                candidate
                    .get(&id)
                    .and_then(|v| v.as_deref())
                    .unwrap_or("(none)")
            );
        }
    }
    Ok(())
}

fn decode_mods(json: &str) -> Result<BTreeMap<String, Option<String>>> {
    let mods: Vec<PerformanceModSnapshot> = serde_json::from_str(json)
        .with_context(|| "stored performance mod snapshot is invalid JSON")?;
    Ok(mods
        .into_iter()
        .map(|m| (m.mod_id, m.version))
        .collect::<BTreeMap<_, _>>())
}

fn print_mod_list(label: &str, ids: &[String], mods: &BTreeMap<String, Option<String>>) {
    if ids.is_empty() {
        println!("  {label}: none");
    } else {
        println!("  {label}:");
        for id in ids {
            match mods.get(id).and_then(|v| v.as_deref()) {
                Some(version) => println!("    {id} ({version})"),
                None => println!("    {id}"),
            }
        }
    }
}

fn format_opt(value: Option<f64>, suffix: &str) -> String {
    value
        .map(|v| format!("{v:.2}{suffix}"))
        .unwrap_or_else(|| "unavailable".to_string())
}

fn new_run_id(game_id: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let normalized = game_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    format!("{normalized}-{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_diff_decodes_snapshots() {
        let mods = vec![PerformanceModSnapshot {
            mod_id: "a".into(),
            version: Some("1".into()),
            enabled: true,
        }];
        let json = serde_json::to_string(&mods).unwrap();
        let decoded = decode_mods(&json).unwrap();
        assert_eq!(decoded.get("a").and_then(|v| v.as_deref()), Some("1"));
    }

    #[test]
    fn mod_hash_is_order_independent() {
        let mut a = vec![
            PerformanceModSnapshot {
                mod_id: "b".into(),
                version: None,
                enabled: true,
            },
            PerformanceModSnapshot {
                mod_id: "a".into(),
                version: Some("1".into()),
                enabled: true,
            },
        ];
        let mut b = a.clone();
        b.reverse();
        assert_eq!(
            modde_core::performance::mod_set_hash(&a),
            modde_core::performance::mod_set_hash(&b)
        );
        a[0].version = Some("2".into());
        assert_ne!(
            modde_core::performance::mod_set_hash(&a),
            modde_core::performance::mod_set_hash(&b)
        );
    }
}
