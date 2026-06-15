use std::sync::{Arc, Mutex};

use modde_oracle_api::{CompatAggregateStat, CompatEvent};
use sqlx::PgPool;

use crate::aggregate::aggregate_events;
use crate::error::ApiError;
use crate::postgres::{insert_postgres_events, query_postgres_stats};

#[derive(Clone)]
pub(super) enum Store {
    Postgres(PgPool),
    Memory(MemoryStore),
}

impl Store {
    pub(super) async fn ingest_events(
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

    pub(super) async fn query_stats(
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
pub(super) struct MemoryStore {
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
