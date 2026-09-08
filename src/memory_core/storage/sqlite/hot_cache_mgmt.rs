use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result};

use super::conn_pool::ConnPool;
use super::hot_cache::HotTierCache;

fn refresh_generation_bound_hot_cache(pool: &ConnPool, hot_cache: &HotTierCache) -> Result<()> {
    refresh_generation_bound_hot_cache_with(pool, hot_cache, |conn| hot_cache.refresh(conn))
}

fn refresh_generation_bound_hot_cache_with(
    pool: &ConnPool,
    hot_cache: &HotTierCache,
    refresh: impl FnOnce(&rusqlite::Connection) -> Result<()>,
) -> Result<()> {
    let result = (|| {
        {
            let conn = pool.reader()?;
            let snapshot = pool.embedding_snapshot(&conn)?;
            refresh(&snapshot)?;
        }
        // An overlapping migration may have committed while refresh held its
        // old snapshot. Release that snapshot AND its reader before validating
        // the live generation, including for single-connection pools.
        let conn = pool.reader()?;
        let _snapshot = pool.embedding_snapshot(&conn)?;
        Ok(())
    })();
    if result.is_err() {
        hot_cache.clear();
    }
    result
}

impl super::SqliteStorage {
    pub(super) async fn refresh_hot_cache(&self) -> Result<()> {
        self.start_hot_cache_refresh_task();
        let Some(hot_cache) = self.hot_cache.clone() else {
            return Ok(());
        };
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || refresh_generation_bound_hot_cache(&pool, &hot_cache))
            .await
            .context("spawn_blocking join error")?
    }

    pub(super) fn refresh_hot_cache_best_effort(&self) {
        let Some(hot_cache) = self.hot_cache.clone() else {
            return;
        };
        let pool = Arc::clone(&self.pool);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    refresh_generation_bound_hot_cache(&pool, &hot_cache)
                })
                .await;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(error = %error, "hot tier cache refresh failed");
                    }
                    Err(join_error) if join_error.is_panic() => {
                        tracing::error!(error = %join_error, "hot tier cache refresh task panicked");
                    }
                    Err(join_error) => {
                        tracing::warn!(error = %join_error, "hot tier cache refresh task cancelled");
                    }
                }
            });
        }
    }

    pub(super) async fn ensure_hot_cache_ready(&self) -> Result<()> {
        if let Some(hot_cache) = &self.hot_cache
            && !hot_cache.is_initialized()
        {
            self.refresh_hot_cache().await?;
        }
        Ok(())
    }

    fn start_hot_cache_refresh_task(&self) {
        if self
            .hot_cache_refresh_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let Some(hot_cache) = self.hot_cache.clone() else {
            self.hot_cache_refresh_started
                .store(false, Ordering::Release);
            return;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            tracing::warn!("hot tier cache background refresh skipped: no Tokio runtime available");
            self.hot_cache_refresh_started
                .store(false, Ordering::Release);
            return;
        };
        let weak_pool = Arc::downgrade(&self.pool);
        let weak_guard = Arc::downgrade(&self.hot_cache_refresh_guard);
        let refresh_interval = hot_cache.refresh_interval();
        handle.spawn(async move {
            let mut interval = tokio::time::interval(refresh_interval);
            loop {
                interval.tick().await;
                if weak_guard.upgrade().is_none() {
                    break;
                }
                let Some(pool) = weak_pool.upgrade() else {
                    break;
                };
                let hot_cache = hot_cache.clone();
                let result = tokio::task::spawn_blocking(move || {
                    refresh_generation_bound_hot_cache(&pool, &hot_cache)
                })
                .await;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(error = %error, "hot tier cache refresh failed");
                    }
                    Err(join_error) if join_error.is_panic() => {
                        tracing::error!(error = %join_error, "hot tier cache refresh task panicked");
                    }
                    Err(join_error) => {
                        tracing::warn!(error = %join_error, "hot tier cache refresh task cancelled");
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::schema;
    use super::*;
    use rusqlite::Connection;
    use std::time::Duration;

    #[test]
    fn hot_cache_refresh_rejects_migration_during_snapshot() -> Result<()> {
        // Same-space generation changes cover A -> B -> A without relying on
        // different vector dimensions or values to detect stale state.
        for target_space in ["space-b", "space-a"] {
            let dir = tempfile::tempdir()?;
            let path = dir.path().join("hot-cache.db");
            let pool = ConnPool::open_file(&path, 4, "space-a")?;
            {
                let writer = pool.writer()?;
                writer.execute(
                    "INSERT INTO memories (id, content, embedding, content_hash, source_type, access_count) VALUES ('alpha', 'alpha', ?1, 'alpha', 'user', 1)",
                    [super::super::encode_embedding(&[1.0, 0.0, 0.0, 0.0])],
                )?;
            }
            let cache = HotTierCache::new(10, Duration::from_secs(300));
            let error = refresh_generation_bound_hot_cache_with(&pool, &cache, |snapshot| {
                cache.refresh(snapshot)?;
                assert!(cache.is_initialized());
                assert_eq!(cache.query("alpha", 5).len(), 1);
                let mut concurrent = Connection::open(&path)?;
                let migration = concurrent.transaction()?;
                schema::advance_embedding_generation(&migration)?;
                schema::update_embedding_space_identity(&migration, target_space)?;
                migration.commit()?;
                Ok(())
            })
            .expect_err("hot cache refresh must reject migration during its snapshot");
            let expected = if target_space == "space-a" {
                "embedding generation mismatch"
            } else {
                "embedding space mismatch"
            };
            assert!(format!("{error:#}").contains(expected), "{error:#}");
            assert!(!cache.is_initialized());
            assert!(cache.query("alpha", 5).is_empty());
        }
        Ok(())
    }

    #[test]
    fn hot_cache_refresh_succeeds_without_migration() -> Result<()> {
        // Release the first reader even for the single-connection memory pool.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                let pool = ConnPool::open_in_memory(4, "space-a")?;
                let cache = HotTierCache::new(10, Duration::from_secs(300));
                refresh_generation_bound_hot_cache(&pool, &cache)?;
                assert!(cache.is_initialized());
                Ok::<_, anyhow::Error>(())
            })();
            let _ = tx.send(result);
        });
        rx.recv_timeout(Duration::from_secs(10))??;
        Ok(())
    }
}
