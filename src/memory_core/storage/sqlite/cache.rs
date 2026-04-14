use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::memory_core::SemanticResult;

/// Query result cache TTL in seconds.
pub(super) const QUERY_CACHE_TTL_SECS: u64 = 60;
/// Maximum number of entries in the query result cache.
const QUERY_CACHE_CAPACITY: usize = 128;

/// A cached query result with filter metadata for selective invalidation.
///
/// Storing the filter dimensions that were active when the query was cached
/// allows `invalidate_cache_selective` to evict only entries whose results
/// *could* be affected by a write with known attributes, instead of clearing
/// the entire cache on every write.
#[derive(Clone, Debug)]
pub(super) struct CachedQuery {
    pub(super) inserted_at: Instant,
    pub(super) results: Vec<SemanticResult>,
    /// The event_type filter active when this query was cached (None = any type could match).
    pub(super) event_type_filter: Option<String>,
    /// The project filter active when this query was cached (None = any project could match).
    pub(super) project_filter: Option<String>,
    /// The session_id filter active when this query was cached (None = any session could match).
    pub(super) session_id_filter: Option<String>,
}

pub(super) type QueryCache = Arc<Mutex<lru::LruCache<u64, CachedQuery>>>;

pub(super) fn new_query_cache() -> QueryCache {
    Arc::new(Mutex::new(lru::LruCache::new(
        NonZeroUsize::new(QUERY_CACHE_CAPACITY).expect("cache capacity must be non-zero"),
    )))
}

impl super::SqliteStorage {
    /// Clears all query and hot cache entries unconditionally.
    ///
    /// Use for broad mutations (import, schema migration, clear_session)
    /// where the scope of change is unknown or affects many records.
    pub(super) fn invalidate_query_cache(&self) {
        if let Ok(mut cache) = self.query_cache.lock() {
            cache.clear();
        }
        if let Some(hot_cache) = &self.hot_cache {
            hot_cache.clear();
        }
    }

    /// Selectively invalidate cache entries that *could* be affected by a
    /// write with the given attributes.
    ///
    /// A cached query is evicted when:
    /// - It had **no filter** on a dimension (could match anything), OR
    /// - Its filter **matches** the written memory's attribute on that dimension.
    ///
    /// Entries whose filters *definitely exclude* the written memory are kept.
    ///
    /// The hot cache is always cleared because it is ordered by access count
    /// and any write could shift rankings.
    pub(super) fn invalidate_cache_selective(
        &self,
        event_type: Option<&str>,
        project: Option<&str>,
        session_id: Option<&str>,
    ) {
        if let Ok(mut cache) = self.query_cache.lock() {
            // Collect keys to evict (cannot mutate while iterating the LRU).
            let keys_to_evict: Vec<u64> = cache
                .iter()
                .filter_map(|(&key, entry)| {
                    if Self::cache_entry_could_be_affected(entry, event_type, project, session_id) {
                        Some(key)
                    } else {
                        None
                    }
                })
                .collect();

            for key in keys_to_evict {
                cache.pop(&key);
            }
        }

        // Hot cache is ranked by access_count; any write can shift rankings.
        if let Some(hot_cache) = &self.hot_cache {
            hot_cache.clear();
        }
    }

    /// Returns `true` when a cached entry *could* contain results affected by
    /// a write with the given attributes.
    pub(super) fn cache_entry_could_be_affected(
        entry: &CachedQuery,
        written_event_type: Option<&str>,
        written_project: Option<&str>,
        written_session_id: Option<&str>,
    ) -> bool {
        // For each dimension: if the cached query had no filter, the write
        // could affect its results. If it had a filter, it's only affected
        // when the filter matches the written value.
        let event_type_affected = match (&entry.event_type_filter, written_event_type) {
            (None, _) => true, // unfiltered query — any type could match
            (_, None) => true, // written memory has no type — could appear in any query
            (Some(f), Some(w)) => f == w,
        };
        let project_affected = match (&entry.project_filter, written_project) {
            (None, _) => true,
            (_, None) => true,
            (Some(f), Some(w)) => f == w,
        };
        let session_affected = match (&entry.session_id_filter, written_session_id) {
            (None, _) => true,
            (_, None) => true,
            (Some(f), Some(w)) => f == w,
        };

        event_type_affected && project_affected && session_affected
    }
}
