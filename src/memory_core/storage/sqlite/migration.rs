use crate::memory_core::{EmbeddingInputKind, EmbeddingModel};

use super::*;

/// Options for migrating persisted memories into a new embedding space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReembedOptions {
    /// Maximum number of memory contents embedded in one model call.
    pub batch_size: usize,
    /// Report the affected rows without changing the database.
    pub dry_run: bool,
}

impl Default for ReembedOptions {
    fn default() -> Self {
        Self {
            batch_size: 100,
            dry_run: false,
        }
    }
}

/// Result of an embedding-space migration or dry-run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReembedReport {
    pub source_embedding_space: String,
    pub target_embedding_space: String,
    pub target_dimension: usize,
    pub memory_count: usize,
    pub migrated_count: usize,
    pub backup_path: Option<PathBuf>,
}

impl SqliteStorage {
    /// Migrates one file-backed database into `embedding_model` without first
    /// opening it through the normal compatibility guard.
    ///
    /// The migration owns an offline maintenance connection. Persisted vectors,
    /// the optional sqlite-vec index, and the embedding-space identity change in
    /// one transaction; the identity is written last. A failed embedding batch
    /// therefore rolls the database back to the original space. The command
    /// creates a rollback backup before any live data is changed.
    pub async fn reembed_path_with_embedding_model(
        path: PathBuf,
        embedding_model: Arc<dyn EmbeddingModel>,
        options: ReembedOptions,
    ) -> Result<ReembedReport> {
        tokio::task::spawn_blocking(move || {
            reembed_path_sync(&path, embedding_model.as_ref(), options)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

fn reembed_path_sync(
    path: &Path,
    embedding_model: &dyn EmbeddingModel,
    options: ReembedOptions,
) -> Result<ReembedReport> {
    if options.batch_size == 0 {
        return Err(anyhow!("re-embed batch size must be greater than zero"));
    }
    if embedding_model.dimension() == 0 {
        return Err(anyhow!("target embedding dimension must be greater than zero"));
    }
    if path.as_os_str() == ":memory:" {
        return Err(anyhow!("re-embed requires a file-backed database"));
    }

    #[cfg(feature = "sqlite-vec")]
    super::ensure_vec_extension_registered();

    let mut conn = Connection::open(path)
        .with_context(|| format!("failed to open sqlite database at {}", path.display()))?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON;\
         PRAGMA journal_mode=WAL;\
         PRAGMA busy_timeout=5000;",
    )
    .context("failed to configure re-embed sqlite connection")?;

    let source_embedding_space: Option<String> = conn
        .query_row(
            "SELECT value FROM runtime_metadata WHERE key = 'embedding_space_identity'",
            [],
            |row| row.get(0),
        )
        .optional()
        .context("failed to read persisted embedding-space identity")?;
    let source_embedding_space = source_embedding_space.ok_or_else(|| {
        anyhow!(
            "database has no persisted embedding-space identity; open it with the current profile before migrating"
        )
    })?;
    let target_embedding_space = embedding_model.embedding_space_identity().to_string();

    if source_embedding_space == target_embedding_space {
        return Ok(ReembedReport {
            source_embedding_space,
            target_embedding_space,
            target_dimension: embedding_model.dimension(),
            memory_count: 0,
            migrated_count: 0,
            backup_path: None,
        });
    }

    let memory_count_i64: i64 = conn
        .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
        .context("failed to count memories for re-embed")?;
    let memory_count = usize::try_from(memory_count_i64)
        .context("memory count does not fit into usize for re-embed")?;

    if options.dry_run {
        return Ok(ReembedReport {
            source_embedding_space,
            target_embedding_space,
            target_dimension: embedding_model.dimension(),
            memory_count,
            migrated_count: 0,
            backup_path: None,
        });
    }

    let backup = super::admin::backup::create_backup_sync(&conn, path)
        .context("failed to create pre-migration backup")?;
    let batch_limit = i64::try_from(options.batch_size)
        .context("re-embed batch size does not fit into SQLite integer")?;
    let target_dimension = embedding_model.dimension();

    let tx = retry_on_lock(|| conn.unchecked_transaction())
        .context("failed to begin re-embed transaction")?;

    #[cfg(feature = "sqlite-vec")]
    recreate_vec_table(&tx, target_dimension)?;

    let mut last_id: Option<String> = None;
    let mut migrated_count = 0_usize;

    loop {
        let rows: Vec<(String, String)> = {
            let mut stmt = tx
                .prepare(
                    "SELECT id, content FROM memories\
                     WHERE (?1 IS NULL OR id > ?1)\
                     ORDER BY id\
                     LIMIT ?2",
                )
                .context("failed to prepare re-embed batch query")?;
            let mapped = stmt
                .query_map(params![last_id.as_deref(), batch_limit], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .context("failed to query re-embed batch")?;
            mapped
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("failed to read re-embed batch")?
        };

        if rows.is_empty() {
            break;
        }

        let texts: Vec<&str> = rows.iter().map(|(_, content)| content.as_str()).collect();
        let embeddings = embedding_model
            .embed_batch_for(EmbeddingInputKind::Document, &texts)
            .context("failed to embed re-embed batch")?;
        if embeddings.len() != rows.len() {
            return Err(anyhow!(
                "embedding model returned {} vectors for {} re-embed inputs",
                embeddings.len(),
                rows.len()
            ));
        }

        for ((id, _), embedding) in rows.iter().zip(embeddings) {
            if embedding.len() != target_dimension {
                return Err(anyhow!(
                    "embedding model returned dimension {} for memory {id}; expected {target_dimension}",
                    embedding.len()
                ));
            }
            let blob = encode_embedding(&embedding);
            tx.execute(
                "UPDATE memories SET embedding = ?2 WHERE id = ?1",
                params![id, &blob],
            )
            .with_context(|| format!("failed to update embedding for memory {id}"))?;

            #[cfg(feature = "sqlite-vec")]
            tx.execute(
                "INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)",
                params![id, &blob],
            )
            .with_context(|| format!("failed to index migrated embedding for memory {id}"))?;
        }

        migrated_count += rows.len();
        last_id = rows.last().map(|(id, _)| id.clone());
        tracing::info!(
            completed = migrated_count,
            total = memory_count,
            "re-embedding migration progress"
        );
    }

    let identity_rows = tx
        .execute(
            "UPDATE runtime_metadata SET value = ?1 WHERE key = 'embedding_space_identity'",
            params![target_embedding_space],
        )
        .context("failed to persist migrated embedding-space identity")?;
    if identity_rows != 1 {
        return Err(anyhow!(
            "expected exactly one embedding-space identity row, updated {identity_rows}"
        ));
    }

    tx.commit().context("failed to commit re-embed transaction")?;

    Ok(ReembedReport {
        source_embedding_space,
        target_embedding_space,
        target_dimension,
        memory_count,
        migrated_count,
        backup_path: Some(backup.path),
    })
}

#[cfg(feature = "sqlite-vec")]
fn recreate_vec_table(conn: &Connection, embedding_dim: usize) -> Result<()> {
    conn.execute_batch("DROP TABLE IF EXISTS vec_memories;")
        .context("failed to drop old vec_memories during re-embed")?;
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE vec_memories USING vec0(\
            memory_id text primary key,\
            embedding float[{embedding_dim}] distance_metric=cosine\
        );"
    ))
    .context("failed to create target vec_memories during re-embed")?;
    Ok(())
}
