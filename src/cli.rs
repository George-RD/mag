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
    Ingest {
        content: String,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long, default_value_t = 0.5)]
        importance: f64,
        #[arg(long)]
        metadata: Option<String>,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        priority: Option<i32>,
        #[arg(long)]
        entity_id: Option<String>,
        #[arg(long)]
        agent_type: Option<String>,
    },
    /// Processes content through the pipeline (alias for run in current model).
    Process {
        content: String,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long, default_value_t = 0.5)]
        importance: f64,
        #[arg(long)]
        metadata: Option<String>,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        priority: Option<i32>,
        #[arg(long)]
        entity_id: Option<String>,
        #[arg(long)]
        agent_type: Option<String>,
    },
    /// Retrieves a stored memory by its ID.
    Retrieve { id: String },
    /// Deletes a stored memory by its ID.
    Delete { id: String },
    /// Updates an existing memory's content and/or tags.
    Update {
        id: String,
        #[arg(long)]
        content: Option<String>,
        #[arg(long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        #[arg(long)]
        importance: Option<f64>,
        #[arg(long)]
        metadata: Option<String>,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        priority: Option<i32>,
    },
    /// Lists stored memories with pagination.
    List {
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Shows relationships for a given memory.
    Relations { id: String },
    /// Searches stored memories by query string.
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Performs semantic search over stored memories.
    SemanticSearch {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Lists recently accessed memories.
    Recent {
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Shows memory store statistics.
    Stats,
    /// Exports all memories and relationships as JSON.
    Export,
    /// Imports memories and relationships from JSON.
    Import {
        /// Path to the JSON file to import, or "-" for stdin.
        path: String,
    },
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
            Commands::Ingest {
                content,
                tags,
                importance,
                metadata,
                ..
            } => {
                assert_eq!(content, "test content");
                assert!(tags.is_empty());
                assert_eq!(importance, 0.5);
                assert!(metadata.is_none());
            }
            _ => panic!("Expected Ingest command"),
        }
    }

    #[test]
    fn test_cli_process_command() {
        let args = vec!["romega", "process", "raw data"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Process {
                content,
                tags,
                importance,
                metadata,
                ..
            } => {
                assert_eq!(content, "raw data");
                assert!(tags.is_empty());
                assert_eq!(importance, 0.5);
                assert!(metadata.is_none());
            }
            _ => panic!("Expected Process command"),
        }
    }

    #[test]
    fn test_cli_delete_command() {
        let args = vec!["romega", "delete", "abc123"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Delete { id } => assert_eq!(id, "abc123"),
            _ => panic!("Expected Delete command"),
        }
    }

    #[test]
    fn test_cli_update_command() {
        let args = vec!["romega", "update", "abc123", "--content", "new content"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Update {
                id,
                content,
                tags,
                importance,
                metadata,
                ..
            } => {
                assert_eq!(id, "abc123");
                assert_eq!(content, Some("new content".to_string()));
                assert!(tags.is_none());
                assert!(importance.is_none());
                assert!(metadata.is_none());
            }
            _ => panic!("Expected Update command"),
        }
    }

    #[test]
    fn test_cli_update_command_with_tags() {
        let args = vec![
            "romega",
            "update",
            "abc123",
            "--content",
            "new content",
            "--tags",
            "a,b",
        ];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Update {
                id,
                content,
                tags,
                importance,
                metadata,
                ..
            } => {
                assert_eq!(id, "abc123");
                assert_eq!(content, Some("new content".to_string()));
                assert_eq!(tags, Some(vec!["a".to_string(), "b".to_string()]));
                assert!(importance.is_none());
                assert!(metadata.is_none());
            }
            _ => panic!("Expected Update command"),
        }
    }

    #[test]
    fn test_cli_update_tags_only() {
        let args = vec!["romega", "update", "abc123", "--tags", "x,y"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Update {
                id,
                content,
                tags,
                importance,
                metadata,
                ..
            } => {
                assert_eq!(id, "abc123");
                assert!(content.is_none());
                assert_eq!(tags, Some(vec!["x".to_string(), "y".to_string()]));
                assert!(importance.is_none());
                assert!(metadata.is_none());
            }
            _ => panic!("Expected Update command"),
        }
    }

    #[test]
    fn test_cli_list_command() {
        let args = vec!["romega", "list", "--offset", "5", "--limit", "3"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::List { offset, limit, .. } => {
                assert_eq!(offset, 5);
                assert_eq!(limit, 3);
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_cli_relations_command() {
        let args = vec!["romega", "relations", "abc123"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Relations { id } => assert_eq!(id, "abc123"),
            _ => panic!("Expected Relations command"),
        }
    }

    #[test]
    fn test_cli_ingest_with_tags() {
        let args = vec!["romega", "ingest", "content", "--tags", "x,y"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Ingest {
                content,
                tags,
                importance,
                metadata,
                ..
            } => {
                assert_eq!(content, "content");
                assert_eq!(tags, vec!["x".to_string(), "y".to_string()]);
                assert_eq!(importance, 0.5);
                assert!(metadata.is_none());
            }
            _ => panic!("Expected Ingest command"),
        }
    }

    #[test]
    fn test_cli_ingest_with_importance() {
        let args = vec!["romega", "ingest", "content", "--importance", "0.9"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Ingest { importance, .. } => assert_eq!(importance, 0.9),
            _ => panic!("Expected Ingest command"),
        }
    }

    #[test]
    fn test_cli_ingest_with_metadata() {
        let args = vec![
            "romega",
            "ingest",
            "content",
            "--metadata",
            "{\"key\":\"val\"}",
        ];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Ingest { metadata, .. } => {
                assert_eq!(metadata, Some("{\"key\":\"val\"}".to_string()))
            }
            _ => panic!("Expected Ingest command"),
        }
    }

    #[test]
    fn test_cli_update_with_importance() {
        let args = vec!["romega", "update", "abc123", "--importance", "0.8"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Update { importance, .. } => assert_eq!(importance, Some(0.8)),
            _ => panic!("Expected Update command"),
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
    fn test_cli_search_command() {
        let args = vec!["romega", "search", "hello", "--limit", "3"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Search { query, limit, .. } => {
                assert_eq!(query, "hello");
                assert_eq!(limit, 3);
            }
            _ => panic!("Expected Search command"),
        }
    }

    #[test]
    fn test_cli_semantic_search_command() {
        let args = vec!["romega", "semantic-search", "context", "--limit", "2"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::SemanticSearch { query, limit, .. } => {
                assert_eq!(query, "context");
                assert_eq!(limit, 2);
            }
            _ => panic!("Expected SemanticSearch command"),
        }
    }

    #[test]
    fn test_cli_recent_command() {
        let args = vec!["romega", "recent", "--limit", "4"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Recent { limit, .. } => assert_eq!(limit, 4),
            _ => panic!("Expected Recent command"),
        }
    }

    #[test]
    fn test_cli_stats_command() {
        let args = vec!["romega", "stats"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Stats => {}
            _ => panic!("Expected Stats command"),
        }
    }

    #[test]
    fn test_cli_export_command() {
        let args = vec!["romega", "export"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Export => {}
            _ => panic!("Expected Export command"),
        }
    }

    #[test]
    fn test_cli_import_command() {
        let args = vec!["romega", "import", "memories.json"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Import { path } => assert_eq!(path, "memories.json"),
            _ => panic!("Expected Import command"),
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
