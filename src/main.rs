use clap::Parser;
use cli::{Cli, Commands, InitModeArg};
use memory_core::storage::{InitMode, SqliteStorage};
use memory_core::{Pipeline, PlaceholderPipeline};
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
    );

    match &cli.command {
        Commands::Ingest { content } => {
            info!(content_len = content.len(), "Ingesting content");
            let id = pipeline.run(content, None).await?;
            info!(memory_id = %id, "Successfully processed and stored");
        }
        Commands::Process { content } => {
            info!(content_len = content.len(), "Processing content directly");
            let id = pipeline.run(content, None).await?;
            info!(memory_id = %id, "Process command stored result");
        }
        Commands::Retrieve { id } => {
            info!(memory_id = %id, "Retrieving memory");
            let result = pipeline.retrieve(id).await?;
            info!(memory_id = %id, content_len = result.len(), "Retrieved memory");
        }
        Commands::Serve => {
            info!("Starting MCP server over stdio");
            McpMemoryServer::new(mcp_storage).serve_stdio().await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
