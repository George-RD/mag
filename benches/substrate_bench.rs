//! Substrate pipeline performance benchmark.
//!
//! Measures end-to-end search latency of the substrate SearchPipeline
//! at a fixed database size using deterministic synthetic data.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;

use mag::memory_core::storage::sqlite::SqliteStorage;
use mag::memory_core::{Embedder, MemoryInput, PlaceholderEmbedder, ScoringParams, SearchOptions};
use mag::substrate::{
    FullTextSearch, MemoryStore, MultiFactorScorer, QueryContext, RrfFusion,
    SearchPipeline, TtlExpirationPolicy, VectorSearch,
};

#[derive(Debug, Parser)]
#[command(name = "substrate_bench")]
#[command(about = "Substrate search pipeline performance benchmark")]
struct Args {
    /// Number of memories to seed
    #[arg(long, default_value_t = 5_000)]
    memories: usize,
    /// Number of search queries to execute
    #[arg(long, default_value_t = 100)]
    queries: usize,
    /// Result limit per query
    #[arg(long, default_value_t = 10)]
    limit: usize,
}

const TOPICS: &[&str] = &[
    "rust programming", "python scripting", "javascript frontend",
    "database design", "API architecture", "microservices",
    "container orchestration", "CI/CD pipelines", "testing strategies",
    "performance optimization", "security practices", "monitoring",
    "logging infrastructure", "message queues", "caching layers",
    "authentication", "authorization", "error handling",
    "concurrency patterns", "data modeling",
];

const SUBTOPICS: &[&str] = &[
    "best practices", "common pitfalls", "advanced techniques",
    "beginner guide", "case study", "benchmark results",
    "migration strategy", "troubleshooting", "design patterns",
    "performance comparison", "security implications", "integration guide",
];

fn generate_content(index: usize) -> String {
    let topic = TOPICS[index % TOPICS.len()];
    let sub = SUBTOPICS[(index / TOPICS.len()) % SUBTOPICS.len()];
    let detail = index % 1000;
    format!("{} — {} (instance #{})", topic, sub, detail)
}

fn generate_query(index: usize) -> String {
    let topic = TOPICS[index % TOPICS.len()];
    let qualifier = match index % 4 {
        0 => "best practices",
        1 => "guide",
        2 => "optimization",
        _ => "patterns",
    };
    format!("{} {}", topic, qualifier)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let storage = SqliteStorage::new_in_memory()?;
    let embedder = PlaceholderEmbedder;
    // Seed memories
    for i in 0..args.memories {
        let content = generate_content(i);
        storage
            .store(
                &format!("mem-{i}"),
                &content,
                &MemoryInput {
                    content: content.clone(),
                    ..Default::default()
                },
            )
            .await?;
    }
    // Run ANALYZE so SQLite can pick optimal query plans
    storage.analyze()?;
    let store: Arc<dyn MemoryStore> = Arc::new(storage);
    // Build pipeline
    let pipeline = SearchPipeline {
        retrieval: vec![
            Box::new(VectorSearch {
                store: Arc::clone(&store),
            }),
            Box::new(FullTextSearch {
                store: Arc::clone(&store),
            }),
        ],
        fusion: Box::new(RrfFusion),
        scorers: vec![Box::new(MultiFactorScorer)],
        lifecycle: Some(Box::new(TtlExpirationPolicy)),
        abstention_min_text: 0.15,
    };

    let scoring_params = ScoringParams::default();

    // Warmup
    for i in 0..5.min(args.queries) {
        let query = generate_query(i);
        let ctx = QueryContext {
            query: query.clone(),
            limit: args.limit,
            opts: SearchOptions::default(),
            scoring_params: scoring_params.clone(),
            query_embedding: embedder.embed(&query).ok(),
            query_tokens: None,
            include_superseded: false,
        };
        let _ = pipeline.search(ctx).await?;
    }

    // Benchmark
    let mut latencies: Vec<Duration> = Vec::with_capacity(args.queries);
    for i in 0..args.queries {
        let query = generate_query(i);
        let ctx = QueryContext {
            query: query.clone(),
            limit: args.limit,
            opts: SearchOptions::default(),
            scoring_params: scoring_params.clone(),
            query_embedding: embedder.embed(&query).ok(),
            query_tokens: None,
            include_superseded: false,
        };

        let start = Instant::now();
        let _ = pipeline.search(ctx).await?;
        latencies.push(start.elapsed());
    }

    // Compute statistics
    latencies.sort();
    let p50 = latencies[latencies.len() / 2];
    let p99 = latencies[latencies.len() * 99 / 100];
    let avg = latencies.iter().sum::<Duration>() / latencies.len() as u32;
    let total_s: f64 = latencies.iter().map(|d| d.as_secs_f64()).sum();
    let qps = args.queries as f64 / total_s;

    println!("METRIC p50_latency_us={}", p50.as_micros());
    println!("METRIC p99_latency_us={}", p99.as_micros());
    println!("METRIC avg_latency_us={}", avg.as_micros());
    println!("METRIC throughput_qps={:.2}", qps);

    Ok(())
}
