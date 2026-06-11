use std::net::SocketAddr;

use anyhow::Context;
use modde_oracle::AppState;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let database_url = std::env::var("MODDE_ORACLE_DATABASE_URL")
        .context("MODDE_ORACLE_DATABASE_URL is required")?;
    let addr = std::env::var("MODDE_ORACLE_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3917".to_string())
        .parse::<SocketAddr>()
        .context("invalid MODDE_ORACLE_BIND")?;
    let min_cohort = std::env::var("MODDE_ORACLE_MIN_COHORT")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(modde_oracle_api::DEFAULT_MIN_COHORT);

    let state = AppState::postgres(&database_url, min_cohort).await?;
    modde_oracle::serve(addr, state).await
}
