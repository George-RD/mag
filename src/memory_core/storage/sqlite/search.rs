use super::*;

#[async_trait]
impl Searcher for SqliteStorage {
    async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let query = query.to_string();
        let effective_limit = i64::try_from(limit).context("search limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let conn = lock_conn(&conn)?;

            let fts_query = build_fts5_query(&query);
            let include_superseded = opts.include_superseded.unwrap_or(false);
            let mut fts_sql = String::from(
                "SELECT f.id, m.content, m.tags, m.importance, m.metadata, m.event_type, m.session_id, m.project
                 FROM memories_fts f
                 JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1",
            );
            if !include_superseded {
                fts_sql.push_str(" AND m.superseded_by_id IS NULL");
            }
            let mut fts_params: Vec<SqlValue> = vec![SqlValue::Text(fts_query)];
            let mut param_idx = 2;
            if let Some(event_type) = opts.event_type.clone() {
                fts_sql.push_str(&format!(" AND m.event_type = ?{param_idx}"));
                fts_params.push(SqlValue::Text(event_type));
                param_idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                fts_sql.push_str(&format!(" AND m.project = ?{param_idx}"));
                fts_params.push(SqlValue::Text(project));
                param_idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                fts_sql.push_str(&format!(" AND m.session_id = ?{param_idx}"));
                fts_params.push(SqlValue::Text(session_id));
                param_idx += 1;
            }
            if let Some(entity_id) = opts.entity_id.clone() {
                fts_sql.push_str(&format!(" AND m.entity_id = ?{param_idx}"));
                fts_params.push(SqlValue::Text(entity_id));
                param_idx += 1;
            }
            if let Some(agent_type) = opts.agent_type.clone() {
                fts_sql.push_str(&format!(" AND m.agent_type = ?{param_idx}"));
                fts_params.push(SqlValue::Text(agent_type));
                param_idx += 1;
            }
            fts_sql.push_str(" ORDER BY bm25(memories_fts)");
            fts_sql.push_str(&format!(" LIMIT ?{param_idx}"));
            fts_params.push(SqlValue::Integer(effective_limit));

            let fts_result = conn.prepare(&fts_sql);

            if let Ok(mut stmt) = fts_result {
                let mut fts_param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
                for value in &fts_params {
                    fts_param_refs.push(value);
                }
                let rows = stmt
                    .query_map(fts_param_refs.as_slice(), search_result_from_row);

                if let Err(ref e) = rows {
                    tracing::warn!("FTS5 search failed, falling back to LIKE: {e}");
                }
                if let Ok(rows) = rows {
                    let mut results = Vec::new();
                    for row in rows {
                        results.push(row.context("failed to decode FTS5 search row")?);
                    }

                    if !results.is_empty() {
                        return Ok(results);
                    }
                }
            }

            let pattern = escape_like_pattern(&query);

            let mut sql = String::from(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project
                 FROM memories
                 WHERE lower(content) LIKE ?1 ESCAPE '\\'",
            );
            if !include_superseded {
                sql.push_str(" AND superseded_by_id IS NULL");
            }
            let mut params_values: Vec<SqlValue> = vec![SqlValue::Text(pattern)];
            let mut idx = 2;
            if let Some(event_type) = opts.event_type.clone() {
                sql.push_str(&format!(" AND event_type = ?{idx}"));
                params_values.push(SqlValue::Text(event_type));
                idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                sql.push_str(&format!(" AND project = ?{idx}"));
                params_values.push(SqlValue::Text(project));
                idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                sql.push_str(&format!(" AND session_id = ?{idx}"));
                params_values.push(SqlValue::Text(session_id));
                idx += 1;
            }
            if let Some(entity_id) = opts.entity_id.clone() {
                sql.push_str(&format!(" AND entity_id = ?{idx}"));
                params_values.push(SqlValue::Text(entity_id));
                idx += 1;
            }
            if let Some(agent_type) = opts.agent_type.clone() {
                sql.push_str(&format!(" AND agent_type = ?{idx}"));
                params_values.push(SqlValue::Text(agent_type));
                idx += 1;
            }
            sql.push_str(" ORDER BY last_accessed_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(SqlValue::Integer(effective_limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare LIKE search query")?;

            let mut like_param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                like_param_refs.push(value);
            }

            let rows = stmt
                .query_map(like_param_refs.as_slice(), search_result_from_row)
                .context("failed to execute LIKE search query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode LIKE search row")?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl Recents for SqliteStorage {
    async fn recent(&self, limit: usize, opts: &SearchOptions) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let effective_limit = i64::try_from(limit).context("recent limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;
            let include_superseded = opts.include_superseded.unwrap_or(false);

            let conn = lock_conn(&conn)?;

            let mut sql = String::from(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project
                 FROM memories
                 WHERE 1 = 1",
            );
            let mut params_values: Vec<SqlValue> = Vec::new();
            let mut idx = 1;
            if let Some(event_type) = opts.event_type.clone() {
                sql.push_str(&format!(" AND event_type = ?{idx}"));
                params_values.push(SqlValue::Text(event_type));
                idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                sql.push_str(&format!(" AND project = ?{idx}"));
                params_values.push(SqlValue::Text(project));
                idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                sql.push_str(&format!(" AND session_id = ?{idx}"));
                params_values.push(SqlValue::Text(session_id));
                idx += 1;
            }
            if !include_superseded {
                sql.push_str(" AND superseded_by_id IS NULL");
            }
            sql.push_str(" ORDER BY last_accessed_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(SqlValue::Integer(effective_limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare recent query")?;

            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                param_refs.push(value);
            }

            let rows = stmt
                .query_map(param_refs.as_slice(), search_result_from_row)
                .context("failed to execute recent query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode recent row")?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl SemanticSearcher for SqliteStorage {
    async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let embedder = Arc::clone(&self.embedder);
        let query = query.to_string();
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let include_superseded = opts.include_superseded.unwrap_or(false);
            let query_embedding = embedder
                .embed(&query)
                .context("failed to compute query embedding")?;

            let conn = lock_conn(&conn)?;

            let mut sql = String::from(
                "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project
                 FROM memories
                 WHERE embedding IS NOT NULL",
            );
            if !include_superseded {
                sql.push_str(" AND superseded_by_id IS NULL");
            }
            let mut params_values: Vec<SqlValue> = Vec::new();
            let mut idx = 1;
            if let Some(event_type) = opts.event_type.clone() {
                sql.push_str(&format!(" AND event_type = ?{idx}"));
                params_values.push(SqlValue::Text(event_type));
                idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                sql.push_str(&format!(" AND project = ?{idx}"));
                params_values.push(SqlValue::Text(project));
                idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                sql.push_str(&format!(" AND session_id = ?{idx}"));
                params_values.push(SqlValue::Text(session_id));
            }

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare semantic search query")?;

            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                param_refs.push(value);
            }

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    let id: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    let embedding_blob: Vec<u8> = row.get(2)?;
                    let tags: String = row.get(3)?;
                    let importance: f64 = row.get(4)?;
                    let metadata: String = row.get(5)?;
                    let event_type: Option<String> = row.get(6).ok();
                    let session_id: Option<String> = row.get(7).ok();
                    let project: Option<String> = row.get(8).ok();
                    Ok((
                        id,
                        content,
                        embedding_blob,
                        tags,
                        importance,
                        metadata,
                        event_type,
                        session_id,
                        project,
                    ))
                })
                .context("failed to execute semantic search query")?;

            let mut ranked = Vec::new();
            for row in rows {
                let (
                    id,
                    content,
                    embedding_blob,
                    raw_tags,
                    importance,
                    raw_metadata,
                    event_type,
                    session_id,
                    project,
                ) = row.context("failed to decode semantic search row")?;
                let candidate: Vec<f32> = serde_json::from_slice(&embedding_blob)
                    .context("failed to decode stored embedding")?;
                let score = cosine_similarity(&query_embedding, &candidate);
                ranked.push(SemanticResult {
                    id,
                    content,
                    tags: parse_tags_from_db(&raw_tags),
                    importance,
                    metadata: parse_metadata_from_db(&raw_metadata),
                    event_type,
                    session_id,
                    project,
                    score,
                });
            }

            ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
            ranked.truncate(limit);

            Ok::<_, anyhow::Error>(ranked)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl PhraseSearcher for SqliteStorage {
    async fn phrase_search(
        &self,
        phrase: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let phrase = phrase.to_string();
        let limit = i64::try_from(limit).context("phrase search limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let include_superseded = opts.include_superseded.unwrap_or(false);
            let conn = lock_conn(&conn)?;

            let pattern = escape_like_pattern(&phrase);

            let mut sql = String::from(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project
                 FROM memories
                 WHERE lower(content) LIKE ?1 ESCAPE '\\'",
            );
            if !include_superseded {
                sql.push_str(" AND superseded_by_id IS NULL");
            }
            let mut params_values: Vec<SqlValue> = vec![SqlValue::Text(pattern)];
            let mut idx = 2;
            if let Some(event_type) = opts.event_type.clone() {
                sql.push_str(&format!(" AND event_type = ?{idx}"));
                params_values.push(SqlValue::Text(event_type));
                idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                sql.push_str(&format!(" AND project = ?{idx}"));
                params_values.push(SqlValue::Text(project));
                idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                sql.push_str(&format!(" AND session_id = ?{idx}"));
                params_values.push(SqlValue::Text(session_id));
                idx += 1;
            }
            if let Some(importance_min) = opts.importance_min {
                sql.push_str(&format!(" AND importance >= ?{idx}"));
                params_values.push(SqlValue::Real(importance_min));
                idx += 1;
            }
            if let Some(created_after) = opts.created_after.clone() {
                sql.push_str(&format!(" AND created_at >= ?{idx}"));
                params_values.push(SqlValue::Text(created_after));
                idx += 1;
            }
            if let Some(created_before) = opts.created_before.clone() {
                sql.push_str(&format!(" AND created_at <= ?{idx}"));
                params_values.push(SqlValue::Text(created_before));
                idx += 1;
            }
            sql.push_str(" ORDER BY created_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(SqlValue::Integer(limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare phrase search query")?;
            let mut refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                refs.push(value);
            }

            let rows = stmt
                .query_map(refs.as_slice(), search_result_from_row)
                .context("failed to execute phrase search query")?;

            let mut out = Vec::new();
            for row in rows {
                let result = row.context("failed to decode phrase search row")?;
                if let Some(context_tags) = opts.context_tags.as_ref()
                    && !context_tags.is_empty()
                    && !context_tags
                        .iter()
                        .all(|tag| result.tags.iter().any(|r| r.eq_ignore_ascii_case(tag)))
                {
                    continue;
                }
                out.push(result);
            }

            Ok::<_, anyhow::Error>(out)
        })
        .await
        .context("spawn_blocking join error")?
    }
}
