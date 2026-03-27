use super::*;

impl SqliteStorage {
    pub async fn get_profile(&self) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let mut profile = serde_json::Map::new();
            let mut stmt = conn
                .prepare("SELECT key, value FROM user_profile")
                .context("failed to prepare user profile query")?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .context("failed to query user profile rows")?;

            for row in rows {
                let (key, value_raw) = row.context("failed to decode user profile row")?;
                let value = serde_json::from_str::<serde_json::Value>(&value_raw)
                    .unwrap_or(serde_json::Value::String(value_raw));
                profile.insert(key, value);
            }

            let mut pref_stmt = conn
                .prepare(
                    "SELECT id, content, metadata, created_at
                     FROM memories
                     WHERE event_type = 'user_preference'
                     ORDER BY created_at DESC
                     LIMIT 20",
                )
                .context("failed to prepare preference query")?;
            let pref_rows = pref_stmt
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "content": row.get::<_, String>(1)?,
                        "metadata": parse_metadata_from_db(&row.get::<_, String>(2)?),
                        "created_at": row.get::<_, String>(3)?,
                    }))
                })
                .context("failed to query preferences from memory")?;

            let mut preferences_from_memory = Vec::new();
            for row in pref_rows {
                preferences_from_memory
                    .push(row.context("failed to decode preference from memory row")?);
            }
            profile.insert(
                "preferences_from_memory".to_string(),
                serde_json::Value::Array(preferences_from_memory),
            );

            Ok::<_, anyhow::Error>(serde_json::Value::Object(profile))
        })
        .await
        .context("spawn_blocking join error")?
    }

    pub async fn set_profile(&self, updates: &serde_json::Value) -> Result<()> {
        let updates = updates.clone();
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let updates_obj = updates
                .as_object()
                .ok_or_else(|| anyhow!("profile updates must be a JSON object"))?;
            let conn = pool.writer()?;

            for (key, value) in updates_obj {
                let value_json = serde_json::to_string(value)
                    .context("failed to serialize user profile value")?;
                conn.execute(
                    "INSERT OR REPLACE INTO user_profile (key, value, updated_at)
                     VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                    params![key, value_json],
                )
                .context("failed to upsert user profile value")?;
            }

            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(())
    }
}

impl SqliteStorage {
    pub async fn save_checkpoint(&self, input: CheckpointInput) -> Result<String> {
        let pool = Arc::clone(&self.pool);
        let task_title = input.task_title.clone();
        let task_marker = format!("## Checkpoint: {}", task_title);
        let existing_count = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            let mut stmt = conn
                .prepare(
                    "SELECT COUNT(*) FROM memories
                     WHERE event_type = 'checkpoint' AND lower(content) LIKE lower(?1) ESCAPE '\\'",
                )
                .context("failed to prepare checkpoint count query")?;
            let pattern = escape_like_pattern(&task_marker);
            let count: i64 = stmt
                .query_row(params![pattern], |row| row.get(0))
                .context("failed to count matching checkpoints")?;
            Ok::<_, anyhow::Error>(count)
        })
        .await
        .context("spawn_blocking join error")??;

        let checkpoint_number = existing_count + 1;
        let mut content = format!(
            "## Checkpoint: {}\n### Progress\n{}",
            input.task_title, input.progress
        );
        if let Some(plan) = input.plan.as_deref() {
            content.push_str("\n\n### Plan\n");
            content.push_str(plan);
        }
        if let Some(files_touched) = input.files_touched.as_ref() {
            content.push_str("\n\n### Files Touched\n");
            content.push_str(
                &serde_json::to_string_pretty(files_touched)
                    .context("failed to serialize files_touched for checkpoint")?,
            );
        }
        if let Some(decisions) = input.decisions.as_ref()
            && !decisions.is_empty()
        {
            content.push_str("\n\n### Decisions\n");
            for decision in decisions {
                content.push_str("- ");
                content.push_str(decision);
                content.push('\n');
            }
        }
        if let Some(key_context) = input.key_context.as_deref() {
            content.push_str("\n### Key Context\n");
            content.push_str(key_context);
        }
        if let Some(next_steps) = input.next_steps.as_deref() {
            content.push_str("\n\n### Next Steps\n");
            content.push_str(next_steps);
        }

        let metadata = serde_json::json!({
            "checkpoint_number": checkpoint_number,
            "checkpoint_data": {
                "task_title": input.task_title,
                "plan": input.plan,
                "progress": input.progress,
                "files_touched": input.files_touched,
                "decisions": input.decisions,
                "key_context": input.key_context,
                "next_steps": input.next_steps,
            }
        });

        let id = Uuid::new_v4().to_string();
        let memory_input = MemoryInput {
            content: content.clone(),
            id: Some(id.clone()),
            metadata,
            event_type: Some(EventType::Checkpoint),
            priority: Some(EventType::Checkpoint.default_priority()),
            ttl_seconds: EventType::Checkpoint.default_ttl(),
            session_id: input.session_id,
            project: input.project,
            ..Default::default()
        };

        <Self as Storage>::store(self, &id, &content, &memory_input).await?;
        Ok(id)
    }

    pub async fn resume_task(
        &self,
        query: &str,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let pool = Arc::clone(&self.pool);
        let query = query.to_string();
        let project = project.map(ToString::to_string);
        let limit = i64::try_from(limit).context("resume_task limit exceeds i64")?;

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let mut sql = String::from(
                "SELECT content, metadata, created_at
                 FROM memories
                 WHERE event_type = 'checkpoint'",
            );
            let mut params_values: Vec<rusqlite::types::Value> = Vec::new();
            let mut idx = 1;

            if !query.trim().is_empty() {
                sql.push_str(&format!(" AND lower(content) LIKE ?{idx} ESCAPE '\\'"));
                params_values.push(rusqlite::types::Value::Text(escape_like_pattern(&query)));
                idx += 1;
            }

            if let Some(project_value) = project {
                sql.push_str(&format!(" AND project = ?{idx}"));
                params_values.push(rusqlite::types::Value::Text(project_value));
                idx += 1;
            }

            sql.push_str(" ORDER BY created_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(rusqlite::types::Value::Integer(limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare resume_task query")?;
            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                param_refs.push(value);
            }

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok(serde_json::json!({
                        "content": row.get::<_, String>(0)?,
                        "metadata": parse_metadata_from_db(&row.get::<_, String>(1)?),
                        "created_at": row.get::<_, String>(2)?,
                    }))
                })
                .context("failed to execute resume_task query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode resume_task row")?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

impl SqliteStorage {
    pub async fn create_reminder(
        &self,
        text: &str,
        duration_str: &str,
        context: Option<&str>,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value> {
        let duration = crate::memory_core::parse_duration(duration_str)?;
        let now = Utc::now();
        let remind_at = now + duration;
        let remind_at_iso = remind_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let now_iso = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let mut metadata = serde_json::json!({
            "event_type": "reminder",
            "reminder_status": "pending",
            "remind_at": remind_at_iso,
            "created_at_utc": now_iso,
        });
        if let Some(context_value) = context {
            metadata["context"] = serde_json::Value::String(context_value.to_string());
        }
        if let Some(session_value) = session_id {
            metadata["session_id"] = serde_json::Value::String(session_value.to_string());
        }
        if let Some(project_value) = project {
            metadata["project"] = serde_json::Value::String(project_value.to_string());
        }

        let reminder_id = Uuid::new_v4().to_string();
        let content = format!("{text}\n[due: {remind_at_iso}]");
        let input = MemoryInput {
            content: content.clone(),
            id: Some(reminder_id.clone()),
            metadata,
            event_type: Some(EventType::Reminder),
            priority: Some(EventType::Reminder.default_priority()),
            ttl_seconds: EventType::Reminder.default_ttl(),
            session_id: session_id.map(ToString::to_string),
            project: project.map(ToString::to_string),
            ..Default::default()
        };

        <Self as Storage>::store(self, &reminder_id, &content, &input).await?;

        Ok(serde_json::json!({
            "reminder_id": reminder_id,
            "text": text,
            "remind_at": remind_at_iso,
            "duration": duration_str,
        }))
    }

    pub async fn list_reminders(&self, status: Option<&str>) -> Result<Vec<serde_json::Value>> {
        let pool = Arc::clone(&self.pool);
        let status = status.map(ToString::to_string);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let mut stmt = conn
                .prepare(
                    "SELECT id, content, metadata, created_at
                     FROM memories
                     WHERE event_type = 'reminder'",
                )
                .context("failed to prepare reminder list query")?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .context("failed to execute reminder list query")?;

            let now = Utc::now();
            let status_filter = status.unwrap_or_else(|| "pending".to_string());
            let include_all = status_filter == "all";

            let mut reminders: Vec<(bool, DateTime<Utc>, serde_json::Value)> = Vec::new();

            for row in rows {
                let (id, content, metadata_raw, created_at) =
                    row.context("failed to decode reminder row")?;
                let metadata = parse_metadata_from_db(&metadata_raw);
                let reminder_status = metadata
                    .get("reminder_status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("pending");

                if !include_all && reminder_status != status_filter {
                    continue;
                }

                let remind_at_str = metadata
                    .get("remind_at")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(EPOCH_FALLBACK);
                let remind_at = DateTime::parse_from_rfc3339(remind_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| {
                        DateTime::parse_from_rfc3339("9999-12-31T23:59:59.000Z")
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or(now)
                    });
                let is_due = now >= remind_at;
                let is_overdue = is_due && reminder_status == "pending";

                reminders.push((
                    is_overdue,
                    remind_at,
                    serde_json::json!({
                        "reminder_id": id,
                        "text": content,
                        "status": reminder_status,
                        "remind_at": remind_at_str,
                        "is_due": is_due,
                        "is_overdue": is_overdue,
                        "metadata": metadata,
                        "created_at": created_at,
                    }),
                ));
            }

            reminders.sort_by(|a, b| {
                b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)).then_with(|| {
                    a.2["created_at"]
                        .as_str()
                        .unwrap_or_default()
                        .cmp(b.2["created_at"].as_str().unwrap_or_default())
                })
            });

            Ok::<_, anyhow::Error>(reminders.into_iter().map(|(_, _, value)| value).collect())
        })
        .await
        .context("spawn_blocking join error")?
    }

    pub async fn dismiss_reminder(&self, reminder_id: &str) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);
        let reminder_id = reminder_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;

            let row: Option<(Option<String>, String)> = conn
                .query_row(
                    "SELECT event_type, metadata FROM memories WHERE id = ?1",
                    params![reminder_id],
                    |row| Ok((row.get(0).ok(), row.get(1)?)),
                )
                .optional()
                .context("failed to query reminder for dismiss")?;

            let (event_type, metadata_raw) =
                row.ok_or_else(|| anyhow!("reminder not found for id={reminder_id}"))?;
            if event_type.as_deref() != Some("reminder") {
                return Err(anyhow!("memory is not a reminder for id={reminder_id}"));
            }

            let mut metadata = parse_metadata_from_db(&metadata_raw);
            let dismissed_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            metadata["reminder_status"] = serde_json::Value::String("dismissed".to_string());
            metadata["dismissed_at"] = serde_json::Value::String(dismissed_at.clone());
            let metadata_json = serde_json::to_string(&metadata)
                .context("failed to serialize dismissed reminder metadata")?;

            conn.execute(
                "UPDATE memories
                 SET metadata = ?2,
                     last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![reminder_id, metadata_json],
            )
            .context("failed to update reminder status")?;

            Ok::<_, anyhow::Error>(serde_json::json!({
                "reminder_id": reminder_id,
                "status": "dismissed",
                "dismissed_at": dismissed_at,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }
}

impl SqliteStorage {
    pub async fn query_lessons(
        &self,
        task: Option<&str>,
        project: Option<&str>,
        exclude_session: Option<&str>,
        agent_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let pool = Arc::clone(&self.pool);
        let task = task.map(ToString::to_string);
        let project = project.map(ToString::to_string);
        let exclude_session = exclude_session.map(ToString::to_string);
        let agent_type = agent_type.map(ToString::to_string);
        let limit = i64::try_from(limit).context("lessons query limit exceeds i64")?;

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let mut sql = String::from(
                "SELECT id, content, session_id, access_count, created_at, metadata, project, agent_type
                 FROM memories
                 WHERE event_type = 'lesson_learned' AND superseded_by_id IS NULL",
            );
            let mut params_values: Vec<rusqlite::types::Value> = Vec::new();
            let mut idx = 1;

            if let Some(task_value) = task {
                sql.push_str(&format!(" AND lower(content) LIKE ?{idx} ESCAPE '\\'"));
                params_values.push(rusqlite::types::Value::Text(escape_like_pattern(&task_value)));
                idx += 1;
            }

            if let Some(ref project_value) = project {
                sql.push_str(&format!(" AND project = ?{idx}"));
                params_values.push(rusqlite::types::Value::Text(project_value.clone()));
                idx += 1;
            }

            if let Some(ref exclude_value) = exclude_session {
                sql.push_str(&format!(" AND (session_id IS NULL OR session_id != ?{idx})"));
                params_values.push(rusqlite::types::Value::Text(exclude_value.clone()));
                idx += 1;
            }

            if let Some(ref agent_value) = agent_type {
                sql.push_str(&format!(" AND agent_type = ?{idx}"));
                params_values.push(rusqlite::types::Value::Text(agent_value.clone()));
                idx += 1;
            }

            sql.push_str(" ORDER BY access_count DESC, created_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(rusqlite::types::Value::Integer(limit * 4));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare lesson query")?;
            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                param_refs.push(value);
            }

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2).ok().flatten(),
                        row.get::<_, i64>(3).unwrap_or(0),
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6).ok().flatten(),
                        row.get::<_, Option<String>>(7).ok().flatten(),
                    ))
                })
                .context("failed to execute lesson query")?;

            let mut dedup_keys = HashSet::new();
            let mut results = Vec::new();
            for row in rows {
                let (
                    id,
                    content,
                    session_id,
                    access_count,
                    created_at,
                    metadata_raw,
                    project_col,
                    agent_type_col,
                ) = row.context("failed to decode lesson row")?;
                let metadata = parse_metadata_from_db(&metadata_raw);

                if let Some(project_filter) = project.as_deref() {
                    let metadata_project = metadata
                        .get("project")
                        .and_then(serde_json::Value::as_str)
                        .map(ToString::to_string)
                        .or(project_col);
                    if metadata_project.as_deref() != Some(project_filter) {
                        continue;
                    }
                }

                if let Some(exclude) = exclude_session.as_deref()
                    && session_id.as_deref() == Some(exclude)
                {
                    continue;
                }

                if let Some(agent_filter) = agent_type.as_deref() {
                    let metadata_agent = metadata
                        .get("agent_type")
                        .and_then(serde_json::Value::as_str)
                        .map(ToString::to_string)
                        .or(agent_type_col);
                    if metadata_agent.as_deref() != Some(agent_filter) {
                        continue;
                    }
                }

                let dedup_key = content.chars().take(80).collect::<String>().to_lowercase();
                if !dedup_keys.insert(dedup_key) {
                    continue;
                }

                results.push(serde_json::json!({
                    "content": content,
                    "lesson_id": id,
                    "session_id": session_id,
                    "access_count": access_count,
                    "created_at": created_at,
                }));
            }

            results.sort_by(|a, b| {
                b["access_count"]
                    .as_i64()
                    .unwrap_or(0)
                    .cmp(&a["access_count"].as_i64().unwrap_or(0))
            });
            results.truncate(usize::try_from(limit).unwrap_or(usize::MAX));

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}
