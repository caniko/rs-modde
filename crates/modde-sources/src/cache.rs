//! In-memory byte cache for extracted archive entries, keyed by archive hash
//! and inner path, with a configurable byte budget and LRU eviction.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use lru::LruCache;
use parking_lot::Mutex;

/// Cache key identifying one extracted entry: its source archive hash plus the
/// inner path within that archive.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ByteCacheKey {
    pub archive_hash: u64,
    pub inner_path: String,
}

/// Byte-budgeted LRU cache of extracted archive entry contents.
pub struct ByteLruCache {
    map: Mutex<LruCache<ByteCacheKey, Bytes>>,
    bytes_budget: AtomicU64,
    bytes_used: AtomicU64,
}

impl ByteLruCache {
    /// Build a cache sized from the `MODDE_BYTE_CACHE_MIB` environment variable
    /// (defaulting to 512 MiB).
    #[must_use]
    pub fn from_env() -> Self {
        let mib = std::env::var("MODDE_BYTE_CACHE_MIB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(512);
        Self::new(mib * 1024 * 1024)
    }

    /// Build a cache holding at most `bytes_budget` bytes of cached entries.
    #[must_use]
    pub fn new(bytes_budget: u64) -> Self {
        Self {
            map: Mutex::new(LruCache::new(NonZeroUsize::new(1024).unwrap())),
            bytes_budget: AtomicU64::new(bytes_budget),
            bytes_used: AtomicU64::new(0),
        }
    }

    /// Return the cached bytes for `key`, marking the entry most-recently-used.
    #[must_use]
    pub fn get(&self, key: &ByteCacheKey) -> Option<Bytes> {
        self.map.lock().get(key).cloned()
    }

    /// Insert `bytes` under `key`, evicting least-recently-used entries to stay
    /// within budget; returns the inserted `bytes` for chaining.
    pub fn insert(&self, key: ByteCacheKey, bytes: Bytes) -> Bytes {
        let len = bytes.len() as u64;
        let budget = self.bytes_budget.load(Ordering::Relaxed);
        if len > budget {
            return bytes;
        }

        let mut map = self.map.lock();
        if let Some(old) = map.put(key, bytes.clone()) {
            self.bytes_used
                .fetch_sub(old.len() as u64, Ordering::Relaxed);
        }
        self.bytes_used.fetch_add(len, Ordering::Relaxed);

        while self.bytes_used.load(Ordering::Relaxed) > budget {
            let Some((_key, evicted)) = map.pop_lru() else {
                break;
            };
            self.bytes_used
                .fetch_sub(evicted.len() as u64, Ordering::Relaxed);
        }
        bytes
    }

    /// Drop all cached entries belonging to the given `archive_hash`.
    pub fn invalidate_archive(&self, archive_hash: u64) {
        let mut map = self.map.lock();
        let keys = map
            .iter()
            .filter_map(|(key, _)| (key.archive_hash == archive_hash).then_some(key.clone()))
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(value) = map.pop(&key) {
                self.bytes_used
                    .fetch_sub(value.len() as u64, Ordering::Relaxed);
            }
        }
    }

    /// Return the total number of bytes currently held in the cache.
    #[must_use]
    pub fn bytes_used(&self) -> u64 {
        self.bytes_used.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_respects_budget() {
        let cache = ByteLruCache::new(32);
        for i in 0..10 {
            cache.insert(
                ByteCacheKey {
                    archive_hash: 1,
                    inner_path: format!("{i}.bin"),
                },
                Bytes::from(vec![i; 8]),
            );
        }
        assert!(cache.bytes_used() <= 32);
    }
}
