"""Temporary, readable PR439 TDD transformations; removed before merge."""
from pathlib import Path
import subprocess
import sys

ROOT = Path.cwd()

def replace(path, old, new, count=1):
    p = ROOT / path
    text = p.read_text()
    actual = text.count(old)
    if actual != count:
        raise RuntimeError(f'{path}: expected {count} occurrences, found {actual}: {old[:90]!r}')
    p.write_text(text.replace(old, new))

def append(path, text):
    p = ROOT / path
    p.write_text(p.read_text() + text)

hot = 'src/memory_core/storage/sqlite/hot_cache_mgmt.rs'
advanced = 'src/memory_core/storage/sqlite/advanced_tests.rs'
race = 'tests/reembed_stale_runtime.rs'

if sys.argv[1] == 'red':
    # Isolate the existing refresh operation at its exact snapshot boundary.
    replace(hot,
        'fn refresh_generation_bound_hot_cache(pool: &ConnPool, hot_cache: &HotTierCache) -> Result<()> {\n',
        '''fn refresh_generation_bound_hot_cache(pool: &ConnPool, hot_cache: &HotTierCache) -> Result<()> {
    refresh_generation_bound_hot_cache_with(pool, hot_cache, |conn| hot_cache.refresh(conn))
}

fn refresh_generation_bound_hot_cache_with(
    pool: &ConnPool,
    hot_cache: &HotTierCache,
    refresh: impl FnOnce(&rusqlite::Connection) -> Result<()>,
) -> Result<()> {
''')
    replace(hot, '        hot_cache.refresh(&conn)\n', '        refresh(&conn)\n')
    append(hot, '''
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::schema;
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
''')
    # These fixture checks fail against canonical dedup before changing data.
    replace(advanced, '    let conn = storage.test_conn().unwrap();\n', '''    let conn = storage.test_conn().unwrap();
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0)).unwrap();
    assert_eq!(count, (super::pipeline::ADVANCED_FTS_CANDIDATE_MIN + 21) as i64,
        "FTS bound fixture must exceed the candidate limit after canonical dedup");
''', count=2)
    # The first reader may retain its old view, but the second must reject it.
    replace('src/memory_core/storage/sqlite/conn_pool.rs',
        '        drop(snapshot);\n        assert!(\n            pool.embedding_snapshot(&reader).is_err(),',
        '''        let other_reader = pool.reader()?;
        assert!(
            pool.embedding_snapshot(&other_reader).is_err(),
            "parallel candidate snapshots must reject a different generation"
        );
        drop(other_reader);
        drop(snapshot);
        assert!(
            pool.embedding_snapshot(&reader).is_err(),''')

elif sys.argv[1] == 'green':
    replace(hot, '''        let conn = pool.reader()?;
        let conn = pool.embedding_snapshot(&conn)?;
        refresh(&conn)
''', '''        {
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
''')
    # Unique short documents outrank the longer desired document in BM25.
    replace(advanced, '        let id = format!("old-{idx}");\n', '''        let id = format!("old-{idx}");
        let content = format!("alpha {idx}");
''')
    replace(advanced, '        let id = format!("old-event-{idx}");\n', '''        let id = format!("old-event-{idx}");
        let content = format!("alpha {idx}");
''')
    replace(advanced, '''            &id,
            "alpha",
            &MemoryInput {
                content: "alpha".to_string(),
''', '''            &id,
            &content,
            &MemoryInput {
                content: content.clone(),
''', count=2)
    replace(advanced, '    let candidates = collect_fts_candidates(\n', '''    assert_unfiltered_limit_excludes(&conn, &storage, "recent-match");
    let candidates = collect_fts_candidates(
''')
    replace(advanced, '    let recent_candidates = collect_fts_candidates(\n', '''    assert_unfiltered_limit_excludes(&conn, &storage, "recent-event-match");
    let recent_candidates = collect_fts_candidates(
''')
    append(advanced, '''
fn assert_unfiltered_limit_excludes(
    conn: &rusqlite::Connection,
    storage: &SqliteStorage,
    excluded_id: &str,
) {
    let unfiltered = collect_fts_candidates(
        conn, "alpha", 1, &SearchOptions::default(), true, &storage.scoring_params,
    ).unwrap();
    assert_eq!(unfiltered.len(), super::pipeline::ADVANCED_FTS_CANDIDATE_MIN);
    assert!(!unfiltered.iter().any(|(id, _, _)| id == excluded_id),
        "fixture must put the filtered match outside the unfiltered candidate limit");
}

#[derive(Debug, Default)]
struct QueryCountingModel(std::sync::atomic::AtomicUsize);

impl crate::memory_core::EmbeddingModel for QueryCountingModel {
    fn dimension(&self) -> usize { 4 }
    fn embedding_space_identity(&self) -> &str { "test-query-routing" }
    fn embed_for(
        &self,
        kind: crate::memory_core::EmbeddingInputKind,
        _text: &str,
    ) -> anyhow::Result<Vec<f32>> {
        if kind == crate::memory_core::EmbeddingInputKind::Query {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(vec![1.0, 0.0, 0.0, 0.0])
    }
}

fn routing_storage() -> (SqliteStorage, std::sync::Arc<QueryCountingModel>) {
    let model = std::sync::Arc::new(QueryCountingModel::default());
    let storage = SqliteStorage::new_in_memory_with_embedding_model(model.clone()).unwrap();
    (storage, model)
}
''')
    # Instrument the existing routing tests rather than creating duplicates.
    for name in ['keyword_dispatch_returns_fts_results', 'non_keyword_query_uses_full_pipeline', 'blank_query_routes_to_fts_only']:
        replace(advanced, f'''async fn {name}() {{
    use crate::memory_core::AdvancedSearcher;

    let storage = SqliteStorage::new_in_memory().unwrap();''',
            f'''async fn {name}() {{
    use crate::memory_core::AdvancedSearcher;

    let (storage, model) = routing_storage();''')
    replace(advanced, '    assert!(!results.is_empty(), "keyword query should return results");', '''    assert_eq!(model.0.load(std::sync::atomic::Ordering::SeqCst), 0,
        "keyword dispatch must skip query embeddings");
    assert!(!results.is_empty(), "keyword query should return results");''')
    replace(advanced, '    // Should still return results through the full pipeline.', '''    assert!(model.0.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "natural-language dispatch must compute query embeddings");
    // Should still return results through the full pipeline.''')
    replace(advanced, '        // FTS5 with an empty query matches nothing, so we expect an empty', '''        assert_eq!(model.0.load(std::sync::atomic::Ordering::SeqCst), 0,
            "blank dispatch must skip query embeddings");
        // FTS5 with an empty query matches nothing, so we expect an empty''')
    # Release the read gate before propagating a migration error or timeout.
    replace(race, '''        let migration = migrate(&path, "space-b", 4).await;
        resume_tx.send(())?;
        migration?;
        let error = read
            .await?''', '''        let migration = tokio::time::timeout(
            Duration::from_secs(20), migrate(&path, "space-b", 4),
        ).await;
        resume_tx.send(())?;
        migration??;
        let error = tokio::time::timeout(Duration::from_secs(30), read)
            .await??''')
    replace(race, '''    let migration = async {
        migrate(&path, "space-b", 4).await?;
        migrate(&path, "space-a", 4).await
    }
    .await;
    resume_tx.send(())?;
    migration?;
    let error = read
        .await?''', '''    let migration = tokio::time::timeout(Duration::from_secs(20), async {
        migrate(&path, "space-b", 4).await?;
        migrate(&path, "space-a", 4).await
    })
    .await;
    resume_tx.send(())?;
    migration??;
    let error = tokio::time::timeout(Duration::from_secs(30), read)
        .await??''')
else:
    raise SystemExit('usage: pr439-review.py red|green')

evidence = Path('/tmp/pr439-review')
evidence.mkdir(parents=True, exist_ok=True)
(evidence / f'{sys.argv[1]}.patch').write_bytes(subprocess.check_output(['git', 'diff', '--binary', '--', 'src', 'tests']))
