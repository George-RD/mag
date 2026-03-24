#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use chrono::{DateTime, FixedOffset, NaiveDate, SecondsFormat, TimeZone, Utc};
use clap::Parser;
use cli::{Cli, Commands, InitModeArg, SearchFilterArgs};
use memory_core::storage::{InitMode, SqliteStorage};
use memory_core::{
    AdvancedSearcher, BackupManager, CheckpointInput, CheckpointManager, Deleter, Embedder,
    EventType, ExpirationSweeper, FeedbackRecorder, GraphTraverser, LessonQuerier, Lister,
    MaintenanceManager, MemoryInput, MemoryUpdate, PhraseSearcher, Pipeline, PlaceholderPipeline,
    ProfileManager, RelationshipQuerier, ReminderManager, SearchOptions, SimilarFinder,
    StatsProvider, Updater, VersionChainQuerier, WelcomeProvider, is_valid_event_type,
};
use serde_json::json;
use std::sync::Arc;
use tracing::info;

#[cfg(not(feature = "real-embeddings"))]
use memory_core::PlaceholderEmbedder;

mod app_paths;
mod cli;
#[allow(dead_code)] // Consumers (daemon HTTP server) land in a follow-up PR.
mod idle_timer;
mod mcp_server;
mod memory_core;

use mcp_server::McpMemoryServer;

#[derive(Clone, Copy)]
struct SearchTimeFilters<'a> {
    created_after: &'a Option<String>,
    created_before: &'a Option<String>,
    event_after: &'a Option<String>,
    event_before: &'a Option<String>,
}

struct NormalizedSearchTimeFilters {
    created_after: Option<String>,
    created_before: Option<String>,
    event_after: Option<String>,
    event_before: Option<String>,
}

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

    if matches!(cli.command, Commands::DownloadCrossEncoder) {
        #[cfg(feature = "real-embeddings")]
        {
            println!("Preparing cross-encoder model files...");
            let model_path = memory_core::reranker::download_cross_encoder_model().await?;
            println!("Cross-encoder model is ready at {}", model_path.display());
        }
        #[cfg(not(feature = "real-embeddings"))]
        {
            anyhow::bail!(
                "download-cross-encoder requires the `real-embeddings` feature to be enabled"
            );
        }
        return Ok(());
    }

    if let Commands::Doctor { verbose } = &cli.command {
        return run_doctor(*verbose).await;
    }

    if matches!(cli.command, Commands::Paths) {
        let paths = app_paths::resolve_app_paths()?;
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "data_root": paths.data_root,
                "database_path": paths.database_path,
                "model_root": paths.model_root,
                "benchmark_root": paths.benchmark_root,
            }))?
        );
        return Ok(());
    }

    let storage_mode = match cli.init_mode {
        InitModeArg::Default => InitMode::Default,
        InitModeArg::Advanced => InitMode::Advanced,
    };
    let warmup = matches!(&cli.command, Commands::Serve { .. });

    #[cfg(feature = "real-embeddings")]
    let onnx_embedder_ref = {
        let onnx = std::sync::Arc::new(memory_core::OnnxEmbedder::new()?);
        if warmup {
            onnx.warmup().await?;
        }
        onnx
    };
    #[cfg(feature = "real-embeddings")]
    let embedder: Arc<dyn Embedder> = onnx_embedder_ref.clone();

    #[cfg(not(feature = "real-embeddings"))]
    let embedder: Arc<dyn Embedder> = Arc::new(PlaceholderEmbedder);
    let sqlite_storage = SqliteStorage::new(storage_mode, Arc::clone(&embedder))?;

    // Automatic startup backup (file-backed DBs only, max every 24h)
    if let Err(e) = <SqliteStorage as BackupManager>::maybe_startup_backup(&sqlite_storage).await {
        tracing::warn!("startup backup failed (non-fatal): {e}");
    }

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
            referenced_date,
        } => {
            info!(content_len = content.len(), "Ingesting content");
            let meta = parse_metadata_arg(metadata.as_deref())?;
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            let mut input = MemoryInput {
                content: content.clone(),
                id: None,
                tags: tags.clone(),
                importance: *importance,
                metadata: meta,
                session_id: session_id.clone(),
                project: project.clone(),
                priority: *priority,
                entity_id: entity_id.clone(),
                agent_type: agent_type.clone(),
                ttl_seconds: *ttl_seconds,
                referenced_date: referenced_date.clone(),
                ..MemoryInput::default()
            };
            input.apply_event_type_defaults(event_type.as_deref());
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
            referenced_date,
        } => {
            info!(content_len = content.len(), "Processing content directly");
            let meta = parse_metadata_arg(metadata.as_deref())?;
            if let Some(kind) = event_type.as_deref()
                && !is_valid_event_type(kind)
            {
                anyhow::bail!("invalid --event-type: {kind}");
            }
            let mut input = MemoryInput {
                content: content.clone(),
                id: None,
                tags: tags.clone(),
                importance: *importance,
                metadata: meta,
                session_id: session_id.clone(),
                project: project.clone(),
                priority: *priority,
                entity_id: entity_id.clone(),
                agent_type: agent_type.clone(),
                ttl_seconds: *ttl_seconds,
                referenced_date: referenced_date.clone(),
                ..MemoryInput::default()
            };
            input.apply_event_type_defaults(event_type.as_deref());
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
                event_type: EventType::from_optional(event_type.as_deref()),
                priority: *priority,
            };
            <SqliteStorage as Updater>::update(&mcp_storage, id, &update).await?;
            info!(memory_id = %id, "Update completed");
            println!("{}", json!({ "id": id, "updated": true }));
        }
        Commands::List {
            offset,
            limit,
            filters,
        } => {
            info!(offset = *offset, limit = *limit, "Listing memories");
            let opts = build_search_options(filters, false)?;
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
                        "project": m.project,
                        "entity_id": m.entity_id,
                        "agent_type": m.agent_type
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
        Commands::Paths => {
            unreachable!("paths is handled before storage initialization");
        }
        Commands::Search {
            query,
            limit,
            filters,
        } => {
            info!(
                query_len = query.len(),
                limit = *limit,
                "Searching memories"
            );
            let opts = build_search_options(filters, false)?;
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
                        "project": result.project,
                        "entity_id": result.entity_id,
                        "agent_type": result.agent_type
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::SemanticSearch {
            query,
            limit,
            filters,
        } => {
            info!(
                query_len = query.len(),
                limit = *limit,
                "Semantic searching memories"
            );
            let opts = build_search_options(filters, false)?;
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
                        "project": result.project,
                        "entity_id": result.entity_id,
                        "agent_type": result.agent_type
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::AdvancedSearch {
            query,
            limit,
            filters,
            explain,
        } => {
            let opts = build_search_options(filters, *explain)?;
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
                        "project": result.project,
                        "entity_id": result.entity_id,
                        "agent_type": result.agent_type
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
                        "entity_id": memory.entity_id,
                        "agent_type": memory.agent_type,
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
                        "project": result.project,
                        "entity_id": result.entity_id,
                        "agent_type": result.agent_type
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
            filters,
        } => {
            let opts = build_search_options(filters, false)?;
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
                        "project": result.project,
                        "entity_id": result.entity_id,
                        "agent_type": result.agent_type
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::Recent { limit, filters } => {
            info!(limit = *limit, "Listing recent memories");
            let opts = build_search_options(filters, false)?;
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
                        "project": result.project,
                        "entity_id": result.entity_id,
                        "agent_type": result.agent_type
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
            backup_path,
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
            "backup" => {
                let info = <SqliteStorage as BackupManager>::create_backup(&mcp_storage).await?;
                <SqliteStorage as BackupManager>::rotate_backups(&mcp_storage, 5).await?;
                println!(
                    "{}",
                    json!({
                        "path": info.path.display().to_string(),
                        "size_bytes": info.size_bytes,
                        "created_at": info.created_at,
                    })
                );
            }
            "backup-list" | "backup_list" => {
                let backups = <SqliteStorage as BackupManager>::list_backups(&mcp_storage).await?;
                let payload: Vec<_> = backups
                    .iter()
                    .map(|b| {
                        json!({
                            "path": b.path.display().to_string(),
                            "size_bytes": b.size_bytes,
                            "created_at": b.created_at,
                        })
                    })
                    .collect();
                println!("{}", json!({ "backups": payload, "count": backups.len() }));
            }
            "backup-restore" | "backup_restore" => {
                let path_str = backup_path.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("--backup-path is required for backup-restore")
                })?;
                let path = std::path::Path::new(path_str);
                <SqliteStorage as BackupManager>::restore_backup(&mcp_storage, path).await?;
                println!(
                    "{}",
                    json!({
                        "restored": true,
                        "from": path_str,
                        "note": "restart the server to use the restored database"
                    })
                );
            }
            other => anyhow::bail!(
                "invalid maintain action: {other} (expected health|consolidate|compact|clear-session|backup|backup-list|backup-restore)"
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
            let protocol = mcp_server::tool_registry_json();
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
        Commands::Doctor { .. } => {
            unreachable!("doctor is handled before storage initialization")
        }
        Commands::DownloadModel => {
            unreachable!("download-model is handled before storage initialization")
        }
        Commands::DownloadCrossEncoder => {
            unreachable!("download-cross-encoder is handled before storage initialization")
        }
        Commands::Serve { cross_encoder } => {
            info!("Starting MCP server over stdio");

            #[cfg(feature = "real-embeddings")]
            let mcp_storage = if *cross_encoder {
                info!("Cross-encoder reranking enabled");
                let reranker =
                    std::sync::Arc::new(memory_core::reranker::CrossEncoderReranker::new()?);
                reranker.warmup().await?;
                let reranker_for_tick = reranker.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                    loop {
                        interval.tick().await;
                        reranker_for_tick.maintenance_tick().await;
                    }
                });
                mcp_storage.with_reranker(reranker)
            } else {
                mcp_storage
            };

            #[cfg(not(feature = "real-embeddings"))]
            if *cross_encoder {
                anyhow::bail!(
                    "--cross-encoder requires the `real-embeddings` feature to be enabled"
                );
            }

            #[cfg(feature = "real-embeddings")]
            {
                let onnx_for_tick = onnx_embedder_ref.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
                    loop {
                        interval.tick().await;
                        onnx_for_tick.maintenance_tick().await;
                    }
                });
            }

            {
                let storage_for_optimize = mcp_storage.clone();
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
                    loop {
                        interval.tick().await;
                        if let Err(e) = storage_for_optimize.optimize().await {
                            tracing::warn!("PRAGMA optimize failed: {e}");
                        }
                    }
                });
            }

            McpMemoryServer::new(mcp_storage).serve_stdio().await?;
        }
    }

    Ok(())
}

fn parse_metadata_arg(metadata: Option<&str>) -> anyhow::Result<serde_json::Value> {
    match metadata {
        Some(s) => {
            serde_json::from_str(s).map_err(|e| anyhow::anyhow!("invalid metadata JSON: {e}"))
        }
        None => Ok(serde_json::json!({})),
    }
}

fn build_search_options(
    filters: &SearchFilterArgs,
    explain: bool,
) -> anyhow::Result<SearchOptions> {
    if let Some(kind) = filters.event_type.as_deref()
        && !is_valid_event_type(kind)
    {
        anyhow::bail!("invalid --event-type: {kind}");
    }

    if let Some(imp) = filters.importance_min
        && !(0.0..=1.0).contains(&imp)
    {
        anyhow::bail!("--importance-min must be between 0.0 and 1.0, got {imp}");
    }

    let times = normalize_search_time_filters(SearchTimeFilters {
        created_after: &filters.created_after,
        created_before: &filters.created_before,
        event_after: &filters.event_after,
        event_before: &filters.event_before,
    })?;

    #[allow(clippy::needless_update)]
    let opts = SearchOptions {
        event_type: EventType::from_optional(filters.event_type.as_deref()),
        project: filters.project.clone(),
        session_id: filters.session_id.clone(),
        entity_id: filters.entity_id.clone(),
        agent_type: filters.agent_type.clone(),
        include_superseded: Some(filters.include_superseded),
        importance_min: filters.importance_min,
        created_after: times.created_after,
        created_before: times.created_before,
        context_tags: filters.context_tags.clone(),
        event_after: times.event_after,
        event_before: times.event_before,
        explain: explain.then_some(true),
        ..SearchOptions::default()
    };
    Ok(opts)
}

fn normalize_search_time_filters(
    times: SearchTimeFilters<'_>,
) -> anyhow::Result<NormalizedSearchTimeFilters> {
    let created_after = parse_cli_time_filter("created-after", times.created_after.as_deref())?;
    let created_before = parse_cli_time_filter("created-before", times.created_before.as_deref())?;
    let event_after = parse_cli_time_filter("event-after", times.event_after.as_deref())?;
    let event_before = parse_cli_time_filter("event-before", times.event_before.as_deref())?;

    validate_time_range("created", created_after, created_before)?;
    validate_time_range("event", event_after, event_before)?;
    Ok(NormalizedSearchTimeFilters {
        created_after: created_after.map(format_time_filter),
        created_before: created_before.map(format_time_filter),
        event_after: event_after.map(format_time_filter),
        event_before: event_before.map(format_time_filter),
    })
}

fn validate_time_range(
    label: &str,
    after: Option<DateTime<FixedOffset>>,
    before: Option<DateTime<FixedOffset>>,
) -> anyhow::Result<()> {
    if let (Some(after), Some(before)) = (after, before)
        && after > before
    {
        anyhow::bail!("invalid --{label}-* range: after must be <= before");
    }
    Ok(())
}

fn parse_cli_time_filter(
    name: &str,
    value: Option<&str>,
) -> anyhow::Result<Option<DateTime<FixedOffset>>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Ok(Some(dt));
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let midnight = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow::anyhow!("invalid --{name}: {value}"))?;
        let utc = FixedOffset::east_opt(0)
            .ok_or_else(|| anyhow::anyhow!("failed to construct UTC offset"))?;
        return Ok(Some(utc.from_utc_datetime(&midnight)));
    }

    anyhow::bail!("invalid --{name}: expected RFC3339 timestamp or YYYY-MM-DD");
}

fn format_time_filter(dt: DateTime<FixedOffset>) -> String {
    dt.with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

async fn run_doctor(_verbose: bool) -> anyhow::Result<()> {
    println!("Checking MAG setup...\n");

    let mut passed = 0u32;
    let mut total = 0u32;

    // 1. Paths check
    total += 1;
    let paths = match app_paths::resolve_app_paths() {
        Ok(p) => {
            let writable = is_dir_writable(&p.data_root);
            if writable {
                println!("[ok] Paths: {} (writable)", p.data_root.display());
                passed += 1;
            } else if p.data_root.exists() {
                println!("[FAIL] Paths: {} (not writable)", p.data_root.display());
                println!("       Ensure the directory has write permissions.");
            } else {
                println!(
                    "[ok] Paths: {} (will be created on first use)",
                    p.data_root.display()
                );
                passed += 1;
            }
            Some(p)
        }
        Err(e) => {
            println!("[FAIL] Paths: {e}");
            println!("       Ensure HOME or USERPROFILE is set.");
            None
        }
    };

    // 2. Database check
    total += 1;
    if let Some(ref paths) = paths {
        if paths.database_path.exists() {
            match check_db_integrity(&paths.database_path) {
                Ok((ok, count)) => {
                    if ok {
                        println!(
                            "[ok] Database: {} (valid, {count} memories)",
                            paths.database_path.display()
                        );
                        passed += 1;
                    } else {
                        println!(
                            "[FAIL] Database: {} (integrity check failed)",
                            paths.database_path.display()
                        );
                        println!("       Run: mag maintain --action health");
                    }
                }
                Err(e) => {
                    println!("[FAIL] Database: {} ({e})", paths.database_path.display());
                    println!("       Run: mag maintain --action health");
                }
            }
        } else {
            println!(
                "[!!] Database: not found at {}",
                paths.database_path.display()
            );
            println!("       Database will be created on first use.");
            passed += 1; // warning, not failure
        }
    } else {
        println!("[FAIL] Database: cannot check (paths unavailable)");
    }

    // 3. Models check
    total += 1;
    if let Some(ref paths) = paths {
        let model_onnx = paths.model_root.join("model.onnx");
        let tokenizer = paths.model_root.join("tokenizer.json");
        if model_onnx.exists() && tokenizer.exists() {
            let model_size = std::fs::metadata(&model_onnx).map(|m| m.len()).unwrap_or(0);
            #[allow(clippy::cast_precision_loss)]
            let size_mb = model_size as f64 / (1024.0 * 1024.0);
            println!("[ok] Models: model.onnx ({size_mb:.0} MB), tokenizer.json");
            passed += 1;
        } else {
            let mut missing = Vec::new();
            if !model_onnx.exists() {
                missing.push("model.onnx");
            }
            if !tokenizer.exists() {
                missing.push("tokenizer.json");
            }
            println!(
                "[FAIL] Models: missing {} in {}",
                missing.join(", "),
                paths.model_root.display()
            );
            println!("       Run: mag download-model");
        }
    } else {
        println!("[FAIL] Models: cannot check (paths unavailable)");
    }

    // 4. Embedder warmup check (only if models present)
    total += 1;
    #[cfg(feature = "real-embeddings")]
    {
        let start = std::time::Instant::now();
        match memory_core::OnnxEmbedder::new() {
            Ok(embedder) => {
                let embedder = std::sync::Arc::new(embedder);
                match embedder.warmup().await {
                    Ok(()) => {
                        let elapsed = start.elapsed();
                        println!("[ok] Embedder: warmup OK ({elapsed:.0?})");
                        passed += 1;
                    }
                    Err(e) => {
                        println!("[FAIL] Embedder: warmup failed ({e})");
                        println!("       Run: mag download-model");
                    }
                }
            }
            Err(e) => {
                println!("[FAIL] Embedder: initialization failed ({e})");
                println!("       Run: mag download-model");
            }
        }
    }
    #[cfg(not(feature = "real-embeddings"))]
    {
        println!("[!!] Embedder: real-embeddings feature not enabled (using placeholder)");
        passed += 1; // warning, not failure
    }

    println!(
        "\n{passed}/{total} checks passed.{}",
        if passed == total {
            " MAG is ready."
        } else {
            ""
        }
    );
    Ok(())
}

fn is_dir_writable(path: &std::path::Path) -> bool {
    if !path.exists() {
        return false;
    }
    let probe = path.join(".mag_doctor_probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn check_db_integrity(db_path: &std::path::Path) -> anyhow::Result<(bool, i64)> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap_or_else(|_| "error".to_string());
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
        .unwrap_or(0);
    Ok((integrity == "ok", count))
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
