use clap::{Parser, Subcommand};

/// CLI representation of the storage initialization mode.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitModeArg {
    /// Standard initialization with default database path.
    Default,
    /// Reserved for future advanced configuration.
    Advanced,
}

/// The main CLI entry point for the romega memory system.
#[derive(Parser)]
#[command(name = "romega")]
#[command(about = "A modular memory system", long_about = None)]
pub struct Cli {
    #[arg(long, value_enum, default_value_t = InitModeArg::Default, global = true)]
    pub init_mode: InitModeArg,

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
    /// Starts the MCP server over stdio transport.
    Serve,
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

    #[test]
    fn test_cli_default_init_mode() {
        let args = vec!["romega", "ingest", "test content"];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.init_mode, InitModeArg::Default);
    }

    #[test]
    fn test_cli_advanced_init_mode() {
        let args = vec![
            "romega",
            "--init-mode",
            "advanced",
            "ingest",
            "test content",
        ];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.init_mode, InitModeArg::Advanced);
    }

    #[test]
    fn test_cli_serve_command() {
        let args = vec!["romega", "serve"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Serve => {}
            _ => panic!("Expected Serve command"),
        }
    }
}
