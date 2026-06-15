use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{DEFAULT_RATE_LIMIT_BURST, DEFAULT_RATE_LIMIT_WINDOW_SECONDS};

#[derive(Clone)]
pub(super) struct RateLimiter {
    burst: u32,
    window: Duration,
    pub(super) trusted_proxy: bool,
    entries: Arc<Mutex<HashMap<String, RateLimitEntry>>>,
}

impl RateLimiter {
    pub(super) fn from_env() -> Self {
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

    pub(super) fn new(burst: u32, window: Duration, trusted_proxy: bool) -> Self {
        Self {
            burst: burst.max(1),
            window,
            trusted_proxy,
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(super) fn check(&self, key: &str) -> Result<(), ()> {
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
