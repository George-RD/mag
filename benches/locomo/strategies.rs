use std::sync::Arc;

use anyhow::Result;
use mag::memory_core::embedder::Embedder;
use mag::memory_core::storage::sqlite::SqliteStorage;

/// Stable identifier used on the CLI and in CSV/JSON outputs.
/// kebab-case, e.g. "sqlite-v1", "sqlite-v1-no-graph".
pub type StrategyId = &'static str;

#[derive(Debug, Clone)]
pub struct StrategyConfig {
    pub id: StrategyId,
    /// One-line human description printed in --list-strategies output.
    pub description: &'static str,
    /// Override graph_neighbor_factor. None = use storage default.
    pub graph_neighbor_factor: Option<f64>,
    /// Disable graph traversal entirely when true (metadata; actual disabling is
    /// via `graph_neighbor_factor: Some(0.0)`).
    #[allow(dead_code)]
    pub no_graph: bool,
    /// Disable cross-encoder reranking even when the binary supports it.
    pub no_rerank: bool,
    /// Disable entity-tag extraction during seeding.
    pub no_entity_tags: bool,
    /// Override RRF k constant. None = use storage default.
    pub rrf_k: Option<f64>,
}

/// Ordered registry of all known strategies.
pub const STRATEGIES: &[StrategyConfig] = &[
    StrategyConfig {
        id: "sqlite-v1",
        description: "Reference strategy -- production defaults (graph + entity tags, no reranking)",
        graph_neighbor_factor: None,
        no_graph: false,
        no_rerank: false,
        no_entity_tags: false,
        rrf_k: None,
    },
    StrategyConfig {
        id: "sqlite-v1-no-graph",
        description: "Ablation: disables graph traversal to measure its contribution",
        graph_neighbor_factor: Some(0.0),
        no_graph: true,
        no_rerank: false,
        no_entity_tags: false,
        rrf_k: None,
    },
    StrategyConfig {
        id: "sqlite-v1-no-rerank",
        description: "Ablation: disables cross-encoder reranking (for when --cross-encoder is on)",
        graph_neighbor_factor: None,
        no_graph: false,
        no_rerank: true,
        no_entity_tags: false,
        rrf_k: None,
    },
    StrategyConfig {
        id: "sqlite-v1-rrf-tuned",
        description: "Experimental: RRF k=30 (lower k up-weights top results)",
        graph_neighbor_factor: None,
        no_graph: false,
        no_rerank: false,
        no_entity_tags: false,
        rrf_k: Some(30.0),
    },
];

/// Look up a strategy by id. Returns None for unknown names.
pub fn find_strategy(id: &str) -> Option<&'static StrategyConfig> {
    STRATEGIES.iter().find(|s| s.id == id)
}

/// Print all strategies to stdout in human-readable form.
pub fn list_strategies() {
    println!("Available strategies:\n");
    for s in STRATEGIES {
        println!("  {:<26} {}", s.id, s.description);
    }
    println!();
}

/// Construct and configure a `SqliteStorage` instance from a `StrategyConfig`.
///
/// `graph_factor_override` comes from the `--graph-factor` CLI flag and wins
/// over the strategy config value when present.
pub fn build_storage(
    cfg: &StrategyConfig,
    embedder: Arc<dyn Embedder>,
    graph_factor_override: Option<f64>,
) -> Result<SqliteStorage> {
    let mut storage = SqliteStorage::new_in_memory_with_embedder(embedder)?;

    let mut params = storage.scoring_params().clone();

    // Apply strategy-level graph_neighbor_factor (0.0 disables graph traversal).
    if let Some(gf) = cfg.graph_neighbor_factor {
        params.graph_neighbor_factor = gf;
    }

    // CLI --graph-factor wins over strategy config.
    if let Some(gf) = graph_factor_override {
        params.graph_neighbor_factor = gf;
    }

    // Apply RRF k override if the strategy specifies one.
    if let Some(k) = cfg.rrf_k {
        params.rrf_k = k;
    }

    storage.set_scoring_params(params);

    Ok(storage)
}
