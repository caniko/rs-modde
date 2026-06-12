//! Opt-in empirical mod compatibility oracle service for modde.
//!
//! # Deployment
//!
//! Per-IP rate limiting keys on the socket peer address by default. Set
//! `MODDE_ORACLE_TRUSTED_PROXY=1` (or `true`/`yes`) **only** when the service
//! is exclusively reachable through a reverse proxy that overwrites
//! `X-Forwarded-For`; the limiter then keys on the first `X-Forwarded-For`
//! hop (or `X-Real-IP`) instead. With the toggle off (the default) those
//! headers are ignored entirely, so direct clients cannot spoof their way
//! past the per-IP limit.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::rejection::ExtensionRejection;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use modde_oracle_api::{
    CompatAggregateStat, CompatEvent, CompatEventAccepted, CompatEventBatch, CompatEventKind,
    CompatQueryResponse, ConfidenceBucket, OracleConfigResponse,
};
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

const MAX_REQUEST_BYTES: usize = 256 * 1024;
const LAPLACE_NOISE_SCALE: f64 = 1.0;
const DEFAULT_RATE_LIMIT_WINDOW_SECONDS: u64 = 60;
const DEFAULT_RATE_LIMIT_BURST: u32 = 60;

#[derive(Clone)]
struct RateLimiter {
    burst: u32,
    window: Duration,
    trusted_proxy: bool,
    entries: Arc<Mutex<HashMap<String, RateLimitEntry>>>,
}

impl RateLimiter {
    fn from_env() -> Self {
        let burst = std::env::var("MODDE_ORACLE_RATE_LIMIT_BURST")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(DEFAULT_RATE_LIMIT_BURST);
        let window_seconds = std::env::var("MODDE_ORACLE_RATE_LIMIT_WINDOW_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_RATE_LIMIT_WINDOW_SECONDS);
        let trusted_proxy = std::env::var("MODDE_ORACLE_TRUSTED_PROXY")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes"
                )
            })
            .unwrap_or(false);
        Self::new(
            burst,
            Duration::from_secs(window_seconds.max(1)),
            trusted_proxy,
        )
    }

    fn new(burst: u32, window: Duration, trusted_proxy: bool) -> Self {
        Self {
            burst: burst.max(1),
            window,
            trusted_proxy,
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn check(&self, key: &str) -> Result<(), ()> {
        let now = Instant::now();
        let mut entries = self.entries.lock().map_err(|_| ())?;
        let entry = entries
            .entry(key.to_string())
            .or_insert_with(|| RateLimitEntry {
                window_start: now,
                count: 0,
            });
        if now.duration_since(entry.window_start) >= self.window {
            entry.window_start = now;
            entry.count = 0;
        }
        if entry.count >= self.burst {
            return Err(());
        }
        entry.count += 1;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct RateLimitEntry {
    window_start: Instant,
    count: u32,
}

#[derive(Clone)]
pub struct AppState {
    store: Store,
    config: OracleConfigResponse,
    rate_limiter: RateLimiter,
}

impl AppState {
    #[must_use]
    pub fn memory_for_tests(min_cohort: i64) -> Self {
        Self {
            store: Store::Memory(MemoryStore::default()),
            config: OracleConfigResponse {
                min_cohort,
                ..OracleConfigResponse::default()
            },
            rate_limiter: RateLimiter::from_env(),
        }
    }

    pub async fn postgres(database_url: &str, min_cohort: i64) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        migrate(&pool).await?;
        Ok(Self {
            store: Store::Postgres(pool),
            config: OracleConfigResponse {
                min_cohort,
                ..OracleConfigResponse::default()
            },
            rate_limiter: RateLimiter::from_env(),
        })
    }

    #[must_use]
    pub fn with_rate_limit(mut self, burst: u32, window: Duration) -> Self {
        self.rate_limiter = RateLimiter::new(burst, window, self.rate_limiter.trusted_proxy);
        self
    }

    #[must_use]
    pub fn with_trusted_proxy(mut self, trusted_proxy: bool) -> Self {
        self.rate_limiter.trusted_proxy = trusted_proxy;
        self
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/compat/config", get(config))
        .route("/v1/compat/events", post(post_events))
        .route("/v1/compat/query", get(query_compat))
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BYTES))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn serve(addr: SocketAddr, state: AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "modde compatibility oracle listening");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

async fn healthz() -> &'static str {
    "ok"
}

async fn config(State(state): State<AppState>) -> Json<OracleConfigResponse> {
    Json(state.config)
}

async fn post_events(
    State(state): State<AppState>,
    peer: Result<ConnectInfo<SocketAddr>, ExtensionRejection>,
    headers: HeaderMap,
    Json(batch): Json<CompatEventBatch>,
) -> Result<Json<CompatEventAccepted>, ApiError> {
    let key = rate_limit_key(
        &headers,
        peer.ok().map(|ConnectInfo(addr)| addr),
        state.rate_limiter.trusted_proxy,
    );
    state
        .rate_limiter
        .check(&key)
        .map_err(|()| ApiError::RateLimited)?;
    batch.validate()?;
    let accepted = state
        .store
        .ingest_events(&batch.events, state.config.min_cohort)
        .await?;
    Ok(Json(CompatEventAccepted { accepted }))
}

fn rate_limit_key(headers: &HeaderMap, peer: Option<SocketAddr>, trusted_proxy: bool) -> String {
    if trusted_proxy {
        let forwarded = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(forwarded) = forwarded {
            return forwarded.to_string();
        }
    }
    peer.map_or_else(|| "unknown".to_string(), |addr| addr.ip().to_string())
}

#[derive(Debug, Deserialize)]
struct CompatQuery {
    game_id: String,
    pair_hash: String,
}

async fn query_compat(
    State(state): State<AppState>,
    Query(query): Query<CompatQuery>,
) -> Result<Json<CompatQueryResponse>, ApiError> {
    let stats = state
        .store
        .query_stats(
            &query.game_id,
            &query
                .pair_hash
                .split(',')
                .filter(|hash| !hash.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>(),
            state.config.min_cohort,
        )
        .await?;
    Ok(Json(CompatQueryResponse {
        game_id: query.game_id,
        stats,
    }))
}

#[derive(Clone)]
enum Store {
    Postgres(PgPool),
    Memory(MemoryStore),
}

impl Store {
    async fn ingest_events(
        &self,
        events: &[CompatEvent],
        min_cohort: i64,
    ) -> Result<usize, ApiError> {
        match self {
            Self::Postgres(pool) => insert_postgres_events(pool, events, min_cohort).await,
            Self::Memory(store) => {
                let accepted = store.insert_events(events);
                store.recompute_aggregates(min_cohort);
                Ok(accepted)
            }
        }
    }

    async fn query_stats(
        &self,
        game_id: &str,
        pair_hashes: &[String],
        min_cohort: i64,
    ) -> Result<Vec<CompatAggregateStat>, ApiError> {
        match self {
            Self::Postgres(pool) => {
                query_postgres_stats(pool, game_id, pair_hashes, min_cohort).await
            }
            Self::Memory(store) => Ok(store.query_stats(game_id, pair_hashes)),
        }
    }
}

#[derive(Clone, Default)]
struct MemoryStore {
    inner: Arc<Mutex<MemoryInner>>,
}

#[derive(Default)]
struct MemoryInner {
    events: Vec<CompatEvent>,
    aggregates: Vec<CompatAggregateStat>,
}

impl MemoryStore {
    fn insert_events(&self, events: &[CompatEvent]) -> usize {
        let mut inner = self.inner.lock().expect("memory store poisoned");
        inner.events.extend_from_slice(events);
        events.len()
    }

    fn recompute_aggregates(&self, min_cohort: i64) {
        let mut inner = self.inner.lock().expect("memory store poisoned");
        inner.aggregates = aggregate_events(&inner.events, min_cohort);
    }

    fn query_stats(&self, game_id: &str, pair_hashes: &[String]) -> Vec<CompatAggregateStat> {
        let inner = self.inner.lock().expect("memory store poisoned");
        inner
            .aggregates
            .iter()
            .filter(|stat| pair_hashes.contains(&stat.pair_hash))
            .filter(|stat| {
                inner.events.iter().any(|event| {
                    event.game_id == game_id && event.pair_hashes.contains(&stat.pair_hash)
                })
            })
            .cloned()
            .collect()
    }
}

fn aggregate_events(events: &[CompatEvent], min_cohort: i64) -> Vec<CompatAggregateStat> {
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

fn noisy_rate(crashes: i64, total: i64) -> f64 {
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

fn confidence(total: i64) -> ConfidenceBucket {
    if total >= 1_000 {
        ConfidenceBucket::High
    } else if total >= 200 {
        ConfidenceBucket::Medium
    } else {
        ConfidenceBucket::Low
    }
}

async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
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

async fn insert_postgres_events(
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

async fn query_postgres_stats(
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

#[derive(Debug)]
enum ApiError {
    BadRequest(String),
    RateLimited,
    Store(anyhow::Error),
}

impl From<modde_oracle_api::ValidationError> for ApiError {
    fn from(value: modde_oracle_api::ValidationError) -> Self {
        Self::BadRequest(value.to_string())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(value: sqlx::Error) -> Self {
        Self::Store(value.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest(message) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": message })),
            )
                .into_response(),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({ "error": "compatibility event rate limit exceeded" })),
            )
                .into_response(),
            Self::Store(error) => {
                tracing::error!(%error, "compatibility oracle storage error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "storage error" })),
                )
                    .into_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request, StatusCode};
    use modde_oracle_api::{CURRENT_SCHEMA_VERSION, CompatEventBatch};
    use tower::ServiceExt;

    use super::*;

    fn event(kind: CompatEventKind, pair_hash: &str, observed_at_unix: i64) -> CompatEvent {
        CompatEvent {
            game_id: "skyrim-se".to_string(),
            platform: "x86_64-linux".to_string(),
            salt_epoch: "epoch-1".to_string(),
            mod_set_hash: "a".repeat(64),
            mod_hashes: vec!["b".repeat(64), "c".repeat(64)],
            pair_hashes: vec![pair_hash.to_string()],
            crash_signature_hash: (kind == CompatEventKind::Crash).then(|| "d".repeat(64)),
            kind,
            observed_at_unix,
        }
    }

    #[tokio::test]
    async fn route_rejects_raw_path_payload() {
        let app = router(AppState::memory_for_tests(1));
        let batch = CompatEventBatch {
            schema_version: CURRENT_SCHEMA_VERSION,
            client_version: "0.3.9".to_string(),
            events: vec![CompatEvent {
                game_id: "/home/user/crash.log".to_string(),
                ..event(CompatEventKind::Session, &"e".repeat(64), 1)
            }],
        };
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/compat/events")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&batch).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn query_suppresses_small_cohorts() {
        let app = router(AppState::memory_for_tests(3));
        let pair = "e".repeat(64);
        let batch = CompatEventBatch {
            schema_version: CURRENT_SCHEMA_VERSION,
            client_version: "0.3.9".to_string(),
            events: vec![
                event(CompatEventKind::Session, &pair, 1),
                event(CompatEventKind::Crash, &pair, 2),
            ],
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/compat/events")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&batch).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ok(response).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/v1/compat/query?game_id=skyrim-se&pair_hash={pair}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let response = assert_ok(response).await;
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: CompatQueryResponse = serde_json::from_slice(&body).unwrap();
        assert!(parsed.stats.is_empty());
    }

    #[tokio::test]
    async fn aggregate_math_reports_lift() {
        let app = router(AppState::memory_for_tests(1));
        let pair = "f".repeat(64);
        let other = "a".repeat(64);
        let batch = CompatEventBatch {
            schema_version: CURRENT_SCHEMA_VERSION,
            client_version: "0.3.9".to_string(),
            events: vec![
                event(CompatEventKind::Session, &pair, 1),
                event(CompatEventKind::Crash, &pair, 2),
                event(CompatEventKind::Session, &other, 3),
                event(CompatEventKind::Session, &other, 4),
            ],
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/compat/events")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&batch).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ok(response).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/v1/compat/query?game_id=skyrim-se&pair_hash={pair}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: CompatQueryResponse = serde_json::from_slice(&body).unwrap();
        let stat = parsed.stats.first().expect("thresholded stat");
        assert_eq!(stat.coinstall_count, 2);
        assert!((stat.crash_signature_rate - 0.5).abs() < f64::EPSILON);
        assert!((stat.baseline_rate - 0.25).abs() < f64::EPSILON);
        assert!((stat.lift - 2.0).abs() < f64::EPSILON);
    }

    fn peer(addr: &str) -> ConnectInfo<SocketAddr> {
        ConnectInfo(addr.parse().expect("valid socket address"))
    }

    fn events_request(body: &[u8], peer_addr: &str, forwarded_for: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri("/v1/compat/events")
            .header("content-type", "application/json")
            .extension(peer(peer_addr));
        if let Some(forwarded_for) = forwarded_for {
            builder = builder.header("x-forwarded-for", forwarded_for);
        }
        builder.body(Body::from(body.to_vec())).unwrap()
    }

    fn rate_limited_batch_body() -> Vec<u8> {
        let pair = "9".repeat(64);
        let batch = CompatEventBatch {
            schema_version: CURRENT_SCHEMA_VERSION,
            client_version: "0.3.9".to_string(),
            events: vec![event(CompatEventKind::Session, &pair, 1)],
        };
        serde_json::to_vec(&batch).unwrap()
    }

    #[tokio::test]
    async fn post_events_rate_limits_by_peer_address() {
        let app = router(AppState::memory_for_tests(1).with_rate_limit(1, Duration::from_mins(1)));
        let body = rate_limited_batch_body();
        let first = app
            .clone()
            .oneshot(events_request(&body, "203.0.113.10:50000", None))
            .await
            .unwrap();
        assert_ok(first).await;

        let second = app
            .oneshot(events_request(&body, "203.0.113.10:50001", None))
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn spoofed_forwarded_for_is_ignored_without_trusted_proxy() {
        let app = router(AppState::memory_for_tests(1).with_rate_limit(1, Duration::from_mins(1)));
        let body = rate_limited_batch_body();
        let first = app
            .clone()
            .oneshot(events_request(
                &body,
                "203.0.113.10:50000",
                Some("198.51.100.1"),
            ))
            .await
            .unwrap();
        assert_ok(first).await;

        let second = app
            .oneshot(events_request(
                &body,
                "203.0.113.10:50001",
                Some("198.51.100.2"),
            ))
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn trusted_proxy_honors_forwarded_for() {
        let app = router(
            AppState::memory_for_tests(1)
                .with_rate_limit(1, Duration::from_mins(1))
                .with_trusted_proxy(true),
        );
        let body = rate_limited_batch_body();
        let first = app
            .clone()
            .oneshot(events_request(
                &body,
                "127.0.0.1:50000",
                Some("198.51.100.1"),
            ))
            .await
            .unwrap();
        assert_ok(first).await;

        let second = app
            .clone()
            .oneshot(events_request(
                &body,
                "127.0.0.1:50001",
                Some("198.51.100.2"),
            ))
            .await
            .unwrap();
        assert_ok(second).await;

        let third = app
            .oneshot(events_request(
                &body,
                "127.0.0.1:50002",
                Some("198.51.100.1"),
            ))
            .await
            .unwrap();
        assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    async fn assert_ok(response: axum::response::Response) -> axum::response::Response {
        let status = response.status();
        if status != StatusCode::OK {
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            panic!(
                "expected 200, got {status}: {}",
                String::from_utf8_lossy(&body)
            );
        }
        response
    }
}
