#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use clap::Parser;
use cli::{Cli, Commands, InitModeArg};
use memory_core::storage::{InitMode, SqliteStorage};
use memory_core::{
    AdvancedSearcher, CheckpointInput, CheckpointManager, Deleter, Embedder, ExpirationSweeper,
    FeedbackRecorder, GraphTraverser, LessonQuerier, Lister, MaintenanceManager, MemoryInput,
    MemoryUpdate, PhraseSearcher, Pipeline, PlaceholderPipeline, ProfileManager,
    RelationshipQuerier, ReminderManager, SearchOptions, SimilarFinder, StatsProvider, Updater,
    VersionChainQuerier, WelcomeProvider, default_priority_for_event_type,
    default_ttl_for_event_type, is_valid_event_type,
};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

#[cfg(not(feature = "real-embeddings"))]
use memory_core::PlaceholderEmbedder;

mod cli;
mod mcp_server;
mod memory_core;

use mcp_server::McpMemoryServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    if matches!(cli.command, Commands::DownloadModel) {
        #[cfg(feature = "real-embeddings")]
        {
            println!("Preparing embedding model files...");
            let model_path = memory_core::embedder::download_bge_small_model().await?;
            println!("Embedding model is ready at {}", model_path.display());
        }
        #[cfg(not(feature = "real-embeddings"))]
        {
            anyhow::bail!("download-model requires the `real-embeddings` feature to be enabled");
        }
        return Ok(());
    }

    let storage_mode = match cli.init_mode {
        InitModeArg::Default => InitMode::Default,
        InitModeArg::Advanced => InitMode::Advanced,
    };
    let warmup = matches!(&cli.command, Commands::Serve);
    let embedder = build_embedder(warmup).await?;
    let sqlite_storage = SqliteStorage::new(storage_mode, Arc::clone(&embedder))?;
    let mcp_storage = sqlite_storage.clone();

    let pipeline = Pipeline::new(
        Box::new(PlaceholderPipeline),
        Box::new(PlaceholderPipeline),
        Box::new(sqlite_storage.clone()),
        Box::new(sqlite_storage),
        Box::new(mcp_storage.clone()),
        Box::new(mcp_storage.clone()),
        Box::new(mcp_storage.clone()),
    );
    match &cli.command {
        Commands::Ingest {
            content,
            tags,
            importance,
            metadata,
            event_type,
            session_id,
            project,
            priority,
            entity_id,
            agent_type,
            ttl_seconds,
        } => {
            info!(content_len = content.len(), "Ingesting content");
            let meta = parse_metadata_arg(metadata.as_deref())?;
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            let input = MemoryInput {
                content: content.clone(),
                id: None,
                tags: tags.clone(),
                importance: *importance,
                metadata: meta,
                event_type: event_type.clone(),
                session_id: session_id.clone(),
                project: project.clone(),
                priority: priority
                    .to_owned()
                    .or_else(|| event_type.as_deref().map(default_priority_for_event_type)),
                entity_id: entity_id.clone(),
                agent_type: agent_type.clone(),
                ttl_seconds: ttl_seconds.to_owned().or_else(|| {
                    event_type
                        .as_deref()
                        .map(default_ttl_for_event_type)
                        .unwrap_or(Some(memory_core::TTL_LONG_TERM))
                }),
            };
            let id = pipeline.run(content, &input).await?;
            info!(memory_id = %id, "Successfully processed and stored");
            println!("{}", json!({ "id": id }));
        }
        Commands::Process {
            content,
            tags,
            importance,
            metadata,
            event_type,
            session_id,
            project,
            priority,
            entity_id,
            agent_type,
            ttl_seconds,
        } => {
            info!(content_len = content.len(), "Processing content directly");
            let meta = parse_metadata_arg(metadata.as_deref())?;
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            let input = MemoryInput {
                content: content.clone(),
                id: None,
                tags: tags.clone(),
                importance: *importance,
                metadata: meta,
                event_type: event_type.clone(),
                session_id: session_id.clone(),
                project: project.clone(),
                priority: priority
                    .to_owned()
                    .or_else(|| event_type.as_deref().map(default_priority_for_event_type)),
                entity_id: entity_id.clone(),
                agent_type: agent_type.clone(),
                ttl_seconds: ttl_seconds.map(Some).unwrap_or_else(|| {
                    event_type
                        .as_deref()
                        .map(default_ttl_for_event_type)
                        .unwrap_or(Some(memory_core::TTL_LONG_TERM))
                }),
            };
            let id = pipeline.run(content, &input).await?;
            info!(memory_id = %id, "Process command stored result");
            println!("{}", json!({ "id": id }));
        }
        Commands::Retrieve { id } => {
            info!(memory_id = %id, "Retrieving memory");
            let result = pipeline.retrieve(id).await?;
            info!(memory_id = %id, content_len = result.len(), "Retrieved memory");
            println!("{}", json!({ "id": id, "content": result }));
        }
        Commands::Delete { id } => {
            info!(memory_id = %id, "Deleting memory");
            let deleted = mcp_storage.delete(id).await?;
            info!(memory_id = %id, deleted = deleted, "Delete completed");
            println!("{}", json!({ "id": id, "deleted": deleted }));
        }
        Commands::Update {
            id,
            content,
            tags,
            importance,
            metadata,
            event_type,
            priority,
        } => {
            info!(memory_id = %id, "Updating memory");
            // NOTE: We intentionally do NOT use `parse_metadata_arg` here because
            // Update needs to distinguish `None` (flag omitted → leave unchanged)
            // from `Some({})` (flag provided → set to empty object). The helper
            // defaults `None` to `{}`, which would always overwrite metadata.
            let meta = metadata
                .as_deref()
                .map(serde_json::from_str::<serde_json::Value>)
                .transpose()
                .map_err(|e| anyhow::anyhow!("invalid metadata JSON: {e}"))?;
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            if content.is_none()
                && tags.is_none()
                && importance.is_none()
                && meta.is_none()
                && event_type.is_none()
                && priority.is_none()
            {
                anyhow::bail!(
                    "at least one of --content, --tags, --importance, --metadata, --event-type, or --priority must be provided"
                );
            }
            let update = MemoryUpdate {
                content: content.clone(),
                tags: tags.clone(),
                importance: *importance,
                metadata: meta,
                event_type: event_type.clone(),
                priority: *priority,
            };
            <SqliteStorage as Updater>::update(&mcp_storage, id, &update).await?;
            info!(memory_id = %id, "Update completed");
            println!("{}", json!({ "id": id, "updated": true }));
        }
        Commands::List {
            offset,
            limit,
            event_type,
            project,
            session_id,
            include_superseded,
        } => {
            info!(offset = *offset, limit = *limit, "Listing memories");
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            let opts = SearchOptions {
                event_type: event_type.clone(),
                project: project.clone(),
                session_id: session_id.clone(),
                include_superseded: Some(*include_superseded),
                ..Default::default()
            };
            let result = mcp_storage.list(*offset, *limit, &opts).await?;
            info!(
                count = result.memories.len(),
                total = result.total,
                "List completed"
            );
            let payload: Vec<_> = result
                .memories
                .into_iter()
                .map(|m| {
                    json!({
                        "id": m.id,
                        "content": m.content,
                        "tags": m.tags,
                        "importance": m.importance,
                        "metadata": m.metadata,
                        "event_type": m.event_type,
                        "session_id": m.session_id,
                        "project": m.project
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload, "total": result.total }));
        }
        Commands::Relations { id } => {
            info!(memory_id = %id, "Querying relationships");
            let rels = mcp_storage.get_relationships(id).await?;
            info!(count = rels.len(), "Relationships retrieved");
            let payload: Vec<_> = rels
                .into_iter()
                .map(|r| {
                    json!({ "id": r.id, "source_id": r.source_id, "target_id": r.target_id, "rel_type": r.rel_type, "weight": r.weight, "metadata": r.metadata, "created_at": r.created_at })
                })
                .collect();
            println!("{}", json!({ "relationships": payload }));
        }
        Commands::Search {
            query,
            limit,
            event_type,
            project,
            session_id,
            include_superseded,
        } => {
            info!(
                query_len = query.len(),
                limit = *limit,
                "Searching memories"
            );
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            let opts = SearchOptions {
                event_type: event_type.clone(),
                project: project.clone(),
                session_id: session_id.clone(),
                include_superseded: Some(*include_superseded),
                ..Default::default()
            };
            let results = pipeline.search(query, *limit, &opts).await?;
            info!(result_count = results.len(), "Search completed");
            let payload: Vec<_> = results
                .into_iter()
                .map(|result| {
                    json!({
                        "id": result.id,
                        "content": result.content,
                        "tags": result.tags,
                        "importance": result.importance,
                        "metadata": result.metadata,
                        "event_type": result.event_type,
                        "session_id": result.session_id,
                        "project": result.project
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::SemanticSearch {
            query,
            limit,
            event_type,
            project,
            session_id,
            include_superseded,
        } => {
            info!(
                query_len = query.len(),
                limit = *limit,
                "Semantic searching memories"
            );
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            let opts = SearchOptions {
                event_type: event_type.clone(),
                project: project.clone(),
                session_id: session_id.clone(),
                include_superseded: Some(*include_superseded),
                ..Default::default()
            };
            let results = pipeline.semantic_search(query, *limit, &opts).await?;
            info!(result_count = results.len(), "Semantic search completed");
            let payload: Vec<_> = results
                .into_iter()
                .map(|result| {
                    json!({
                        "id": result.id,
                        "content": result.content,
                        "score": result.score,
                        "tags": result.tags,
                        "importance": result.importance,
                        "metadata": result.metadata,
                        "event_type": result.event_type,
                        "session_id": result.session_id,
                        "project": result.project
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::AdvancedSearch {
            query,
            limit,
            event_type,
            project,
            include_superseded,
        } => {
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            let opts = SearchOptions {
                event_type: event_type.clone(),
                project: project.clone(),
                include_superseded: Some(*include_superseded),
                ..Default::default()
            };
            let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
                &mcp_storage,
                query,
                *limit,
                &opts,
            )
            .await?;
            let payload: Vec<_> = results
                .into_iter()
                .map(|result| {
                    json!({
                        "id": result.id,
                        "content": result.content,
                        "score": result.score,
                        "tags": result.tags,
                        "importance": result.importance,
                        "metadata": result.metadata,
                        "event_type": result.event_type,
                        "session_id": result.session_id,
                        "project": result.project
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::VersionChain { id } => {
            let chain = mcp_storage.get_version_chain(id).await?;
            for memory in &chain {
                let superseded = memory
                    .metadata
                    .get("superseded_by_id")
                    .and_then(|v| v.as_str())
                    .map(|s| format!(" [superseded by {s}]"))
                    .unwrap_or_default();
                info!(id = %memory.id, event_type = ?memory.event_type, content_len = memory.content.len(), "{}", superseded);
            }
            info!("Chain length: {}", chain.len());
            let payload: Vec<_> = chain
                .into_iter()
                .map(|memory| {
                    json!({
                        "id": memory.id,
                        "content": memory.content,
                        "tags": memory.tags,
                        "importance": memory.importance,
                        "metadata": memory.metadata,
                        "event_type": memory.event_type,
                        "session_id": memory.session_id,
                        "project": memory.project,
                    })
                })
                .collect();
            println!("{}", json!({ "chain": payload }));
        }
        Commands::Similar { id, limit } => {
            let results =
                <SqliteStorage as SimilarFinder>::find_similar(&mcp_storage, id, *limit).await?;
            let payload: Vec<_> = results
                .into_iter()
                .map(|result| {
                    json!({
                        "id": result.id,
                        "content": result.content,
                        "score": result.score,
                        "tags": result.tags,
                        "importance": result.importance,
                        "metadata": result.metadata,
                        "event_type": result.event_type,
                        "session_id": result.session_id,
                        "project": result.project
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::Traverse {
            id,
            max_hops,
            min_weight,
        } => {
            let nodes = <SqliteStorage as GraphTraverser>::traverse(
                &mcp_storage,
                id,
                *max_hops,
                *min_weight,
                None,
            )
            .await?;

            let mut grouped = serde_json::Map::new();
            for node in nodes {
                let key = node.hop.to_string();
                let entry = grouped
                    .entry(key)
                    .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                if let serde_json::Value::Array(items) = entry {
                    items.push(json!({
                        "id": node.id,
                        "content": node.content,
                        "event_type": node.event_type,
                        "metadata": node.metadata,
                        "hop": node.hop,
                        "weight": node.weight,
                        "edge_type": node.edge_type,
                        "created_at": node.created_at
                    }));
                }
            }
            println!("{}", serde_json::Value::Object(grouped));
        }
        Commands::PhraseSearch {
            phrase,
            limit,
            event_type,
            include_superseded,
        } => {
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            let opts = SearchOptions {
                event_type: event_type.clone(),
                include_superseded: Some(*include_superseded),
                ..Default::default()
            };
            let results = <SqliteStorage as PhraseSearcher>::phrase_search(
                &mcp_storage,
                phrase,
                *limit,
                &opts,
            )
            .await?;
            let payload: Vec<_> = results
                .into_iter()
                .map(|result| {
                    json!({
                        "id": result.id,
                        "content": result.content,
                        "tags": result.tags,
                        "importance": result.importance,
                        "metadata": result.metadata,
                        "event_type": result.event_type,
                        "session_id": result.session_id,
                        "project": result.project
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::Recent {
            limit,
            event_type,
            project,
            session_id,
            include_superseded,
        } => {
            info!(limit = *limit, "Listing recent memories");
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            let opts = SearchOptions {
                event_type: event_type.clone(),
                project: project.clone(),
                session_id: session_id.clone(),
                include_superseded: Some(*include_superseded),
                ..Default::default()
            };
            let results = pipeline.recent(*limit, &opts).await?;
            info!(result_count = results.len(), "Recent list completed");
            let payload: Vec<_> = results
                .into_iter()
                .map(|result| {
                    json!({
                        "id": result.id,
                        "content": result.content,
                        "tags": result.tags,
                        "importance": result.importance,
                        "metadata": result.metadata,
                        "event_type": result.event_type,
                        "session_id": result.session_id,
                        "project": result.project
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::Stats => {
            info!("Getting memory stats");
            let stats = mcp_storage.stats().await?;
            info!("Stats retrieved successfully");
            println!("{}", serde_json::to_string_pretty(&stats)?);
        }
        Commands::Export => {
            info!("Exporting all memories");
            let data = mcp_storage.export_all().await?;
            info!(bytes = data.len(), "Export completed");
            println!("{data}");
        }
        Commands::Import { path } => {
            info!(source = %path, "Importing memories");
            let data = if path == "-" {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                buf
            } else {
                std::fs::read_to_string(path)?
            };
            let (memories, relationships) = mcp_storage.import_all(&data).await?;
            info!(
                imported_memories = memories,
                imported_relationships = relationships,
                "Import completed"
            );
            println!(
                "{}",
                json!({ "imported_memories": memories, "imported_relationships": relationships })
            );
        }
        Commands::Feedback {
            memory_id,
            rating,
            reason,
        } => {
            let result = <SqliteStorage as FeedbackRecorder>::record_feedback(
                &mcp_storage,
                memory_id,
                rating.as_str(),
                reason.as_deref(),
            )
            .await?;
            println!("{}", result);
        }
        Commands::Sweep => {
            let swept_count =
                <SqliteStorage as ExpirationSweeper>::sweep_expired(&mcp_storage).await?;
            println!("{}", json!({ "swept_count": swept_count }));
        }
        Commands::Profile { action, data } => match action.as_str() {
            "read" => {
                let profile = <SqliteStorage as ProfileManager>::get_profile(&mcp_storage).await?;
                println!("{}", profile);
            }
            "update" => {
                let raw = data
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("profile update requires JSON data"))?;
                let parsed: serde_json::Value = serde_json::from_str(raw)
                    .map_err(|e| anyhow::anyhow!("invalid profile JSON: {e}"))?;
                <SqliteStorage as ProfileManager>::set_profile(&mcp_storage, &parsed).await?;
                println!("{}", json!({ "updated": true }));
            }
            other => anyhow::bail!("invalid profile action: {other} (expected read|update)"),
        },
        Commands::Checkpoint {
            task_title,
            progress,
            plan,
            next_steps,
            session_id,
            project,
        } => {
            let input = CheckpointInput {
                task_title: task_title.clone(),
                progress: progress.clone(),
                plan: plan.clone(),
                files_touched: None,
                decisions: None,
                key_context: None,
                next_steps: next_steps.clone(),
                session_id: session_id.clone(),
                project: project.clone(),
            };
            let memory_id =
                <SqliteStorage as CheckpointManager>::save_checkpoint(&mcp_storage, input).await?;
            let latest = <SqliteStorage as CheckpointManager>::resume_task(
                &mcp_storage,
                task_title,
                project.as_deref(),
                1,
            )
            .await?;
            let checkpoint_number = latest
                .first()
                .and_then(|entry| entry.get("metadata"))
                .and_then(|metadata| metadata.get("checkpoint_number"))
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(1);
            println!(
                "{}",
                json!({ "memory_id": memory_id, "checkpoint_number": checkpoint_number })
            );
        }
        Commands::ResumeTask {
            task_title,
            project,
            limit,
        } => {
            let query = task_title.clone().unwrap_or_default();
            let results = <SqliteStorage as CheckpointManager>::resume_task(
                &mcp_storage,
                &query,
                project.as_deref(),
                *limit,
            )
            .await?;
            let mut markdown = String::new();
            for (index, entry) in results.iter().enumerate() {
                if index > 0 {
                    markdown.push_str("\n\n---\n\n");
                }
                markdown.push_str("### Checkpoint\n");
                markdown.push_str(entry["content"].as_str().unwrap_or(""));
                markdown.push_str("\n\nMetadata:\n");
                markdown.push_str(&entry["metadata"].to_string());
                markdown.push_str("\n\nCreated At: ");
                markdown.push_str(entry["created_at"].as_str().unwrap_or(""));
            }
            println!("{markdown}");
        }
        Commands::Remind {
            action,
            text,
            duration,
            context,
            session_id,
            project,
            status,
            reminder_id,
        } => match action.as_str() {
            "set" => {
                let text = text
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--text is required for remind set"))?;
                let duration = duration
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--duration is required for remind set"))?;
                let result = <SqliteStorage as ReminderManager>::create_reminder(
                    &mcp_storage,
                    text,
                    duration,
                    context.as_deref(),
                    session_id.as_deref(),
                    project.as_deref(),
                )
                .await?;
                println!("{}", result);
            }
            "list" => {
                let result = <SqliteStorage as ReminderManager>::list_reminders(
                    &mcp_storage,
                    status.as_deref(),
                )
                .await?;
                println!("{}", json!({ "results": result }));
            }
            "dismiss" => {
                let reminder_id = reminder_id.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("--reminder-id is required for remind dismiss")
                })?;
                let result =
                    <SqliteStorage as ReminderManager>::dismiss_reminder(&mcp_storage, reminder_id)
                        .await?;
                println!("{}", result);
            }
            other => anyhow::bail!("invalid remind action: {other} (expected set|list|dismiss)"),
        },
        Commands::Lessons {
            task,
            project,
            limit,
        } => {
            let results = <SqliteStorage as LessonQuerier>::query_lessons(
                &mcp_storage,
                task.as_deref(),
                project.as_deref(),
                None,
                None,
                *limit,
            )
            .await?;
            println!("{}", json!({ "results": results }));
        }
        Commands::Maintain {
            action,
            warn_mb,
            critical_mb,
            max_nodes,
            prune_days,
            max_summaries,
            event_type,
            similarity_threshold,
            min_cluster_size,
            dry_run,
            session_id,
        } => match action.as_str() {
            "health" => {
                let result = <SqliteStorage as MaintenanceManager>::check_health(
                    &mcp_storage,
                    warn_mb.unwrap_or(350.0),
                    critical_mb.unwrap_or(800.0),
                    max_nodes.unwrap_or(10000),
                )
                .await?;
                println!("{result}");
            }
            "consolidate" => {
                let result = <SqliteStorage as MaintenanceManager>::consolidate(
                    &mcp_storage,
                    prune_days.unwrap_or(30),
                    max_summaries.unwrap_or(50),
                )
                .await?;
                println!("{result}");
            }
            "compact" => {
                let result = <SqliteStorage as MaintenanceManager>::compact(
                    &mcp_storage,
                    event_type.as_deref().unwrap_or("lesson_learned"),
                    similarity_threshold.unwrap_or(0.6),
                    min_cluster_size.unwrap_or(3),
                    *dry_run,
                )
                .await?;
                println!("{result}");
            }
            "clear_session" | "clear-session" => {
                let sid = session_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--session-id is required for clear-session"))?;
                let removed =
                    <SqliteStorage as MaintenanceManager>::clear_session(&mcp_storage, sid).await?;
                println!("{}", json!({"session_id": sid, "removed": removed}));
            }
            other => anyhow::bail!(
                "invalid maintain action: {other} (expected health|consolidate|compact|clear-session)"
            ),
        },
        Commands::Welcome {
            session_id,
            project,
        } => {
            let result = <SqliteStorage as WelcomeProvider>::welcome(
                &mcp_storage,
                session_id.as_deref(),
                project.as_deref(),
            )
            .await?;
            println!("{result}");
        }
        Commands::Protocol { section: _ } => {
            let protocol = serde_json::json!({
                "tools": [
                    "memory_store", "memory_retrieve", "memory_delete", "memory_update",
                    "memory_search", "memory_semantic_search", "memory_advanced_search",
                    "memory_similar", "memory_traverse", "memory_phrase_search",
                    "memory_tag_search", "memory_list", "memory_recent",
                    "memory_relations", "memory_add_relation",
                    "memory_version_chain",
                    "memory_feedback", "memory_sweep",
                    "memory_profile", "memory_checkpoint", "memory_resume_task",
                    "memory_remind", "memory_lessons",
                    "memory_health", "memory_stats", "memory_export", "memory_import",
                    "memory_maintain", "memory_welcome", "memory_protocol", "memory_stats_extended",
                ],
                "tool_count": 31,
            });
            println!("{protocol}");
        }
        Commands::StatsExtended { action, days } => match action.as_str() {
            "types" => {
                let result = <SqliteStorage as StatsProvider>::type_stats(&mcp_storage).await?;
                println!("{result}");
            }
            "sessions" => {
                let result = <SqliteStorage as StatsProvider>::session_stats(&mcp_storage).await?;
                println!("{result}");
            }
            "digest" => {
                let result =
                    <SqliteStorage as StatsProvider>::weekly_digest(&mcp_storage, *days).await?;
                println!("{result}");
            }
            "access_rate" | "access-rate" => {
                let result =
                    <SqliteStorage as StatsProvider>::access_rate_stats(&mcp_storage).await?;
                println!("{result}");
            }
            other => anyhow::bail!(
                "invalid stats-extended action: {other} (expected types|sessions|digest|access-rate)"
            ),
        },
        Commands::DownloadModel => {
            unreachable!("download-model is handled before storage initialization")
        }
        Commands::Serve => {
            info!("Starting MCP server over stdio");
            McpMemoryServer::new(mcp_storage).serve_stdio().await?;
        }
    }

    Ok(())
}

async fn build_embedder(warmup: bool) -> anyhow::Result<Arc<dyn Embedder>> {
    #[cfg(feature = "real-embeddings")]
    {
        let onnx = Arc::new(memory_core::OnnxEmbedder::new()?);
        if warmup {
            onnx.warmup().await?;
        }
        Ok(onnx)
    }

    #[cfg(not(feature = "real-embeddings"))]
    {
        Ok(Arc::new(PlaceholderEmbedder))
    }
}

fn parse_metadata_arg(metadata: Option<&str>) -> anyhow::Result<serde_json::Value> {
    match metadata {
        Some(s) => {
            serde_json::from_str(s).map_err(|e| anyhow::anyhow!("invalid metadata JSON: {e}"))
        }
        None => Ok(serde_json::json!({})),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
