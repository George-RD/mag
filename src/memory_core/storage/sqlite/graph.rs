use super::*;

#[async_trait]
impl GraphTraverser for SqliteStorage {
    async fn traverse(
        &self,
        start_id: &str,
        max_hops: usize,
        min_weight: f64,
        edge_types: Option<&[String]>,
    ) -> Result<Vec<GraphNode>> {
        let pool = Arc::clone(&self.pool);
        let start_id = start_id.to_string();
        let hop_limit = max_hops.clamp(1, 5);
        if max_hops > 5 {
            tracing::warn!(requested = max_hops, capped = 5, "max_hops capped to 5");
        }
        let edge_types = edge_types.map(|edges| edges.to_vec());

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let conn = pool.reader()?;

            let mut frontier = vec![start_id.clone()];
            let mut visited: HashSet<String> = HashSet::from([start_id.clone()]);
            let mut nodes = Vec::new();

            for hop in 1..=hop_limit {
                if frontier.is_empty() {
                    break;
                }
                let mut next_frontier = Vec::new();

                for current in &frontier {
                    let mut sql = String::from(
                        "SELECT source_id, target_id, rel_type, weight
                         FROM relationships
                         WHERE (source_id = ?1 OR target_id = ?1)
                           AND weight >= ?2",
                    );
                    let mut params_values: Vec<SqlValue> = vec![
                        SqlValue::Text(current.clone()),
                        SqlValue::Real(min_weight),
                    ];
                    if let Some(types) = edge_types.as_ref()
                        && !types.is_empty()
                    {
                        sql.push_str(" AND rel_type IN (");
                        for idx in 0..types.len() {
                            if idx > 0 {
                                sql.push_str(", ");
                            }
                            sql.push_str(&format!("?{}", idx + 3));
                        }
                        sql.push(')');
                        for rel_type in types {
                            params_values.push(SqlValue::Text(rel_type.clone()));
                        }
                    }
                    sql.push_str(" ORDER BY weight DESC");

                    let mut stmt = conn
                        .prepare(&sql)
                        .context("failed to prepare traversal query")?;
                    let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
                    for value in &params_values {
                        param_refs.push(value);
                    }

                    let edges = stmt
                        .query_map(param_refs.as_slice(), |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, f64>(3).unwrap_or(1.0),
                            ))
                        })
                        .context("failed to execute traversal query")?;

                    for edge in edges {
                        let (source_id, target_id, rel_type, weight) =
                            edge.context("failed to decode traversal edge")?;
                        let neighbor = if source_id == *current {
                            target_id
                        } else {
                            source_id
                        };

                        if neighbor == start_id || !visited.insert(neighbor.clone()) {
                            continue;
                        }

                        let memory: Option<(String, Option<String>, String, String)> = conn
                            .query_row(
                                "SELECT content, event_type, metadata, created_at FROM memories WHERE id = ?1",
                                params![neighbor],
                                |row| {
                                    Ok((
                                        row.get::<_, String>(0)?,
                                        row.get::<_, Option<String>>(1).ok().flatten(),
                                        row.get::<_, String>(2)
                                            .unwrap_or_else(|_| "{}".to_string()),
                                        row.get::<_, String>(3)
                                            .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                                    ))
                                },
                            )
                            .optional()
                            .context("failed to fetch neighbor memory")?;

                        if let Some((content, event_type_str, metadata_raw, created_at)) = memory {
                            next_frontier.push(neighbor.clone());
                            nodes.push(GraphNode {
                                id: neighbor,
                                content,
                                event_type: event_type_from_sql(event_type_str),
                                metadata: parse_metadata_from_db(&metadata_raw),
                                hop,
                                weight,
                                edge_type: rel_type,
                                created_at,
                            });
                        }
                    }
                }

                frontier = next_frontier;
            }

            nodes.sort_by(|a, b| a.hop.cmp(&b.hop).then_with(|| b.weight.total_cmp(&a.weight)));
            Ok::<_, anyhow::Error>(nodes)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl SimilarFinder for SqliteStorage {
    async fn find_similar(&self, memory_id: &str, limit: usize) -> Result<Vec<SemanticResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let pool = Arc::clone(&self.pool);
        let memory_id = memory_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let source_embedding: Vec<u8> = conn
                .query_row(
                    "SELECT embedding FROM memories WHERE id = ?1",
                    params![memory_id],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query source embedding")?
                .ok_or_else(|| anyhow!("memory not found for id={memory_id}"))?;
            let source_embedding: Vec<f32> = decode_embedding(&source_embedding)
                .context("failed to decode source embedding")?;

            let mut ranked = Vec::new();

            #[cfg(feature = "sqlite-vec")]
            {
                let knn_limit = limit.saturating_mul(5).clamp(50, 5_000);
                let knn_results = vec_knn_search(&conn, &source_embedding, knn_limit)?;
                let ordered_ids: Vec<String> = knn_results
                    .iter()
                    .filter(|(candidate_id, _)| candidate_id != &memory_id)
                    .map(|(candidate_id, _)| candidate_id.clone())
                    .collect();
                let mut hydrated_rows =
                    hydrate_memories_by_ids(&conn, &ordered_ids, false, None, false)?;

                for (candidate_id, distance) in knn_results {
                    if candidate_id == memory_id {
                        continue;
                    }
                    if ranked.len() >= limit {
                        break;
                    }
                    let similarity = vec_distance_to_similarity(distance) as f32;
                    if let Some(row_data) = hydrated_rows.remove(&candidate_id) {
                        ranked.push(SemanticResult {
                            id: candidate_id,
                            content: row_data.content,
                            tags: row_data.tags,
                            importance: row_data.importance,
                            metadata: row_data.metadata,
                            event_type: row_data.event_type,
                            session_id: row_data.session_id,
                            project: row_data.project,
                            entity_id: row_data.entity_id,
                            agent_type: row_data.agent_type,
                            score: similarity,
                        });
                    }
                }
            }

            #[cfg(not(feature = "sqlite-vec"))]
            {
                let mut stmt = conn
                    .prepare(
                        "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project, entity_id, agent_type
                         FROM memories WHERE embedding IS NOT NULL AND id != ?1 AND superseded_by_id IS NULL",
                    )
                    .context("failed to prepare similar query")?;
                let rows = stmt
                    .query_map(params![memory_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, f64>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, Option<String>>(6).ok().flatten(),
                            row.get::<_, Option<String>>(7).ok().flatten(),
                            row.get::<_, Option<String>>(8).ok().flatten(),
                            row.get::<_, Option<String>>(9).ok().flatten(),
                            row.get::<_, Option<String>>(10).ok().flatten(),
                        ))
                    })
                    .context("failed to execute similar query")?;

                for row in rows {
                    let (
                        id,
                        content,
                        embedding_blob,
                        raw_tags,
                        importance,
                        raw_metadata,
                        event_type_str,
                        session_id,
                        project,
                        entity_id,
                        agent_type,
                    ) = row.context("failed to decode similar row")?;
                    let embedding: Vec<f32> = decode_embedding(&embedding_blob)
                        .context("failed to decode candidate embedding")?;
                    let score = cosine_similarity(&source_embedding, &embedding);
                    ranked.push(SemanticResult {
                        id,
                        content,
                        tags: parse_tags_from_db(&raw_tags),
                        importance,
                        metadata: parse_metadata_from_db(&raw_metadata),
                        event_type: event_type_from_sql(event_type_str),
                        session_id,
                        project,
                        entity_id,
                        agent_type,
                        score,
                    });
                }

                ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
                ranked.truncate(limit);
            }
            Ok::<_, anyhow::Error>(ranked)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl RelationshipQuerier for SqliteStorage {
    async fn get_relationships(&self, memory_id: &str) -> Result<Vec<Relationship>> {
        let pool = Arc::clone(&self.pool);
        let memory_id = memory_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let mut stmt = conn
                .prepare(
                    "SELECT id, source_id, target_id, rel_type, weight, metadata, created_at
                     FROM relationships
                     WHERE source_id = ?1 OR target_id = ?1",
                )
                .context("failed to prepare relationships query")?;

            let rows = stmt
                .query_map(params![memory_id], |row| {
                    Ok(Relationship {
                        id: row.get(0)?,
                        source_id: row.get(1)?,
                        target_id: row.get(2)?,
                        rel_type: row.get(3)?,
                        weight: row.get::<_, f64>(4).unwrap_or(1.0),
                        metadata: parse_metadata_from_db(
                            &row.get::<_, String>(5).unwrap_or_else(|_| "{}".to_string()),
                        ),
                        created_at: row.get::<_, String>(6).unwrap_or_else(|_| "".to_string()),
                    })
                })
                .context("failed to execute relationships query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode relationship row")?);
            }
            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl VersionChainQuerier for SqliteStorage {
    async fn get_version_chain(&self, memory_id: &str) -> Result<Vec<SearchResult>> {
        let pool = Arc::clone(&self.pool);
        let memory_id = memory_id.to_string();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let conn = pool.reader()?;

            let chain_id: Option<String> = conn
                .query_row(
                    "SELECT version_chain_id FROM memories WHERE id = ?1",
                    params![&memory_id],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query version chain id")?
                .flatten();

            let missing_id = memory_id.clone();
            let (sql, params_values): (&str, Vec<SqlValue>) = if let Some(chain_id) = chain_id {
                (
                    "SELECT id, content, tags, importance, metadata, event_type, session_id, project,
                            superseded_by_id, superseded_at, version_chain_id, entity_id, agent_type
                     FROM memories WHERE version_chain_id = ?1 ORDER BY created_at ASC",
                    vec![SqlValue::Text(chain_id)],
                )
            } else {
                (
                    "SELECT id, content, tags, importance, metadata, event_type, session_id, project,
                            superseded_by_id, superseded_at, version_chain_id, entity_id, agent_type
                     FROM memories WHERE id = ?1 ORDER BY created_at ASC",
                    vec![SqlValue::Text(memory_id.clone())],
                )
            };

            let mut stmt = conn
                .prepare(sql)
                .context("failed to prepare version chain query")?;

            let mut refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                refs.push(value);
            }

            let rows = stmt
                .query_map(refs.as_slice(), |row| {
                    let raw_tags: String = row.get(2)?;
                    let raw_metadata: String = row.get(4)?;
                    let mut metadata = parse_metadata_from_db(&raw_metadata);
                    if let Some(object) = metadata.as_object_mut() {
                        object.insert(
                            "superseded_by_id".to_string(),
                            row.get::<_, Option<String>>(8)
                                .ok()
                                .flatten()
                                .map_or(serde_json::Value::Null, serde_json::Value::String),
                        );
                        object.insert(
                            "superseded_at".to_string(),
                            row.get::<_, Option<String>>(9)
                                .ok()
                                .flatten()
                                .map_or(serde_json::Value::Null, serde_json::Value::String),
                        );
                        object.insert(
                            "version_chain_id".to_string(),
                            row.get::<_, Option<String>>(10)
                                .ok()
                                .flatten()
                                .map_or(serde_json::Value::Null, serde_json::Value::String),
                        );
                    }
                    let event_type_str: Option<String> = row.get(5).ok();
                    Ok(SearchResult {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        tags: parse_tags_from_db(&raw_tags),
                        importance: row.get(3)?,
                        metadata,
                        event_type: event_type_from_sql(event_type_str),
                        session_id: row.get(6).ok(),
                        project: row.get(7).ok(),
                        entity_id: row.get(11).ok(),
                        agent_type: row.get(12).ok(),
                    })
                })
                .context("failed to execute version chain query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode version chain row")?);
            }
            if results.is_empty() {
                return Err(anyhow!("memory not found for id={missing_id}"));
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn supersede_memory(&self, old_id: &str, new_id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let old_id = old_id.to_string();
        let new_id = new_id.to_string();
        if old_id == new_id {
            return Err(anyhow!("cannot supersede memory with itself"));
        }

        tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;
            let tx = conn
                .unchecked_transaction()
                .context("failed to start supersede transaction")?;

            let old_exists: Option<String> = tx
                .query_row("SELECT id FROM memories WHERE id = ?1", params![old_id], |row| {
                    row.get(0)
                })
                .optional()
                .context("failed to query old memory")?;
            if old_exists.is_none() {
                return Err(anyhow!("memory not found for id={old_id}"));
            }

            let new_exists: Option<String> = tx
                .query_row("SELECT id FROM memories WHERE id = ?1", params![new_id], |row| {
                    row.get(0)
                })
                .optional()
                .context("failed to query new memory")?;
            if new_exists.is_none() {
                return Err(anyhow!("memory not found for id={new_id}"));
            }

            let old_chain_id: Option<String> = tx
                .query_row(
                    "SELECT version_chain_id FROM memories WHERE id = ?1",
                    params![old_id],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query old chain id")?
                .flatten();
            let new_chain_id: Option<String> = tx
                .query_row(
                    "SELECT version_chain_id FROM memories WHERE id = ?1",
                    params![new_id],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query new chain id")?
                .flatten();
            let chain_id = old_chain_id
                .clone()
                .or(new_chain_id.clone())
                .unwrap_or_else(|| old_id.clone());
            // Merge chains if old and new belonged to different chains
            if let (Some(old_c), Some(new_c)) = (&old_chain_id, &new_chain_id)
                && old_c != new_c
            {
                tx.execute(
                    "UPDATE memories SET version_chain_id = ?1 WHERE version_chain_id = ?2",
                    params![chain_id, new_c],
                )
                .context("failed to merge version chains in supersede_memory")?;
            }

            let now_str: String = tx
                .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
                    row.get::<_, String>(0)
                })
                .context("failed to get current timestamp from sqlite")?;

            tx.execute(
                "UPDATE memories
                 SET superseded_by_id = ?1,
                     superseded_at = ?2,
                     version_chain_id = COALESCE(version_chain_id, ?3)
                 WHERE id = ?4",
                params![new_id, now_str, chain_id, old_id],
            )
            .context("failed to mark old memory as superseded")?;

            tx.execute(
                "UPDATE memories SET version_chain_id = ?1 WHERE id = ?2",
                params![chain_id, new_id],
            )
            .context("failed to set chain id on new memory")?;

            tx.execute(
                "INSERT INTO relationships (id, source_id, target_id, rel_type, weight, metadata, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    Uuid::new_v4().to_string(),
                    old_id,
                    new_id,
                    "SUPERSEDES",
                    1.0f64,
                    "{}",
                    now_str,
                ],
            )
            .context("failed to create SUPERSEDES relationship")?;

            tx.commit().context("failed to commit supersede transaction")?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")?
    }
}
