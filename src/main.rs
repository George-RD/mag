use clap::Parser;
use cli::{Cli, Commands};
use memory_core::{Pipeline, PlaceholderPipeline};

mod cli;
mod memory_core;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // Construct the pipeline with placeholder modules for now
    let pipeline = Pipeline::new(
        Box::new(PlaceholderPipeline),
        Box::new(PlaceholderPipeline),
        Box::new(PlaceholderPipeline),
        Box::new(PlaceholderPipeline),
    );

    match &cli.command {
        Commands::Ingest { content } => {
            println!("Ingesting content: {}", content);
            let id = pipeline.run(content, None).await?;
            println!("Successfully processed and stored with ID: {}", id);
        }
        Commands::Process { content } => {
            // In the current simple model, process is part of the run pipeline.
            // This command might just test the processing step individually later,
            // but for now we can treat it similarly or just log it.
            println!("Processing content directly: {}", content);
            // Example: Just run the pipeline for now as "process" implies the whole flow in this context
            let id = pipeline.run(content, None).await?;
            println!("Result ID: {}", id);
        }
        Commands::Retrieve { id } => {
            println!("Retrieving ID: {}", id);
            let result = pipeline.retrieve(id).await?;
            println!("Retrieved content: {}", result);
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
