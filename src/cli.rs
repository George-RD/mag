use clap::{Parser, Subcommand};

/// CLI representation of the storage initialization mode.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitModeArg {
    /// Standard initialization with default database path.
    Default,
    /// Reserved for future advanced configuration.
    Advanced,
}

/// CLI representation of allowed feedback ratings.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedbackRating {
    Helpful,
    Unhelpful,
    Outdated,
}

impl FeedbackRating {
    /// Returns the string representation expected by the storage layer.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Helpful => "helpful",
            Self::Unhelpful => "unhelpful",
            Self::Outdated => "outdated",
        }
    }
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
        #[arg(long, value_parser = clap::value_parser!(i64).range(0..))]
        ttl_seconds: Option<i64>,
        /// ISO 8601 timestamp for when the event actually occurred.
        #[arg(long)]
        referenced_date: Option<String>,
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
        #[arg(long, value_parser = clap::value_parser!(i64).range(0..))]
        ttl_seconds: Option<i64>,
        /// ISO 8601 timestamp for when the event actually occurred.
        #[arg(long)]
        referenced_date: Option<String>,
    },
    /// Retrieves a stored memory by its ID.
    Retrieve {
        id: String,
    },
    /// Deletes a stored memory by its ID.
    Delete {
        id: String,
    },
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
        #[arg(long)]
        include_superseded: bool,
    },
    /// Shows relationships for a given memory.
    Relations {
        id: String,
    },
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
        #[arg(long)]
        include_superseded: bool,
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
        #[arg(long)]
        include_superseded: bool,
    },
    /// Advanced multi-phase search with scoring.
    AdvancedSearch {
        query: String,
        #[arg(long, default_value = "10")]
        limit: usize,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        include_superseded: bool,
    },
    /// Get the version history chain for a memory.
    VersionChain {
        /// Memory ID to get chain for.
        id: String,
    },
    /// Find similar memories by embedding.
    Similar {
        id: String,
        #[arg(long, default_value = "5")]
        limit: usize,
    },
    /// Traverse relationship graph from a memory.
    Traverse {
        id: String,
        #[arg(long, default_value = "2")]
        max_hops: usize,
        #[arg(long, default_value = "0.0")]
        min_weight: f64,
    },
    /// Search for exact phrase matches.
    PhraseSearch {
        phrase: String,
        #[arg(long, default_value = "10")]
        limit: usize,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        include_superseded: bool,
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
        #[arg(long)]
        include_superseded: bool,
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
    Feedback {
        memory_id: String,
        rating: FeedbackRating,
        #[arg(long)]
        reason: Option<String>,
    },
    Sweep,
    Profile {
        action: String,
        data: Option<String>,
    },
    Checkpoint {
        task_title: String,
        progress: String,
        #[arg(long)]
        plan: Option<String>,
        #[arg(long)]
        next_steps: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    ResumeTask {
        #[arg(long)]
        task_title: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value_t = 1)]
        limit: usize,
    },
    Remind {
        action: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        duration: Option<String>,
        #[arg(long)]
        context: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        reminder_id: Option<String>,
    },
    Lessons {
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// System maintenance: health check, consolidate, compact, clear-session.
    Maintain {
        #[arg(long)]
        action: String,
        #[arg(long)]
        warn_mb: Option<f64>,
        #[arg(long)]
        critical_mb: Option<f64>,
        #[arg(long)]
        max_nodes: Option<i64>,
        #[arg(long)]
        prune_days: Option<i64>,
        #[arg(long)]
        max_summaries: Option<i64>,
        #[arg(long)]
        event_type: Option<String>,
        #[arg(long)]
        similarity_threshold: Option<f64>,
        #[arg(long)]
        min_cluster_size: Option<usize>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Session startup briefing.
    Welcome {
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// Show available tools and operational guidelines.
    Protocol {
        #[arg(long)]
        section: Option<String>,
    },
    /// Extended statistics (types, sessions, digest, access-rate).
    StatsExtended {
        #[arg(long)]
        action: String,
        #[arg(long, default_value_t = 7)]
        days: i64,
    },
    /// Downloads the ONNX model and tokenizer used for embeddings.
    DownloadModel,
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
    fn test_cli_ingest_with_ttl_seconds() {
        let args = vec!["romega", "ingest", "content", "--ttl-seconds", "123"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Ingest { ttl_seconds, .. } => assert_eq!(ttl_seconds, Some(123)),
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
    fn test_cli_advanced_search_command() {
        let args = vec!["romega", "advanced-search", "query", "--limit", "7"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::AdvancedSearch {
                query,
                limit,
                include_superseded,
                ..
            } => {
                assert_eq!(query, "query");
                assert_eq!(limit, 7);
                assert!(!include_superseded);
            }
            _ => panic!("Expected AdvancedSearch command"),
        }
    }

    #[test]
    fn test_cli_advanced_search_include_superseded_flag() {
        let args = vec!["romega", "advanced-search", "query", "--include-superseded"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::AdvancedSearch {
                include_superseded, ..
            } => assert!(include_superseded),
            _ => panic!("Expected AdvancedSearch command"),
        }
    }

    #[test]
    fn test_cli_version_chain_command() {
        let args = vec!["romega", "version-chain", "abc123"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::VersionChain { id } => assert_eq!(id, "abc123"),
            _ => panic!("Expected VersionChain command"),
        }
    }

    #[test]
    fn test_cli_similar_command() {
        let args = vec!["romega", "similar", "mem-1", "--limit", "3"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Similar { id, limit } => {
                assert_eq!(id, "mem-1");
                assert_eq!(limit, 3);
            }
            _ => panic!("Expected Similar command"),
        }
    }

    #[test]
    fn test_cli_traverse_command() {
        let args = vec![
            "romega",
            "traverse",
            "mem-2",
            "--max-hops",
            "3",
            "--min-weight",
            "0.4",
        ];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Traverse {
                id,
                max_hops,
                min_weight,
            } => {
                assert_eq!(id, "mem-2");
                assert_eq!(max_hops, 3);
                assert!((min_weight - 0.4).abs() < f64::EPSILON);
            }
            _ => panic!("Expected Traverse command"),
        }
    }

    #[test]
    fn test_cli_phrase_search_command() {
        let args = vec!["romega", "phrase-search", "exact phrase", "--limit", "4"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::PhraseSearch { phrase, limit, .. } => {
                assert_eq!(phrase, "exact phrase");
                assert_eq!(limit, 4);
            }
            _ => panic!("Expected PhraseSearch command"),
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
    fn test_cli_feedback_command() {
        let args = vec![
            "romega", "feedback", "mem-1", "helpful", "--reason", "clear",
        ];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Feedback {
                memory_id,
                rating,
                reason,
            } => {
                assert_eq!(memory_id, "mem-1");
                assert_eq!(rating, FeedbackRating::Helpful);
                assert_eq!(reason.as_deref(), Some("clear"));
            }
            _ => panic!("Expected Feedback command"),
        }
    }

    #[test]
    fn test_cli_sweep_command() {
        let args = vec!["romega", "sweep"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Sweep => {}
            _ => panic!("Expected Sweep command"),
        }
    }

    #[test]
    fn test_cli_profile_command() {
        let args = vec!["romega", "profile", "read"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Profile { action, data } => {
                assert_eq!(action, "read");
                assert!(data.is_none());
            }
            _ => panic!("Expected Profile command"),
        }
    }

    #[test]
    fn test_cli_checkpoint_command() {
        let args = vec![
            "romega",
            "checkpoint",
            "Task",
            "Done",
            "--project",
            "romega",
        ];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Checkpoint {
                task_title,
                progress,
                project,
                ..
            } => {
                assert_eq!(task_title, "Task");
                assert_eq!(progress, "Done");
                assert_eq!(project.as_deref(), Some("romega"));
            }
            _ => panic!("Expected Checkpoint command"),
        }
    }

    #[test]
    fn test_cli_resume_task_command() {
        let args = vec![
            "romega",
            "resume-task",
            "--task-title",
            "Task",
            "--limit",
            "3",
        ];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::ResumeTask {
                task_title, limit, ..
            } => {
                assert_eq!(task_title.as_deref(), Some("Task"));
                assert_eq!(limit, 3);
            }
            _ => panic!("Expected ResumeTask command"),
        }
    }

    #[test]
    fn test_cli_remind_command() {
        let args = vec![
            "romega",
            "remind",
            "set",
            "--text",
            "review",
            "--duration",
            "1h",
        ];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Remind {
                action,
                text,
                duration,
                ..
            } => {
                assert_eq!(action, "set");
                assert_eq!(text.as_deref(), Some("review"));
                assert_eq!(duration.as_deref(), Some("1h"));
            }
            _ => panic!("Expected Remind command"),
        }
    }

    #[test]
    fn test_cli_lessons_command() {
        let args = vec!["romega", "lessons", "--task", "search", "--limit", "7"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Lessons { task, limit, .. } => {
                assert_eq!(task.as_deref(), Some("search"));
                assert_eq!(limit, 7);
            }
            _ => panic!("Expected Lessons command"),
        }
    }

    #[test]
    fn test_cli_download_model_command() {
        let args = vec!["romega", "download-model"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::DownloadModel => {}
            _ => panic!("Expected DownloadModel command"),
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

    #[test]
    fn test_cli_maintain_command() {
        let args = vec![
            "romega",
            "maintain",
            "--action",
            "health",
            "--warn-mb",
            "100",
        ];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Maintain {
                action, warn_mb, ..
            } => {
                assert_eq!(action, "health");
                assert_eq!(warn_mb, Some(100.0));
            }
            _ => panic!("Expected Maintain command"),
        }
    }

    #[test]
    fn test_cli_welcome_command() {
        let args = vec![
            "romega",
            "welcome",
            "--session-id",
            "s1",
            "--project",
            "proj",
        ];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Welcome {
                session_id,
                project,
            } => {
                assert_eq!(session_id.as_deref(), Some("s1"));
                assert_eq!(project.as_deref(), Some("proj"));
            }
            _ => panic!("Expected Welcome command"),
        }
    }

    #[test]
    fn test_cli_protocol_command() {
        let args = vec!["romega", "protocol"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Protocol { section } => {
                assert!(section.is_none());
            }
            _ => panic!("Expected Protocol command"),
        }
    }

    #[test]
    fn test_cli_stats_extended_command() {
        let args = vec!["romega", "stats-extended", "--action", "types"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::StatsExtended { action, days } => {
                assert_eq!(action, "types");
                assert_eq!(days, 7);
            }
            _ => panic!("Expected StatsExtended command"),
        }
    }
}
