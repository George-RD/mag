use clap::Parser;
use cli::{Cli, Commands, InitModeArg};
use memory_core::storage::{InitMode, SqliteStorage};
use memory_core::{
    Deleter, Embedder, Lister, MemoryInput, MemoryUpdate, Pipeline, PlaceholderPipeline,
    RelationshipQuerier, SearchOptions, Updater, default_priority_for_event_type,
    is_valid_event_type,
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
    let embedder = build_embedder()?;
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
        Commands::Recent {
            limit,
            event_type,
            project,
            session_id,
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

fn build_embedder() -> anyhow::Result<Arc<dyn Embedder>> {
    #[cfg(feature = "real-embeddings")]
    {
        let onnx = memory_core::OnnxEmbedder::new()?;
        Ok(Arc::new(onnx))
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
