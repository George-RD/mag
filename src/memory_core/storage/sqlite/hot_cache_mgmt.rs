use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result};

impl super::SqliteStorage {
    pub(super) async fn refresh_hot_cache(&self) -> Result<()> {
        self.start_hot_cache_refresh_task();
        let Some(hot_cache) = self.hot_cache.clone() else {
            return Ok(());
        };
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            hot_cache.refresh(&conn)
        })
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
                    let conn = pool.reader()?;
                    hot_cache.refresh(&conn)
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
                    let conn = pool.reader()?;
                    hot_cache.refresh(&conn)
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
