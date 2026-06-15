use std::collections::BTreeMap;

use modde_oracle_api::{CompatAggregateStat, CompatEvent, CompatEventKind};
use sqlx::{PgPool, Row};

use crate::aggregate::{confidence, noisy_rate};
use crate::error::ApiError;

pub(super) async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    for statement in [
        "
        CREATE TABLE IF NOT EXISTS compat_events (
            id BIGSERIAL PRIMARY KEY,
            game_id TEXT NOT NULL,
            platform TEXT NOT NULL,
            salt_epoch TEXT NOT NULL,
            mod_set_hash TEXT NOT NULL,
            mod_hashes TEXT[] NOT NULL,
            pair_hashes TEXT[] NOT NULL,
            crash_signature_hash TEXT,
            kind TEXT NOT NULL,
            observed_at_unix BIGINT NOT NULL,
            received_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        ",
        "
        CREATE INDEX IF NOT EXISTS idx_compat_events_game_received
            ON compat_events(game_id, received_at)
        ",
        "
        CREATE INDEX IF NOT EXISTS idx_compat_events_pair_hashes
            ON compat_events USING GIN(pair_hashes)
        ",
        "
        CREATE TABLE IF NOT EXISTS compat_pair_aggregates (
            game_id TEXT NOT NULL,
            pair_hash TEXT NOT NULL,
            coinstall_count BIGINT NOT NULL,
            crash_count BIGINT NOT NULL,
            baseline_count BIGINT NOT NULL,
            baseline_crash_count BIGINT NOT NULL,
            window_start_unix BIGINT NOT NULL,
            window_end_unix BIGINT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY (game_id, pair_hash)
        )
        ",
    ] {
        sqlx::query(statement).execute(pool).await?;
    }
    Ok(())
}

pub(super) async fn insert_postgres_events(
    pool: &PgPool,
    events: &[CompatEvent],
    min_cohort: i64,
) -> Result<usize, ApiError> {
    let touched = touched_pairs(events);
    let mut tx = pool.begin().await?;
    for event in events {
        sqlx::query(
            "
            INSERT INTO compat_events
                (game_id, platform, salt_epoch, mod_set_hash, mod_hashes, pair_hashes,
                 crash_signature_hash, kind, observed_at_unix)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ",
        )
        .bind(&event.game_id)
        .bind(&event.platform)
        .bind(&event.salt_epoch)
        .bind(&event.mod_set_hash)
        .bind(&event.mod_hashes)
        .bind(&event.pair_hashes)
        .bind(&event.crash_signature_hash)
        .bind(match event.kind {
            CompatEventKind::Session => "session",
            CompatEventKind::Crash => "crash",
        })
        .bind(event.observed_at_unix)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    update_postgres_aggregates(pool, &touched, min_cohort).await?;
    Ok(events.len())
}

#[derive(Debug, Default)]
struct TouchedAggregates {
    games: BTreeMap<String, Vec<String>>,
}

fn touched_pairs(events: &[CompatEvent]) -> TouchedAggregates {
    let mut touched = TouchedAggregates::default();
    for event in events {
        let pairs = touched.games.entry(event.game_id.clone()).or_default();
        for pair in &event.pair_hashes {
            if !pairs.contains(pair) {
                pairs.push(pair.clone());
            }
        }
    }
    touched
}

async fn update_postgres_aggregates(
    pool: &PgPool,
    touched: &TouchedAggregates,
    min_cohort: i64,
) -> Result<(), ApiError> {
    for (game_id, pair_hashes) in &touched.games {
        sqlx::query(
            "DELETE FROM compat_pair_aggregates
             WHERE game_id = $1 AND pair_hash = ANY($2)",
        )
        .bind(game_id)
        .bind(pair_hashes)
        .execute(pool)
        .await?;

        sqlx::query(
            "
            INSERT INTO compat_pair_aggregates
                (game_id, pair_hash, coinstall_count, crash_count, baseline_count,
                 baseline_crash_count, window_start_unix, window_end_unix)
            WITH baseline AS (
                SELECT
                    game_id,
                    count(*)::BIGINT AS baseline_count,
                    count(*) FILTER (WHERE kind = 'crash')::BIGINT AS baseline_crash_count
                FROM compat_events
                WHERE game_id = $1
                GROUP BY game_id
            ),
            pairs AS (
                SELECT
                    e.game_id,
                    pair_hash,
                    count(*)::BIGINT AS coinstall_count,
                    count(*) FILTER (WHERE e.kind = 'crash')::BIGINT AS crash_count,
                    min(e.observed_at_unix)::BIGINT AS window_start_unix,
                    max(e.observed_at_unix)::BIGINT AS window_end_unix
                FROM compat_events e
                CROSS JOIN LATERAL unnest(e.pair_hashes) AS pair_hash
                WHERE e.game_id = $1 AND pair_hash = ANY($2)
                GROUP BY e.game_id, pair_hash
                HAVING count(*) >= $3
            )
            SELECT
                pairs.game_id,
                pairs.pair_hash,
                pairs.coinstall_count,
                pairs.crash_count,
                baseline.baseline_count,
                baseline.baseline_crash_count,
                pairs.window_start_unix,
                pairs.window_end_unix
            FROM pairs
            JOIN baseline ON baseline.game_id = pairs.game_id
            ",
        )
        .bind(game_id)
        .bind(pair_hashes)
        .bind(min_cohort)
        .execute(pool)
        .await?;

        sqlx::query(
            "
            WITH baseline AS (
                SELECT
                    count(*)::BIGINT AS baseline_count,
                    count(*) FILTER (WHERE kind = 'crash')::BIGINT AS baseline_crash_count
                FROM compat_events
                WHERE game_id = $1
            )
            UPDATE compat_pair_aggregates
            SET baseline_count = baseline.baseline_count,
                baseline_crash_count = baseline.baseline_crash_count,
                updated_at = now()
            FROM baseline
            WHERE compat_pair_aggregates.game_id = $1
            ",
        )
        .bind(game_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub(super) async fn query_postgres_stats(
    pool: &PgPool,
    game_id: &str,
    pair_hashes: &[String],
    min_cohort: i64,
) -> Result<Vec<CompatAggregateStat>, ApiError> {
    let rows = sqlx::query(
        "
        SELECT pair_hash, coinstall_count, crash_count, baseline_count, baseline_crash_count,
               window_start_unix, window_end_unix
        FROM compat_pair_aggregates
        WHERE game_id = $1
          AND pair_hash = ANY($2)
          AND coinstall_count >= $3
        ORDER BY pair_hash
        ",
    )
    .bind(game_id)
    .bind(pair_hashes)
    .bind(min_cohort)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let pair_hash: String = row.try_get("pair_hash")?;
            let coinstall_count: i64 = row.try_get("coinstall_count")?;
            let crash_count: i64 = row.try_get("crash_count")?;
            let baseline_count: i64 = row.try_get("baseline_count")?;
            let baseline_crash_count: i64 = row.try_get("baseline_crash_count")?;
            let crash_signature_rate = noisy_rate(crash_count, coinstall_count);
            let baseline_rate = noisy_rate(baseline_crash_count, baseline_count);
            Ok(CompatAggregateStat {
                pair_hash,
                coinstall_count,
                crash_signature_rate,
                baseline_rate,
                lift: if baseline_rate > 0.0 {
                    crash_signature_rate / baseline_rate
                } else {
                    0.0
                },
                confidence: confidence(coinstall_count),
                window_start_unix: row.try_get("window_start_unix")?,
                window_end_unix: row.try_get("window_end_unix")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(ApiError::from)
}
