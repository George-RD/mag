use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "romega")]
#[command(about = "A modular memory system", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Ingest {
        content: String,
    },
    Process {
        content: String,
    },
    Retrieve {
        id: String,
    },
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
