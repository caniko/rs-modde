use std::collections::{BTreeMap, HashMap};

use modde_oracle_api::{CompatAggregateStat, CompatEvent, CompatEventKind, ConfidenceBucket};

use crate::LAPLACE_NOISE_SCALE;

pub(super) fn aggregate_events(
    events: &[CompatEvent],
    min_cohort: i64,
) -> Vec<CompatAggregateStat> {
    let mut baseline: HashMap<String, Counts> = HashMap::new();
    let mut pair_counts: BTreeMap<(String, String), Counts> = BTreeMap::new();

    for event in events {
        let base = baseline
            .entry(event.game_id.clone())
            .or_insert_with(|| Counts {
                start: event.observed_at_unix,
                end: event.observed_at_unix,
                ..Counts::default()
            });
        update_counts(base, event);

        for pair in &event.pair_hashes {
            let counts = pair_counts
                .entry((event.game_id.clone(), pair.clone()))
                .or_insert_with(|| Counts {
                    start: event.observed_at_unix,
                    end: event.observed_at_unix,
                    ..Counts::default()
                });
            update_counts(counts, event);
        }
    }

    pair_counts
        .into_iter()
        .filter_map(|((game_id, pair_hash), counts)| {
            (counts.total >= min_cohort).then(|| {
                let baseline = baseline.get(&game_id).expect("baseline exists");
                let crash_signature_rate = noisy_rate(counts.crashes, counts.total);
                let baseline_rate = noisy_rate(baseline.crashes, baseline.total);
                CompatAggregateStat {
                    pair_hash,
                    coinstall_count: counts.total,
                    crash_signature_rate,
                    baseline_rate,
                    lift: if baseline_rate > 0.0 {
                        crash_signature_rate / baseline_rate
                    } else {
                        0.0
                    },
                    confidence: confidence(counts.total),
                    window_start_unix: counts.start,
                    window_end_unix: counts.end,
                }
            })
        })
        .collect()
}

#[derive(Default)]
struct Counts {
    total: i64,
    crashes: i64,
    start: i64,
    end: i64,
}

fn update_counts(counts: &mut Counts, event: &CompatEvent) {
    counts.total += 1;
    if event.kind == CompatEventKind::Crash {
        counts.crashes += 1;
    }
    if event.observed_at_unix < counts.start {
        counts.start = event.observed_at_unix;
    }
    if event.observed_at_unix > counts.end {
        counts.end = event.observed_at_unix;
    }
}

fn rate(crashes: i64, total: i64) -> f64 {
    if total == 0 {
        0.0
    } else {
        crashes as f64 / total as f64
    }
}

pub(super) fn noisy_rate(crashes: i64, total: i64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    rate(noisy_count(crashes, total), total)
}

fn noisy_count(count: i64, total: i64) -> i64 {
    let noisy = count as f64 + laplace_noise(LAPLACE_NOISE_SCALE);
    noisy.round().clamp(0.0, total as f64) as i64
}

fn laplace_noise(scale: f64) -> f64 {
    #[cfg(test)]
    {
        let _ = scale;
        0.0
    }
    #[cfg(not(test))]
    {
        let mut bytes = [0_u8; 8];
        if getrandom::fill(&mut bytes).is_err() {
            return 0.0;
        }
        let raw = u64::from_le_bytes(bytes) >> 11;
        let u = (raw as f64 + 0.5) / ((1_u64 << 53) as f64);
        if u < 0.5 {
            scale * (2.0 * u).ln()
        } else {
            -scale * (2.0 * (1.0 - u)).ln()
        }
    }
}

pub(super) fn confidence(total: i64) -> ConfidenceBucket {
    if total >= 1_000 {
        ConfidenceBucket::High
    } else if total >= 200 {
        ConfidenceBucket::Medium
    } else {
        ConfidenceBucket::Low
    }
}
