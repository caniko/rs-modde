//! Performance oracle and statistical regression helpers.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use modde_core::performance::{DEFAULT_WARMUP_SECONDS, mod_snapshot};
use modde_core::profile::{Profile, ProfileManager};
use modde_core::{
    GameId, NewPerformanceRun, PerformanceSample, PerformanceSummary,
};


pub(super) async fn run_perf_candidate(
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

#[derive(Debug, Clone, Copy)]
pub(super) struct PerfRegressionConfig {
    pub(super) p99_frame_time_ratio: f64,
    pub(super) one_percent_low_fps_ratio: f64,
    pub(super) alpha: f64,
    pub(super) min_samples: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PerfRegressionVerdict {
    pub(super) regressed: bool,
    pub(super) signal: String,
}

pub(super) fn perf_regression_verdict(
    baseline: &PerformanceSummary,
    candidate: &PerformanceSummary,
    baseline_samples: &[PerformanceSample],
    candidate_samples: &[PerformanceSample],
    config: PerfRegressionConfig,
) -> PerfRegressionVerdict {
    let baseline_filtered = samples_after_warmup(baseline_samples);
    let candidate_filtered = samples_after_warmup(candidate_samples);
    let baseline_fps = fps_values(&baseline_filtered);
    let candidate_fps = fps_values(&candidate_filtered);
    let baseline_frame_times = frame_time_values(&baseline_filtered);
    let candidate_frame_times = frame_time_values(&candidate_filtered);

    let p99_regressed = baseline
        .p99_frame_time_ms
        .zip(candidate.p99_frame_time_ms)
        .is_some_and(|(base, cand)| {
            cand >= base * config.p99_frame_time_ratio
                && variance_gate_regressed(
                    &baseline_frame_times,
                    &candidate_frame_times,
                    Direction::HigherIsWorse,
                    config,
                )
        });
    let low_regressed = baseline
        .one_percent_low_fps
        .zip(candidate.one_percent_low_fps)
        .is_some_and(|(base, cand)| {
            cand <= base * config.one_percent_low_fps_ratio
                && variance_gate_regressed(
                    &baseline_fps,
                    &candidate_fps,
                    Direction::LowerIsWorse,
                    config,
                )
        });

    let signal = format!(
        "p99={} -> {}; 1% low={} -> {}; samples={} -> {}",
        format_metric(baseline.p99_frame_time_ms, "ms"),
        format_metric(candidate.p99_frame_time_ms, "ms"),
        format_metric(baseline.one_percent_low_fps, "fps"),
        format_metric(candidate.one_percent_low_fps, "fps"),
        baseline_fps.len(),
        candidate_fps.len()
    );
    PerfRegressionVerdict {
        regressed: p99_regressed || low_regressed,
        signal,
    }
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    HigherIsWorse,
    LowerIsWorse,
}

fn samples_after_warmup(samples: &[PerformanceSample]) -> Vec<PerformanceSample> {
    let Some(start) = samples
        .iter()
        .filter_map(|sample| sample.elapsed_seconds)
        .filter(|value| value.is_finite())
        .min_by(f64::total_cmp)
    else {
        return samples.to_vec();
    };
    samples
        .iter()
        .filter(|sample| {
            sample
                .elapsed_seconds
                .is_none_or(|elapsed| elapsed - start >= DEFAULT_WARMUP_SECONDS)
        })
        .cloned()
        .collect()
}

fn fps_values(samples: &[PerformanceSample]) -> Vec<f64> {
    samples
        .iter()
        .map(|sample| sample.fps)
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect()
}

fn frame_time_values(samples: &[PerformanceSample]) -> Vec<f64> {
    samples
        .iter()
        .filter_map(|sample| sample.frame_time_ms)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .collect()
}

fn variance_gate_regressed(
    baseline: &[f64],
    candidate: &[f64],
    direction: Direction,
    config: PerfRegressionConfig,
) -> bool {
    if baseline.is_empty() || candidate.is_empty() {
        return false;
    }
    if baseline.len() >= config.min_samples && candidate.len() >= config.min_samples {
        return welch_significant(baseline, candidate, direction, config.alpha);
    }
    let Some(base_mean) = mean(baseline) else {
        return false;
    };
    let Some(candidate_mean) = mean(candidate) else {
        return false;
    };
    let cv = coefficient_of_variation(baseline).unwrap_or(0.0);
    let delta = match direction {
        Direction::HigherIsWorse => (candidate_mean - base_mean) / base_mean.max(f64::EPSILON),
        Direction::LowerIsWorse => (base_mean - candidate_mean) / base_mean.max(f64::EPSILON),
    };
    delta.is_finite() && delta > cv
}

fn welch_significant(
    baseline: &[f64],
    candidate: &[f64],
    direction: Direction,
    alpha: f64,
) -> bool {
    let Some(base_mean) = mean(baseline) else {
        return false;
    };
    let Some(candidate_mean) = mean(candidate) else {
        return false;
    };
    let base_var = sample_variance(baseline).unwrap_or(0.0);
    let candidate_var = sample_variance(candidate).unwrap_or(0.0);
    let standard_error =
        ((base_var / baseline.len() as f64) + (candidate_var / candidate.len() as f64)).sqrt();
    if !standard_error.is_finite() || standard_error <= f64::EPSILON {
        return match direction {
            Direction::HigherIsWorse => candidate_mean > base_mean,
            Direction::LowerIsWorse => candidate_mean < base_mean,
        };
    }
    let t = match direction {
        Direction::HigherIsWorse => (candidate_mean - base_mean) / standard_error,
        Direction::LowerIsWorse => (base_mean - candidate_mean) / standard_error,
    };
    t.is_finite() && t >= normal_critical_one_sided(alpha)
}

fn normal_critical_one_sided(alpha: f64) -> f64 {
    if alpha <= 0.001 {
        3.09
    } else if alpha <= 0.01 {
        2.33
    } else if alpha <= 0.025 {
        1.96
    } else if alpha <= 0.05 {
        1.645
    } else if alpha <= 0.10 {
        1.282
    } else {
        1.0
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn sample_variance(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let avg = mean(values)?;
    Some(
        values
            .iter()
            .map(|value| (value - avg).powi(2))
            .sum::<f64>()
            / (values.len() - 1) as f64,
    )
}

fn coefficient_of_variation(values: &[f64]) -> Option<f64> {
    let avg = mean(values)?;
    let stddev = sample_variance(values)?.sqrt();
    (avg.abs() > f64::EPSILON).then_some(stddev / avg.abs())
}

fn format_metric(value: Option<f64>, unit: &str) -> String {
    value.map_or_else(|| "n/a".to_string(), |value| format!("{value:.2} {unit}"))
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

pub(super) fn time_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}{:09}", now.as_secs(), now.subsec_nanos())
}
