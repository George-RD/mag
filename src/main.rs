use clap::Parser;
use cli::{Cli, Commands, InitModeArg};
use memory_core::storage::{InitMode, SqliteStorage};
use memory_core::{Deleter, Lister, Pipeline, PlaceholderPipeline, RelationshipQuerier, Updater};
use serde_json::json;
use tracing::info;

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

    let storage_mode = match cli.init_mode {
        InitModeArg::Default => InitMode::Default,
        InitModeArg::Advanced => InitMode::Advanced,
    };
    let sqlite_storage = SqliteStorage::new(storage_mode)?;
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
        } => {
            info!(content_len = content.len(), "Ingesting content");
            let meta = parse_metadata_arg(metadata.as_deref())?;
            let id = pipeline
                .run(content, None, tags, *importance, &meta)
                .await?;
            info!(memory_id = %id, "Successfully processed and stored");
            println!("{}", json!({ "id": id }));
        }
        Commands::Process {
            content,
            tags,
            importance,
            metadata,
        } => {
            info!(content_len = content.len(), "Processing content directly");
            let meta = parse_metadata_arg(metadata.as_deref())?;
            let id = pipeline
                .run(content, None, tags, *importance, &meta)
                .await?;
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
            if content.is_none() && tags.is_none() && importance.is_none() && meta.is_none() {
                anyhow::bail!(
                    "at least one of --content, --tags, --importance, or --metadata must be provided"
                );
            }
            <SqliteStorage as Updater>::update(
                &mcp_storage,
                id,
                content.as_deref(),
                tags.as_ref().map(|v| v.as_slice()),
                *importance,
                meta.as_ref(),
            )
            .await?;
            info!(memory_id = %id, "Update completed");
            println!("{}", json!({ "id": id, "updated": true }));
        }
        Commands::List { offset, limit } => {
            info!(offset = *offset, limit = *limit, "Listing memories");
            let result = mcp_storage.list(*offset, *limit).await?;
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
                        "metadata": m.metadata
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
                    json!({ "id": r.id, "source_id": r.source_id, "target_id": r.target_id, "rel_type": r.rel_type })
                })
                .collect();
            println!("{}", json!({ "relationships": payload }));
        }
        Commands::Search { query, limit } => {
            info!(
                query_len = query.len(),
                limit = *limit,
                "Searching memories"
            );
            let results = pipeline.search(query, *limit).await?;
            info!(result_count = results.len(), "Search completed");
            let payload: Vec<_> = results
                .into_iter()
                .map(|result| {
                    json!({
                        "id": result.id,
                        "content": result.content,
                        "tags": result.tags,
                        "importance": result.importance,
                        "metadata": result.metadata
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::SemanticSearch { query, limit } => {
            info!(
                query_len = query.len(),
                limit = *limit,
                "Semantic searching memories"
            );
            let results = pipeline.semantic_search(query, *limit).await?;
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
                        "metadata": result.metadata
                    })
                })
                .collect();
            println!("{}", json!({ "results": payload }));
        }
        Commands::Recent { limit } => {
            info!(limit = *limit, "Listing recent memories");
            let results = pipeline.recent(*limit).await?;
            info!(result_count = results.len(), "Recent list completed");
            let payload: Vec<_> = results
                .into_iter()
                .map(|result| {
                    json!({
                        "id": result.id,
                        "content": result.content,
                        "tags": result.tags,
                        "importance": result.importance,
                        "metadata": result.metadata
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
        Commands::Serve => {
            info!("Starting MCP server over stdio");
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

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
