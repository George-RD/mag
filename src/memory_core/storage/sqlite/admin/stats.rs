//! Statistics and analytics for the SQLite storage backend.
//!
//! Implements the `StatsProvider` trait, providing aggregated views of the
//! memory store: per-type counts, per-session counts, weekly digest with
//! growth metrics, and access-rate analytics (including top-accessed memories).

use super::super::*;

#[async_trait]
impl StatsProvider for SqliteStorage {
    async fn type_stats(&self) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let mut stmt = conn
                .prepare("SELECT COALESCE(event_type, 'untyped') as etype, COUNT(*) as cnt FROM memories GROUP BY event_type ORDER BY cnt DESC")
                .context("failed to prepare type_stats query")?;

            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .context("failed to query type stats")?;

            let mut result = serde_json::Map::new();
            let mut total = 0i64;
            for row in rows {
                let (etype, cnt) = row.context("failed to decode type stat row")?;
                total += cnt;
                result.insert(etype, serde_json::json!(cnt));
            }
            result.insert("_total".to_string(), serde_json::json!(total));

            Ok::<_, anyhow::Error>(serde_json::Value::Object(result))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn session_stats(&self) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let mut stmt = conn
                .prepare("SELECT session_id, COUNT(*) as cnt FROM memories WHERE session_id IS NOT NULL GROUP BY session_id ORDER BY cnt DESC LIMIT 20")
                .context("failed to prepare session_stats query")?;

            let rows = stmt
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "session_id": row.get::<_, String>(0)?,
                        "count": row.get::<_, i64>(1)?,
                    }))
                })
                .context("failed to query session stats")?;

            let results: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();

            let total_sessions: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT session_id) FROM memories WHERE session_id IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or_else(|e| {
                    tracing::warn!("failed to count total sessions: {e}");
                    results.len() as i64
                });

            Ok::<_, anyhow::Error>(serde_json::json!({
                "sessions": results,
                "total_sessions": total_sessions,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn weekly_digest(&self, days: i64) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .unwrap_or(0);

            if days <= 0 {
                anyhow::bail!("days must be > 0");
            }
            let prev_days = days
                .checked_mul(2)
                .ok_or_else(|| anyhow::anyhow!("days value is too large"))?;
            let days_str = format!("-{days} days");
            let prev_days_str = format!("-{prev_days} days");

            let period_new: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE datetime(created_at) >= datetime('now', ?1)",
                    params![days_str],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let session_count: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT session_id) FROM memories WHERE datetime(created_at) >= datetime('now', ?1) AND session_id IS NOT NULL",
                    params![days_str],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // Type breakdown in period
            let mut stmt = conn
                .prepare("SELECT COALESCE(event_type, 'untyped'), COUNT(*) FROM memories WHERE datetime(created_at) >= datetime('now', ?1) GROUP BY event_type ORDER BY COUNT(*) DESC")
                .context("failed to prepare digest type breakdown")?;

            let breakdown_rows = stmt
                .query_map(params![days_str], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .context("failed to query type breakdown")?;

            let mut type_breakdown = serde_json::Map::new();
            for (etype, cnt) in breakdown_rows.flatten() {
                type_breakdown.insert(etype, serde_json::json!(cnt));
            }

            // Previous period count for growth calc
            let prev_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE datetime(created_at) >= datetime('now', ?1) AND datetime(created_at) < datetime('now', ?2)",
                    params![prev_days_str, days_str],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            #[allow(clippy::cast_precision_loss)]
            let growth_pct = if prev_count > 0 {
                ((period_new - prev_count) as f64 / prev_count as f64) * 100.0
            } else if period_new > 0 {
                100.0
            } else {
                0.0
            };

            Ok::<_, anyhow::Error>(serde_json::json!({
                "period_days": days,
                "total_memories": total,
                "period_new": period_new,
                "session_count": session_count,
                "type_breakdown": serde_json::Value::Object(type_breakdown),
                "growth_pct": (growth_pct * 100.0).round() / 100.0,
                "prev_period_count": prev_count,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn access_rate_stats(&self) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .unwrap_or(0);

            let zero_access: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE access_count = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let avg_access: f64 = conn
                .query_row(
                    "SELECT COALESCE(AVG(access_count), 0.0) FROM memories",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            // By type breakdown
            let mut stmt = conn
                .prepare("SELECT COALESCE(event_type, 'untyped') as etype, COUNT(*) as cnt, AVG(access_count) as avg_ac, SUM(CASE WHEN access_count = 0 THEN 1 ELSE 0 END) as zero_cnt FROM memories GROUP BY event_type ORDER BY avg_ac DESC")
                .context("failed to prepare access rate by-type query")?;

            let by_type_rows = stmt
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "event_type": row.get::<_, String>(0)?,
                        "count": row.get::<_, i64>(1)?,
                        "avg_access_count": row.get::<_, f64>(2)?,
                        "zero_access_count": row.get::<_, i64>(3)?,
                    }))
                })
                .context("failed to query access rate by type")?;

            let by_type: Vec<serde_json::Value> = by_type_rows.filter_map(|r| r.ok()).collect();

            // Top 10 most accessed
            let mut stmt2 = conn
                .prepare("SELECT id, content, access_count, event_type FROM memories WHERE access_count > 0 ORDER BY access_count DESC LIMIT 10")
                .context("failed to prepare top accessed query")?;

            let top_rows = stmt2
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "content": row.get::<_, String>(1)?.chars().take(100).collect::<String>(),
                        "access_count": row.get::<_, i64>(2)?,
                        "event_type": row.get::<_, Option<String>>(3)?,
                    }))
                })
                .context("failed to query top accessed")?;

            let top_accessed: Vec<serde_json::Value> = top_rows.filter_map(|r| r.ok()).collect();

            #[allow(clippy::cast_precision_loss)]
            let never_pct = if total > 0 {
                (zero_access as f64 / total as f64) * 100.0
            } else {
                0.0
            };

            Ok::<_, anyhow::Error>(serde_json::json!({
                "total_memories": total,
                "zero_access_count": zero_access,
                "never_accessed_pct": (never_pct * 100.0).round() / 100.0,
                "avg_access_count": (avg_access * 100.0).round() / 100.0,
                "by_type": by_type,
                "top_accessed": top_accessed,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }
}
