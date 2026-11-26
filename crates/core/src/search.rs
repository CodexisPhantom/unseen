//! Short-lived in-memory cache of manual search results so that a grab can
//! refer to a release by id without re-querying indexers or exposing
//! indexer download URLs containing credentials to clients.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Default lifetime of a cached search result (15 minutes).
pub const DEFAULT_TTL: Duration = Duration::from_secs(15 * 60);

/// Default upper bound on cached releases across searches.
pub const DEFAULT_MAX_ENTRIES: usize = 4096;

/// Everything a later grab needs for one searched release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedRelease {
    pub movie_id: i32,
    pub movie_profile_id: i32,
    pub indexer_id: i32,
    pub indexer_name: String,
    pub guid: String,
    pub title: String,
    pub download_url: Option<String>,
    pub magnet_url: Option<String>,
    pub info_hash: Option<String>,
    pub size: Option<u64>,
    pub quality_id: i32,
    pub custom_format_score: i32,
    pub protocol: String,
    pub accepted: bool,
}

#[derive(Debug, Clone)]
struct Entry {
    release: CachedRelease,
    expires_at: Instant,
    seq: u64,
}

#[derive(Debug)]
struct Inner {
    entries: HashMap<String, Entry>,
    next_seq: u64,
}

/// Thread-safe, bounded, TTL-expiring cache of search results keyed by a
/// caller-generated release id.
#[derive(Debug, Clone)]
pub struct SearchCache {
    ttl: Duration,
    max_entries: usize,
    inner: Arc<Mutex<Inner>>,
}

impl Default for SearchCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchCache {
    /// Creates a cache with [`DEFAULT_TTL`] and [`DEFAULT_MAX_ENTRIES`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(DEFAULT_TTL, DEFAULT_MAX_ENTRIES)
    }

    /// Creates a cache with a custom time-to-live and capacity.
    #[must_use]
    pub fn with_config(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries,
            inner: Arc::new(Mutex::new(Inner {
                entries: HashMap::new(),
                next_seq: 0,
            })),
        }
    }

    /// Configured entry lifetime.
    #[must_use]
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Stores one release under `id`, pruning expired entries and evicting
    /// the oldest entries if capacity is reached.
    pub fn insert(&self, id: String, release: CachedRelease) {
        self.insert_at(id, release, Instant::now());
    }

    /// [`Self::insert`] using an explicit timestamp.
    pub fn insert_at(&self, id: String, release: CachedRelease, now: Instant) {
        self.insert_batch_at(std::iter::once((id, release)), now);
    }

    /// Stores a batch of releases under a single lock acquisition.
    pub fn insert_batch(&self, items: impl IntoIterator<Item = (String, CachedRelease)>) {
        self.insert_batch_at(items, Instant::now());
    }

    /// [`Self::insert_batch`] using an explicit timestamp.
    pub fn insert_batch_at(
        &self,
        items: impl IntoIterator<Item = (String, CachedRelease)>,
        now: Instant,
    ) {
        if self.max_entries == 0 {
            return;
        }
        let mut inner = self.inner.lock().expect("search cache lock");
        inner.entries.retain(|_, entry| entry.expires_at > now);
        let expires_at = now + self.ttl;
        for (id, release) in items {
            if !inner.entries.contains_key(&id)
                && inner.entries.len() >= self.max_entries
                && let Some(oldest_key) = inner
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| (entry.expires_at, entry.seq))
                    .map(|(key, _)| key.clone())
            {
                inner.entries.remove(&oldest_key);
            }
            inner.next_seq = inner.next_seq.wrapping_add(1);
            let seq = inner.next_seq;
            inner.entries.insert(
                id,
                Entry {
                    release,
                    expires_at,
                    seq,
                },
            );
        }
    }

    /// Looks up a release by `id`. Returns `None` if absent or expired,
    /// removing an expired entry on access.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<CachedRelease> {
        self.get_at(id, Instant::now())
    }

    /// [`Self::get`] using an explicit timestamp.
    #[must_use]
    pub fn get_at(&self, id: &str, now: Instant) -> Option<CachedRelease> {
        let mut inner = self.inner.lock().expect("search cache lock");
        match inner.entries.get(id) {
            Some(entry) if entry.expires_at > now => Some(entry.release.clone()),
            Some(_) => {
                inner.entries.remove(id);
                None
            }
            None => None,
        }
    }

    /// Removes and returns a live entry by `id`, or `None` if absent or expired.
    #[must_use]
    pub fn remove(&self, id: &str) -> Option<CachedRelease> {
        self.remove_at(id, Instant::now())
    }

    /// [`Self::remove`] using an explicit timestamp.
    #[must_use]
    pub fn remove_at(&self, id: &str, now: Instant) -> Option<CachedRelease> {
        let mut inner = self.inner.lock().expect("search cache lock");
        inner
            .entries
            .remove(id)
            .filter(|entry| entry.expires_at > now)
            .map(|entry| entry.release)
    }

    /// Drops every expired entry.
    pub fn prune(&self) {
        self.prune_at(Instant::now());
    }

    /// [`Self::prune`] using an explicit timestamp.
    pub fn prune_at(&self, now: Instant) {
        let mut inner = self.inner.lock().expect("search cache lock");
        inner.entries.retain(|_, entry| entry.expires_at > now);
    }

    /// Current number of stored entries (including any not yet pruned).
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().expect("search cache lock").entries.len()
    }

    /// Whether the cache currently holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(title: &str) -> CachedRelease {
        CachedRelease {
            movie_id: 1,
            movie_profile_id: 10,
            indexer_id: 2,
            indexer_name: "Alpha".to_owned(),
            guid: format!("guid:{title}"),
            title: title.to_owned(),
            download_url: Some(format!("https://tracker.example/dl/{title}")),
            magnet_url: None,
            info_hash: Some("aabbccddeeff00112233445566778899aabbccdd".to_owned()),
            size: Some(4_000_000_000),
            quality_id: 7,
            custom_format_score: 0,
            protocol: "torrent".to_owned(),
            accepted: true,
        }
    }

    #[test]
    fn stores_looks_up_and_expires_entries_past_ttl() {
        let cache = SearchCache::with_config(Duration::from_secs(60), 10);
        let t0 = Instant::now();
        let rel = sample("Matrix");
        cache.insert_at("r1".to_owned(), rel.clone(), t0);

        assert_eq!(cache.ttl(), Duration::from_secs(60));
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
        assert_eq!(cache.get_at("r1", t0 + Duration::from_secs(59)), Some(rel));
        assert_eq!(cache.get_at("missing", t0), None);

        // At or after TTL the entry is expired and removed on lookup.
        assert_eq!(cache.get_at("r1", t0 + Duration::from_secs(60)), None);
        assert!(cache.is_empty());
    }

    #[test]
    fn prunes_expired_entries_and_evicts_oldest_when_full() {
        let cache = SearchCache::with_config(Duration::from_secs(30), 2);
        let t0 = Instant::now();
        cache.insert_at("a".to_owned(), sample("A"), t0);
        cache.insert_at("b".to_owned(), sample("B"), t0 + Duration::from_secs(5));
        assert_eq!(cache.len(), 2);

        // Capacity reached: oldest ("a") is evicted.
        cache.insert_at("c".to_owned(), sample("C"), t0 + Duration::from_secs(10));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get_at("a", t0 + Duration::from_secs(10)), None);
        assert!(cache.get_at("b", t0 + Duration::from_secs(10)).is_some());
        assert!(cache.get_at("c", t0 + Duration::from_secs(10)).is_some());

        // Inserting after "b" expires prunes "b" without evicting "c".
        cache.insert_at("d".to_owned(), sample("D"), t0 + Duration::from_secs(36));
        assert_eq!(cache.len(), 2);
        assert!(cache.get_at("c", t0 + Duration::from_secs(36)).is_some());
        assert!(cache.get_at("d", t0 + Duration::from_secs(36)).is_some());

        // Explicit prune clears everything past expiry.
        cache.prune_at(t0 + Duration::from_secs(100));
        assert!(cache.is_empty());
    }

    #[test]
    fn default_cache_insert_batch_remove_and_zero_capacity() {
        let cache = SearchCache::default();
        assert_eq!(cache.ttl(), DEFAULT_TTL);
        cache.insert("k1".to_owned(), sample("One"));
        cache.insert_batch([
            ("k2".to_owned(), sample("Two")),
            ("k3".to_owned(), sample("Three")),
        ]);
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get("k1").unwrap().title, "One");
        assert_eq!(cache.remove("k2").unwrap().title, "Two");
        assert!(cache.get("k2").is_none());
        cache.prune();
        assert_eq!(cache.len(), 2);

        // Zero-capacity cache stores nothing.
        let disabled = SearchCache::with_config(Duration::from_secs(60), 0);
        disabled.insert("x".to_owned(), sample("X"));
        assert!(disabled.is_empty());
    }

    #[test]
    fn remove_at_ignores_expired_entries() {
        let cache = SearchCache::with_config(Duration::from_secs(10), 4);
        let t0 = Instant::now();
        cache.insert_at("r1".to_owned(), sample("A"), t0);
        assert_eq!(cache.remove_at("r1", t0 + Duration::from_secs(10)), None);
        assert!(cache.is_empty());
    }
}
