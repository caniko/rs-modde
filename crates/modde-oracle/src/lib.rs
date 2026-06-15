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

mod aggregate;
mod app;
mod error;
mod postgres;
mod rate_limit;
mod store;

#[cfg(test)]
mod tests;

pub use app::{AppState, router, serve};

const MAX_REQUEST_BYTES: usize = 256 * 1024;
const LAPLACE_NOISE_SCALE: f64 = 1.0;
const DEFAULT_RATE_LIMIT_WINDOW_SECONDS: u64 = 60;
const DEFAULT_RATE_LIMIT_BURST: u32 = 60;
