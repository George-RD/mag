use clap::{Parser, Subcommand};

/// The main CLI entry point for the romega memory system.
#[derive(Parser)]
#[command(name = "romega")]
#[command(about = "A modular memory system", long_about = None)]
pub struct Cli {
    /// The subcommand to execute.
    #[command(subcommand)]
    pub command: Commands,
}

/// Available subcommands for the memory pipeline.
#[derive(Subcommand)]
pub enum Commands {
    /// Ingests raw content into the system.
    Ingest { content: String },
    /// Processes content through the pipeline (alias for run in current model).
    Process { content: String },
    /// Retrieves a stored memory by its ID.
    Retrieve { id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_ingest_command() {
        let args = vec!["romega", "ingest", "test content"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Ingest { content } => assert_eq!(content, "test content"),
            _ => panic!("Expected Ingest command"),
        }
    }

    #[test]
    fn test_cli_process_command() {
        let args = vec!["romega", "process", "raw data"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Process { content } => assert_eq!(content, "raw data"),
            _ => panic!("Expected Process command"),
        }
    }

    #[test]
    fn test_cli_retrieve_command() {
        let args = vec!["romega", "retrieve", "123"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Retrieve { id } => assert_eq!(id, "123"),
            _ => panic!("Expected Retrieve command"),
        }
    }
}
