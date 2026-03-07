//! Scale degradation benchmark for romega-memory.
//!
//! Measures store throughput, search latency, and recall quality at increasing
//! database sizes: 1K, 5K, 10K, and 50K memories.
//!
//! Run with:
//!   cargo run --release --bin scale_bench --features real-embeddings
//!   cargo run --release --bin scale_bench --features real-embeddings -- --max-scale 10000
//!   cargo run --release --bin scale_bench --features real-embeddings -- --search-queries 50

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use clap::Parser;

use romega_memory::memory_core::storage::sqlite::SqliteStorage;
use romega_memory::memory_core::{AdvancedSearcher, MemoryInput, OnnxEmbedder, SearchOptions};

// ── CLI ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Parser)]
#[command(name = "scale_bench")]
#[command(about = "Scale degradation benchmark for romega-memory")]
struct Args {
    /// Maximum scale level to test (1000, 5000, 10000, or 50000)
    #[arg(long, default_value_t = 50_000)]
    max_scale: usize,

    /// Number of search queries to run at each scale level
    #[arg(long, default_value_t = 30)]
    search_queries: usize,
}

// ── Scale levels ─────────────────────────────────────────────────────────────

const SCALE_LEVELS: &[usize] = &[1_000, 5_000, 10_000, 50_000];

// ── Content generation ───────────────────────────────────────────────────────

const TOPICS: &[&str] = &[
    "programming",
    "debugging",
    "architecture",
    "testing",
    "deployment",
    "databases",
    "networking",
    "security",
    "performance",
    "concurrency",
    "API design",
    "error handling",
    "logging",
    "monitoring",
    "CI/CD pipelines",
    "containerization",
    "microservices",
    "caching",
    "message queues",
    "authentication",
];

const DETAILS: &[&str] = &[
    "discovered that connection pooling reduces latency by 40% in high-throughput scenarios",
    "the retry logic should use exponential backoff with jitter to avoid thundering herd",
    "migrated from REST to gRPC for internal service communication, saw 3x throughput improvement",
    "implemented circuit breaker pattern to handle cascading failures gracefully",
    "using feature flags for gradual rollouts reduced production incidents by 60%",
    "switched to structured logging with correlation IDs for better distributed tracing",
    "found that N+1 query problem was the root cause of the slow dashboard page",
    "added request rate limiting at the API gateway level to prevent abuse",
    "implemented blue-green deployment to achieve zero-downtime releases",
    "using content-addressable storage for deduplication saved 35% disk space",
    "the deadlock was caused by inconsistent lock ordering across two services",
    "implemented saga pattern for distributed transactions across microservices",
    "added health check endpoints with readiness and liveness probes for Kubernetes",
    "using write-ahead logging improved SQLite write throughput significantly",
    "implemented optimistic concurrency control with version vectors",
    "the memory leak was traced to unbounded channel buffers in the event processor",
    "added request coalescing to reduce redundant upstream API calls",
    "using batch inserts instead of individual INSERTs improved ingestion by 10x",
    "implemented graceful shutdown with drain timeout for in-flight requests",
    "added index on (project, created_at) to speed up filtered temporal queries",
    "the race condition was fixed by using compare-and-swap instead of lock-then-update",
    "implemented read replicas for scaling read-heavy workloads horizontally",
    "using bloom filters for membership testing reduced unnecessary disk lookups",
    "added automatic schema migration with version tracking and rollback support",
    "implemented token bucket algorithm for fine-grained API rate limiting",
    "the slow query was optimized by adding a covering index to avoid table lookups",
    "using immutable data structures eliminated an entire class of concurrency bugs",
    "implemented event sourcing for audit trail and temporal query capabilities",
    "added distributed caching layer with consistent hashing for cache key routing",
    "the timeout issue was resolved by setting per-request deadlines instead of global timeouts",
];

const EVENT_TYPES: &[&str] = &[
    "lesson_learned",
    "decision",
    "error_pattern",
    "task_completion",
    "user_preference",
    "session_summary",
];

const TAGS_POOL: &[&str] = &[
    "rust",
    "python",
    "backend",
    "frontend",
    "database",
    "infrastructure",
    "performance",
    "reliability",
    "security",
    "observability",
    "refactoring",
    "optimization",
    "debugging",
    "design-pattern",
    "best-practice",
];

/// Generate a synthetic memory content string for a given index.
fn generate_content(index: usize) -> String {
    let topic = TOPICS[index % TOPICS.len()];
    let detail = DETAILS[index % DETAILS.len()];
    // Mix in the index to create unique content and avoid dedup
    format!(
        "Learned about {topic} (item {index}): {detail}. \
         This insight was gained while working on project-{proj} during sprint {sprint}.",
        proj = index % 7,
        sprint = index % 20,
    )
}

/// Generate a MemoryInput for a given index.
fn generate_input(index: usize) -> MemoryInput {
    let event_type = EVENT_TYPES[index % EVENT_TYPES.len()];
    let tag1 = TAGS_POOL[index % TAGS_POOL.len()];
    let tag2 = TAGS_POOL[(index * 7 + 3) % TAGS_POOL.len()];
    MemoryInput {
        content: String::new(),
        id: None,
        tags: vec![tag1.to_string(), tag2.to_string()],
        importance: 0.3 + (index % 7) as f64 * 0.1, // 0.3 to 0.9
        metadata: serde_json::json!({}),
        event_type: Some(event_type.to_string()),
        session_id: Some(format!("scale-session-{}", index % 50)),
        project: Some(format!("project-{}", index % 7)),
        priority: None,
        entity_id: None,
        agent_type: None,
        ttl_seconds: None,
    }
}

// ── Needle memories for recall testing ───────────────────────────────────────

struct Needle {
    id: String,
    content: String,
    query: String,
}

/// Create a set of needle memories with distinctive content that should be
/// findable via semantic search. Each needle has a unique query designed to
/// match it.
fn create_needles() -> Vec<Needle> {
    vec![
        Needle {
            id: "needle-db-conn".to_string(),
            content: "When handling database connections in production, always use a connection \
                      pool with a maximum size of 20 and idle timeout of 300 seconds to prevent \
                      resource exhaustion under load."
                .to_string(),
            query: "How should I configure database connection pooling for production?".to_string(),
        },
        Needle {
            id: "needle-deploy-process".to_string(),
            content: "Our deployment process uses a three-stage pipeline: first run the full \
                      test suite in CI, then deploy to staging with canary analysis for 15 \
                      minutes, and finally promote to production with automatic rollback on \
                      error rate spike."
                .to_string(),
            query: "What is the deployment process we follow?".to_string(),
        },
        Needle {
            id: "needle-auth-tokens".to_string(),
            content: "Authentication tokens should be JWT with RS256 signing, 15-minute access \
                      token expiry, and 7-day refresh token rotation. Store refresh tokens in \
                      HttpOnly secure cookies, never in localStorage."
                .to_string(),
            query: "How should authentication tokens be configured?".to_string(),
        },
        Needle {
            id: "needle-error-handling".to_string(),
            content: "For error handling in our Rust services, use anyhow for application errors \
                      and thiserror for library errors. Always attach context with .context() \
                      and never use unwrap() in production code paths."
                .to_string(),
            query: "What are the error handling conventions for Rust services?".to_string(),
        },
        Needle {
            id: "needle-cache-strategy".to_string(),
            content: "The caching strategy for the product catalog uses a two-level cache: L1 \
                      is an in-process LRU cache with 1000 entries and 60-second TTL, L2 is \
                      Redis with 5-minute TTL and cache-aside pattern for invalidation."
                .to_string(),
            query: "What caching strategy do we use for the product catalog?".to_string(),
        },
    ]
}

// ── Search queries for latency testing ───────────────────────────────────────

const SEARCH_QUERIES: &[&str] = &[
    "How do I handle database connections?",
    "What was the deployment process?",
    "How to fix the memory leak?",
    "What is the retry strategy?",
    "How to implement rate limiting?",
    "What architecture pattern should I use for microservices?",
    "How to configure logging and monitoring?",
    "What are the best practices for error handling?",
    "How to optimize query performance?",
    "What is our caching strategy?",
    "How to handle concurrent requests safely?",
    "What security measures should I implement?",
    "How to set up CI/CD pipelines?",
    "What testing strategies should I follow?",
    "How to handle graceful shutdown?",
    "What is the schema migration process?",
    "How to implement distributed tracing?",
    "What container orchestration approach do we use?",
    "How to implement feature flags?",
    "What message queue should I use for async processing?",
    "How to handle API versioning?",
    "What database indexing strategy should I follow?",
    "How to implement circuit breakers?",
    "What load balancing approach do we use?",
    "How to set up health checks for services?",
    "What is the data backup and recovery plan?",
    "How to handle configuration management?",
    "What observability stack do we use?",
    "How to implement blue-green deployments?",
    "What are the code review guidelines?",
];

// ── Latency statistics ───────────────────────────────────────────────────────

struct LatencyStats {
    count: usize,
    mean_us: f64,
    p50_us: f64,
    p95_us: f64,
    p99_us: f64,
}

fn compute_latency_stats(durations: &[Duration]) -> LatencyStats {
    if durations.is_empty() {
        return LatencyStats {
            count: 0,
            mean_us: 0.0,
            p50_us: 0.0,
            p95_us: 0.0,
            p99_us: 0.0,
        };
    }

    let mut micros: Vec<f64> = durations
        .iter()
        .map(|d| d.as_secs_f64() * 1_000_000.0)
        .collect();
    micros.sort_by(|a, b| a.total_cmp(b));

    let count = micros.len();
    let mean_us = micros.iter().sum::<f64>() / count as f64;
    let last = count - 1;
    let p50_us = micros[((last as f64) * 0.50).round() as usize];
    let p95_us = micros[((last as f64) * 0.95).round() as usize];
    let p99_us = micros[((last as f64) * 0.99).round() as usize];

    LatencyStats {
        count,
        mean_us,
        p50_us,
        p95_us,
        p99_us,
    }
}

fn format_us(us: f64) -> String {
    if us >= 1_000_000.0 {
        format!("{:.2}s", us / 1_000_000.0)
    } else if us >= 1_000.0 {
        format!("{:.2}ms", us / 1_000.0)
    } else {
        format!("{:.0}us", us)
    }
}

// ── Scale level result ───────────────────────────────────────────────────────

struct ScaleResult {
    level: usize,
    store_throughput: f64,      // memories/sec for this batch
    avg_store_latency_us: f64,  // average per-memory store latency
    search_stats: LatencyStats, // search latency statistics
    recall_at_5: f64,           // fraction of needles found in top-5
    needles_found: usize,
    needles_total: usize,
}

// ── Main benchmark logic ─────────────────────────────────────────────────────

fn create_temp_db_path() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("romega-scale-bench");
    std::fs::create_dir_all(&dir)?;
    let filename = format!("scale_bench_{}.db", std::process::id());
    Ok(dir.join(filename))
}

async fn run_benchmark(args: &Args) -> Result<Vec<ScaleResult>> {
    // Determine which scale levels to run
    let levels: Vec<usize> = SCALE_LEVELS
        .iter()
        .copied()
        .filter(|&l| l <= args.max_scale)
        .collect();

    if levels.is_empty() {
        return Err(anyhow!(
            "No scale levels to run (max_scale={} is below minimum level {})",
            args.max_scale,
            SCALE_LEVELS[0]
        ));
    }

    eprintln!("=== Scale Degradation Benchmark ===");
    eprintln!();
    eprintln!("Scale levels: {:?}", levels);
    eprintln!("Search queries per level: {}", args.search_queries);
    eprintln!();

    // Initialize embedder (one-time cost)
    eprintln!("[1/4] Initializing ONNX embedder...");
    let embedder_start = Instant::now();
    let embedder = Arc::new(OnnxEmbedder::new()?);
    eprintln!(
        "      Embedder ready in {:.1}s",
        embedder_start.elapsed().as_secs_f64()
    );
    eprintln!();

    // Create temp database file
    let db_path = create_temp_db_path()?;
    eprintln!("[2/4] Database path: {}", db_path.display());
    let storage = SqliteStorage::new_with_path(db_path.clone(), embedder)?;
    eprintln!();

    // Plant needle memories first (before any bulk content)
    eprintln!("[3/4] Planting needle memories for recall testing...");
    let needles = create_needles();
    for needle in &needles {
        let input = MemoryInput {
            content: String::new(),
            id: None,
            tags: vec!["needle".to_string(), "scale-bench".to_string()],
            importance: 0.8,
            metadata: serde_json::json!({}),
            event_type: Some("lesson_learned".to_string()),
            session_id: Some("needle-session".to_string()),
            project: Some("needle-project".to_string()),
            priority: Some(4),
            entity_id: None,
            agent_type: None,
            ttl_seconds: None,
        };
        storage.store(&needle.id, &needle.content, &input).await?;
    }
    eprintln!("      Planted {} needles", needles.len());
    eprintln!();

    // Run benchmark at each scale level
    eprintln!("[4/4] Running scale tests...");
    eprintln!();

    let mut results = Vec::new();
    let mut memories_stored = needles.len(); // account for needles

    for &level in &levels {
        let memories_to_add = level - (memories_stored - needles.len());
        eprintln!(
            "--- Scale level: {} memories (adding {} new) ---",
            level, memories_to_add
        );

        // Store throughput test
        let store_start = Instant::now();
        let batch_start_idx = memories_stored - needles.len();
        for i in 0..memories_to_add {
            let idx = batch_start_idx + i;
            let content = generate_content(idx);
            let input = generate_input(idx);
            let id = format!("scale-{idx}");
            storage.store(&id, &content, &input).await?;
        }
        let store_elapsed = store_start.elapsed();
        memories_stored += memories_to_add;

        let store_throughput = if store_elapsed.as_secs_f64() > 0.0 {
            memories_to_add as f64 / store_elapsed.as_secs_f64()
        } else {
            0.0
        };
        let avg_store_latency_us = if memories_to_add > 0 {
            store_elapsed.as_secs_f64() * 1_000_000.0 / memories_to_add as f64
        } else {
            0.0
        };

        eprintln!(
            "      Store: {} memories in {:.2}s ({:.1} mem/s, avg {}/mem)",
            memories_to_add,
            store_elapsed.as_secs_f64(),
            store_throughput,
            format_us(avg_store_latency_us),
        );

        // Search latency test
        let opts = SearchOptions::default();
        let num_queries = args.search_queries;
        let mut search_durations = Vec::with_capacity(num_queries);

        for i in 0..num_queries {
            let query = SEARCH_QUERIES[i % SEARCH_QUERIES.len()];
            let t = Instant::now();
            let _results = storage.advanced_search(query, 5, &opts).await?;
            search_durations.push(t.elapsed());
        }

        let search_stats = compute_latency_stats(&search_durations);
        eprintln!(
            "      Search: {} queries  mean={}  p50={}  p95={}  p99={}",
            search_stats.count,
            format_us(search_stats.mean_us),
            format_us(search_stats.p50_us),
            format_us(search_stats.p95_us),
            format_us(search_stats.p99_us),
        );

        // Recall quality test (needle search)
        let mut found = 0usize;
        for needle in &needles {
            let results = storage.advanced_search(&needle.query, 5, &opts).await?;
            let needle_found = results.iter().any(|r| r.id == needle.id);
            if needle_found {
                found += 1;
            }
        }
        let recall_at_5 = found as f64 / needles.len() as f64;
        eprintln!(
            "      Recall@5: {}/{} = {:.1}%",
            found,
            needles.len(),
            recall_at_5 * 100.0,
        );
        eprintln!();

        results.push(ScaleResult {
            level,
            store_throughput,
            avg_store_latency_us,
            search_stats,
            recall_at_5,
            needles_found: found,
            needles_total: needles.len(),
        });
    }

    // Cleanup temp database
    if db_path.exists() {
        let _ = std::fs::remove_file(&db_path);
        // Also try removing WAL and SHM files
        let wal_path = db_path.with_extension("db-wal");
        let shm_path = db_path.with_extension("db-shm");
        let _ = std::fs::remove_file(wal_path);
        let _ = std::fs::remove_file(shm_path);
    }

    Ok(results)
}

// ── Output formatting ────────────────────────────────────────────────────────

fn print_results_table(results: &[ScaleResult]) {
    eprintln!();
    eprintln!("===============================================================================");
    eprintln!("                        SCALE DEGRADATION RESULTS");
    eprintln!("===============================================================================");
    eprintln!();

    // Header
    eprintln!(
        "{:<10} {:>12} {:>12} {:>12} {:>12} {:>12} {:>10}",
        "Scale", "Store", "Avg Store", "Search", "Search", "Search", "Recall"
    );
    eprintln!(
        "{:<10} {:>12} {:>12} {:>12} {:>12} {:>12} {:>10}",
        "", "Throughput", "Latency", "Mean", "P95", "P99", "@5"
    );
    eprintln!("{}", "-".repeat(82));

    for r in results {
        eprintln!(
            "{:<10} {:>10.1}/s {:>12} {:>12} {:>12} {:>12} {:>8.1}%",
            format!("{}K", r.level / 1000),
            r.store_throughput,
            format_us(r.avg_store_latency_us),
            format_us(r.search_stats.mean_us),
            format_us(r.search_stats.p95_us),
            format_us(r.search_stats.p99_us),
            r.recall_at_5 * 100.0,
        );
    }

    eprintln!("{}", "-".repeat(82));
    eprintln!();

    // Degradation analysis
    if results.len() >= 2 {
        let first = &results[0];
        let last = &results[results.len() - 1];

        eprintln!(
            "Degradation Analysis ({0}K -> {1}K):",
            first.level / 1000,
            last.level / 1000
        );
        eprintln!();

        let search_slowdown = if first.search_stats.mean_us > 0.0 {
            last.search_stats.mean_us / first.search_stats.mean_us
        } else {
            0.0
        };
        eprintln!(
            "  Search mean latency: {} -> {} ({:.1}x)",
            format_us(first.search_stats.mean_us),
            format_us(last.search_stats.mean_us),
            search_slowdown,
        );

        let p95_slowdown = if first.search_stats.p95_us > 0.0 {
            last.search_stats.p95_us / first.search_stats.p95_us
        } else {
            0.0
        };
        eprintln!(
            "  Search P95 latency:  {} -> {} ({:.1}x)",
            format_us(first.search_stats.p95_us),
            format_us(last.search_stats.p95_us),
            p95_slowdown,
        );

        let recall_delta = last.recall_at_5 - first.recall_at_5;
        eprintln!(
            "  Recall@5:            {:.1}% -> {:.1}% ({:+.1}pp)",
            first.recall_at_5 * 100.0,
            last.recall_at_5 * 100.0,
            recall_delta * 100.0,
        );
    }

    // Detailed recall breakdown
    eprintln!();
    eprintln!("Recall@5 by scale level:");
    for r in results {
        eprintln!(
            "  {}K: {}/{} needles found ({:.1}%)",
            r.level / 1000,
            r.needles_found,
            r.needles_total,
            r.recall_at_5 * 100.0,
        );
    }

    eprintln!();
}

// ── Entry point ──────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let args = Args::parse();

    let runtime = tokio::runtime::Runtime::new()?;
    let results = runtime.block_on(run_benchmark(&args))?;
    print_results_table(&results);

    Ok(())
}
