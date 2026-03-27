use std::sync::MutexGuard;
use std::time::Duration;

use super::*;

/// Fallback timestamp used when a row's `created_at` column is missing or unparseable.
pub(super) const EPOCH_FALLBACK: &str = "1970-01-01T00:00:00.000Z";

/// Returns `true` when a rusqlite error indicates SQLite lock contention
/// (`SQLITE_BUSY`). Used to decide whether to retry a transaction.
pub(super) fn is_lock_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _)
            if e.code == rusqlite::ffi::ErrorCode::DatabaseBusy
    )
}

/// Maximum number of retry attempts for lock contention.
const RETRY_MAX_ATTEMPTS: u32 = 5;

/// Base delay between retries (doubled each attempt).
const RETRY_BASE_DELAY_MS: u64 = 10;

/// Retries `f` with exponential backoff when it returns a lock contention error.
///
/// This is a **synchronous** function intended to run inside `spawn_blocking`.
/// Uses `std::thread::sleep` (not tokio) for delays.
///
/// - Max attempts: 5
/// - Backoff: 10ms, 20ms, 40ms, 80ms, 160ms (+ random 0-50% jitter)
/// - Non-lock errors are returned immediately without retry.
pub(super) fn retry_on_lock<T, F>(mut f: F) -> std::result::Result<T, rusqlite::Error>
where
    F: FnMut() -> std::result::Result<T, rusqlite::Error>,
{
    let mut attempt = 0_u32;
    loop {
        match f() {
            Ok(val) => return Ok(val),
            Err(err) if is_lock_error(&err) && attempt + 1 < RETRY_MAX_ATTEMPTS => {
                attempt += 1;
                let base_ms = RETRY_BASE_DELAY_MS * 2_u64.pow(attempt - 1);
                // Simple jitter: add 0-50% of base_ms using a cheap hash of the attempt
                // counter and a timestamp nanos component to avoid pulling in a rand dependency.
                let jitter_ms = {
                    let nanos = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos() as u64)
                        .unwrap_or(0);
                    let seed = nanos.wrapping_mul(u64::from(attempt).wrapping_add(7));
                    seed % (base_ms / 2 + 1)
                };
                let delay = Duration::from_millis(base_ms + jitter_ms);
                tracing::debug!(
                    attempt,
                    delay_ms = delay.as_millis(),
                    "sqlite lock contention, retrying"
                );
                std::thread::sleep(delay);
            }
            Err(err) => {
                if is_lock_error(&err) {
                    tracing::warn!(
                        attempts = RETRY_MAX_ATTEMPTS,
                        "sqlite lock contention persisted after all retries"
                    );
                }
                return Err(err);
            }
        }
    }
}

/// Computes a u64 hash key for the query result cache.
pub(super) fn query_cache_key(query: &str, limit: usize, opts: &super::SearchOptions) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    limit.hash(&mut hasher);
    opts.event_type
        .as_ref()
        .map(|et| et.to_string())
        .hash(&mut hasher);
    opts.project.hash(&mut hasher);
    opts.session_id.hash(&mut hasher);
    opts.include_superseded.hash(&mut hasher);
    opts.importance_min.map(|f| f.to_bits()).hash(&mut hasher);
    opts.created_after.hash(&mut hasher);
    opts.created_before.hash(&mut hasher);
    opts.context_tags.hash(&mut hasher);
    opts.entity_id.hash(&mut hasher);
    opts.agent_type.hash(&mut hasher);
    opts.event_after.hash(&mut hasher);
    opts.event_before.hash(&mut hasher);
    opts.explain.hash(&mut hasher);
    hasher.finish()
}

/// Returns `true` when the query looks like a keyword/identifier lookup
/// that should skip ONNX embedding and vector search (FTS5 only).
///
/// Heuristics:
/// - Contains backtick-wrapped code (`` `...` ``)
/// - Looks like a file path (contains `/` with no spaces)
/// - Is a CamelCase identifier (2+ uppercase letters, no spaces)
/// - Is a snake_case identifier (contains `_`, no spaces, all lowercase)
pub(super) fn is_keyword_query(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Contains backtick-wrapped code
    if trimmed.matches('`').count() >= 2 {
        return true;
    }

    // Looks like a file path (contains `/` with no spaces)
    if !trimmed.contains(' ') && trimmed.contains('/') {
        return true;
    }

    // Is a CamelCase identifier (2+ uppercase letters, no spaces, alphanumeric only)
    if !trimmed.contains(' ') {
        let upper_count = trimmed.chars().filter(|c| c.is_uppercase()).count();
        if upper_count >= 2 && trimmed.chars().all(|c| c.is_alphanumeric()) {
            return true;
        }
    }

    // Is a snake_case identifier (contains `_`, no spaces, all lowercase alphanumeric + `_`)
    if !trimmed.contains(' ')
        && trimmed.contains('_')
        && trimmed
            .chars()
            .all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return true;
    }

    false
}

/// Broad intent categories for incoming queries.
/// Used to adjust scoring multipliers so that, e.g., factual lookups
/// favour FTS/BM25 while conceptual queries lean on vector similarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueryIntent {
    /// Code identifier / file-path lookup — FTS only, skip embedding.
    Keyword,
    /// Factual "who/what/when/where" lookups — precise term matching matters.
    Factual,
    /// Conceptual "why/how/explain" queries — semantic similarity matters.
    Conceptual,
    /// Everything else — balanced scoring.
    General,
}

/// Per-intent multipliers applied on top of the base `ScoringParams`.
///
/// Each field is a multiplicative factor:
///   effective_value = base_value × multiplier
///
/// `top_k_mult` scales the internal candidate oversampling (not the final
/// result limit) so that precision-oriented intents can oversample less
/// while recall-oriented intents pull in more candidates.
#[derive(Debug, Clone, Copy)]
pub(super) struct IntentProfile {
    pub vec_weight_mult: f64,
    pub fts_weight_mult: f64,
    pub word_overlap_mult: f64,
    pub top_k_mult: f64,
    #[allow(dead_code)] // Planned for future use; currently tested for intent profiling.
    pub suggested_limit_mult: f64,
}

impl IntentProfile {
    /// Returns the tuned profile for the given intent.
    pub fn for_intent(intent: QueryIntent) -> Self {
        match intent {
            QueryIntent::Keyword => Self {
                vec_weight_mult: 0.0, // no vector search
                fts_weight_mult: 1.0,
                word_overlap_mult: 1.3,
                top_k_mult: 1.0,
                suggested_limit_mult: 1.0,
            },
            QueryIntent::Factual => Self {
                vec_weight_mult: 1.0,
                fts_weight_mult: 1.1,
                word_overlap_mult: 1.15,
                top_k_mult: 1.0,
                suggested_limit_mult: 1.0,
            },
            QueryIntent::Conceptual => Self {
                vec_weight_mult: 1.5,
                fts_weight_mult: 0.85,
                word_overlap_mult: 0.7,
                top_k_mult: 1.3,
                suggested_limit_mult: 1.3,
            },
            QueryIntent::General => Self {
                vec_weight_mult: 1.0,
                fts_weight_mult: 1.0,
                word_overlap_mult: 1.0,
                top_k_mult: 1.0,
                suggested_limit_mult: 1.0,
            },
        }
    }
}

/// Classify a query into a `QueryIntent` category.
///
/// Uses lightweight heuristics (no model inference):
/// 1. `Keyword` — code identifiers, file paths (delegates to `is_keyword_query`).
/// 2. `Factual` — starts with who/what/when/where/which, or asks for
///    names/numbers/dates. These benefit from exact term matching.
/// 3. `Conceptual` — starts with why/how/explain/describe, or contains
///    phrases like "relationship between", "difference between". These
///    benefit from semantic similarity.
/// 4. `General` — everything else.
pub(super) fn classify_query_intent(query: &str) -> QueryIntent {
    if is_keyword_query(query) {
        return QueryIntent::Keyword;
    }

    let lower = query.trim().to_lowercase();
    if lower.is_empty() {
        return QueryIntent::General;
    }

    // Split on first word for prefix matching.
    let first_word = lower.split_whitespace().next().unwrap_or("");

    // Factual "how many/much/old/long/often" — must be checked before the generic
    // "how" → Conceptual rule so that "How many children …" is Factual.
    if lower.starts_with("how many")
        || lower.starts_with("how much")
        || lower.starts_with("how old")
        || lower.starts_with("how long")
        || lower.starts_with("how often")
    {
        return QueryIntent::Factual;
    }

    // Conceptual signals.
    if matches!(
        first_word,
        "why" | "how" | "explain" | "describe" | "elaborate"
    ) || lower.starts_with("what is the relationship")
        || lower.starts_with("what is the difference")
        || lower.contains("difference between")
        || lower.contains("relationship between")
        || lower.contains("compared to")
        || lower.contains("pros and cons")
    {
        return QueryIntent::Conceptual;
    }

    // Factual signals.
    if matches!(
        first_word,
        "who" | "what" | "when" | "where" | "which" | "name" | "list" | "find"
    ) || lower.contains("date of")
        || lower.contains("name of")
        || lower.contains("number of")
    {
        return QueryIntent::Factual;
    }

    QueryIntent::General
}

/// Detects whether a query warrants additional retrieval candidates.
///
/// Returns a multiplier for the candidate limit:
/// - 2.0x for multi-hop indicators ("and" combined with relationship/connect/between/both)
/// - 1.5x for broad temporal queries ("last month/year", "this month", "over the past")
/// - 1.0x otherwise (no adjustment)
pub(super) fn detect_dynamic_limit_mult(query: &str) -> f64 {
    let lower = query.trim().to_lowercase();

    // Multi-hop: "and" + relationship words
    let has_and = lower.contains(" and ");
    let multi_hop_indicators = [
        "relationship",
        "connect",
        "between",
        "both",
        "related",
        "sister",
        "brother",
        "friend",
    ];
    if has_and && multi_hop_indicators.iter().any(|w| lower.contains(w)) {
        return 2.0;
    }

    // Broad temporal patterns
    let temporal_patterns = [
        "last month",
        "last year",
        "this month",
        "this year",
        "over the past",
        "in the past",
        "recent months",
        "past year",
        "past month",
        "few months",
    ];
    if temporal_patterns.iter().any(|p| lower.contains(p)) {
        return 1.5;
    }

    1.0
}

/// Default number of reader connections in the pool for file-backed databases.
const DEFAULT_READER_COUNT: usize = 4;

/// Default number of writes between WAL checkpoints.
const WAL_CHECKPOINT_INTERVAL: u64 = 10;

/// Connection pool providing one writer connection and N reader connections.
///
/// In WAL mode, SQLite allows concurrent readers while a single writer holds
/// the write lock. `rusqlite::Connection` is `!Sync`, so each connection is
/// wrapped in its own `Mutex`.
///
/// For in-memory databases (`:memory:` or `file::memory:`) that cannot share
/// state across connections, the pool falls back to single-connection mode
/// where all operations use the writer.
#[derive(Debug)]
pub(super) struct ConnPool {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    /// Round-robin counter for reader selection.
    reader_idx: std::sync::atomic::AtomicUsize,
    /// Monotonic write counter for periodic WAL checkpoints.
    write_count: std::sync::atomic::AtomicU64,
    /// Whether this pool is file-backed (WAL checkpoints only apply to files).
    is_file_backed: bool,
}

impl ConnPool {
    /// Opens a file-backed connection pool: one writer + N readers.
    ///
    /// All connections share the same SQLite database file and run in WAL mode.
    /// Performs a startup WAL checkpoint (TRUNCATE) to reclaim any stale WAL
    /// from previous runs.
    pub(super) fn open_file(path: &Path, embedding_dim: usize) -> Result<Self> {
        #[cfg(feature = "sqlite-vec")]
        super::ensure_vec_extension_registered();

        super::initialize_parent_dir(path)?;

        let writer = open_connection(path)?;
        // Writer sets WAL mode; readers inherit it from the file.
        initialize_schema(&writer, embedding_dim)?;

        // Startup checkpoint: safe because we're the only writer at init time.
        if let Err(e) = writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
            tracing::debug!("startup WAL checkpoint skipped: {e}");
        }

        let mut readers = Vec::with_capacity(DEFAULT_READER_COUNT);
        for _ in 0..DEFAULT_READER_COUNT {
            let reader = open_connection(path)?;
            configure_reader(&reader)?;
            readers.push(Mutex::new(reader));
        }

        Ok(Self {
            writer: Mutex::new(writer),
            readers,
            reader_idx: std::sync::atomic::AtomicUsize::new(0),
            write_count: std::sync::atomic::AtomicU64::new(0),
            is_file_backed: true,
        })
    }

    /// Opens an in-memory pool (single connection, no reader pool).
    ///
    /// Used for tests and non-file-backed scenarios where multiple connections
    /// cannot share state.
    pub(super) fn open_in_memory(embedding_dim: usize) -> Result<Self> {
        #[cfg(feature = "sqlite-vec")]
        super::ensure_vec_extension_registered();

        let conn = Connection::open_in_memory().context("failed to open in-memory sqlite")?;
        initialize_schema(&conn, embedding_dim)?;

        Ok(Self {
            writer: Mutex::new(conn),
            readers: Vec::new(),
            reader_idx: std::sync::atomic::AtomicUsize::new(0),
            write_count: std::sync::atomic::AtomicU64::new(0),
            is_file_backed: false,
        })
    }

    /// Acquires the writer connection.
    pub(super) fn writer(&self) -> Result<MutexGuard<'_, Connection>> {
        self.writer
            .lock()
            .map_err(|_| anyhow!("sqlite writer mutex poisoned"))
    }

    /// Returns `true` when the pool has dedicated reader connections.
    pub(super) fn has_readers(&self) -> bool {
        !self.readers.is_empty()
    }

    /// Increments the write counter and runs a passive WAL checkpoint every
    /// [`WAL_CHECKPOINT_INTERVAL`] writes. Call after each write transaction
    /// commits. No-op for in-memory databases.
    pub(super) fn note_write(&self) {
        if !self.is_file_backed {
            return;
        }
        let count = self
            .write_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        if !count.is_multiple_of(WAL_CHECKPOINT_INTERVAL) {
            return;
        }
        if let Ok(conn) = self.writer() {
            match conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);") {
                Ok(()) => tracing::debug!(writes = count, "WAL passive checkpoint completed"),
                Err(e) => tracing::debug!(writes = count, "WAL passive checkpoint failed: {e}"),
            }
        }
    }

    /// Acquires a reader connection via round-robin. Falls back to the writer
    /// when no dedicated readers exist (in-memory mode).
    pub(super) fn reader(&self) -> Result<MutexGuard<'_, Connection>> {
        if self.readers.is_empty() {
            return self.writer();
        }
        let idx = self
            .reader_idx
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            % self.readers.len();
        self.readers[idx]
            .lock()
            .map_err(|_| anyhow!("sqlite reader mutex poisoned"))
    }
}

/// Opens a single SQLite connection with standard PRAGMAs.
fn open_connection(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("failed to open sqlite database at {}", path.display()))?;
    configure_reader(&conn)?;
    Ok(conn)
}

/// Applies read-oriented PRAGMAs shared by all connections (writer and readers).
fn configure_reader(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;\
         PRAGMA busy_timeout=5000;\
         PRAGMA cache_size=-16000;\
         PRAGMA mmap_size=33554432;\
         PRAGMA synchronous=NORMAL;\
         PRAGMA temp_store=MEMORY;",
    )
    .context("failed to set connection PRAGMAs")?;
    Ok(())
}

pub(super) fn canonicalize(content: &str) -> String {
    let stripped: String = content
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '*' | '#' | '`' | '~' | '[' | ']' | '(' | ')' | '>' | '|' | '_'
            )
        })
        .collect();
    stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

pub(super) fn canonical_hash(content: &str) -> String {
    let canonical = canonicalize(content);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract entity slugs from tags matching the `entity:*` prefix pattern.
///
/// Given tags like `["entity:people:alice", "entity:tools:react", "locomo-test"]`,
/// returns `["entity:people:alice", "entity:tools:react"]`.
pub(super) fn extract_entities_from_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .filter(|tag| tag.starts_with("entity:"))
        .cloned()
        .collect()
}

pub(super) fn parse_tags_from_db(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

pub(super) fn parse_metadata_from_db(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw)
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default()))
}

/// Maximum number of synonyms to inject per query token.
/// Keeps FTS5 queries from growing too large on synonym-rich words.
const SYNONYM_CAP: usize = 3;

/// Returns synonyms for common memory-relevant words.
///
/// Each entry maps a word to its synonym group. The mapping is bidirectional:
/// every word in a group maps to all *other* words in that group.
///
/// Returns an empty slice for words without known synonyms.
fn get_synonyms(word: &str) -> &'static [&'static str] {
    match word {
        "buy" | "purchase" | "bought" => match word {
            "buy" => &["purchase", "bought"],
            "purchase" => &["buy", "bought"],
            "bought" => &["buy", "purchase"],
            _ => &[],
        },
        "movie" | "film" => match word {
            "movie" => &["film"],
            "film" => &["movie"],
            _ => &[],
        },
        "doctor" | "physician" | "dr" => match word {
            "doctor" => &["physician", "dr"],
            "physician" => &["doctor", "dr"],
            "dr" => &["doctor", "physician"],
            _ => &[],
        },
        "phone" | "telephone" | "mobile" | "cell" => match word {
            "phone" => &["telephone", "mobile", "cell"],
            "telephone" => &["phone", "mobile", "cell"],
            "mobile" => &["phone", "telephone", "cell"],
            "cell" => &["phone", "telephone", "mobile"],
            _ => &[],
        },
        "car" | "automobile" | "vehicle" => match word {
            "car" => &["automobile", "vehicle"],
            "automobile" => &["car", "vehicle"],
            "vehicle" => &["car", "automobile"],
            _ => &[],
        },
        "happy" | "glad" | "pleased" | "joyful" => match word {
            "happy" => &["glad", "pleased", "joyful"],
            "glad" => &["happy", "pleased", "joyful"],
            "pleased" => &["happy", "glad", "joyful"],
            "joyful" => &["happy", "glad", "pleased"],
            _ => &[],
        },
        "sad" | "unhappy" | "depressed" => match word {
            "sad" => &["unhappy", "depressed"],
            "unhappy" => &["sad", "depressed"],
            "depressed" => &["sad", "unhappy"],
            _ => &[],
        },
        "big" | "large" | "huge" | "enormous" => match word {
            "big" => &["large", "huge", "enormous"],
            "large" => &["big", "huge", "enormous"],
            "huge" => &["big", "large", "enormous"],
            "enormous" => &["big", "large", "huge"],
            _ => &[],
        },
        "small" | "little" | "tiny" | "mini" => match word {
            "small" => &["little", "tiny", "mini"],
            "little" => &["small", "tiny", "mini"],
            "tiny" => &["small", "little", "mini"],
            "mini" => &["small", "little", "tiny"],
            _ => &[],
        },
        "start" | "begin" | "commence" => match word {
            "start" => &["begin", "commence"],
            "begin" => &["start", "commence"],
            "commence" => &["start", "begin"],
            _ => &[],
        },
        "end" | "finish" | "complete" | "conclude" => match word {
            "end" => &["finish", "complete", "conclude"],
            "finish" => &["end", "complete", "conclude"],
            "complete" => &["end", "finish", "conclude"],
            "conclude" => &["end", "finish", "complete"],
            _ => &[],
        },
        "fast" | "quick" | "rapid" | "swift" => match word {
            "fast" => &["quick", "rapid", "swift"],
            "quick" => &["fast", "rapid", "swift"],
            "rapid" => &["fast", "quick", "swift"],
            "swift" => &["fast", "quick", "rapid"],
            _ => &[],
        },
        "slow" | "sluggish" | "gradual" => match word {
            "slow" => &["sluggish", "gradual"],
            "sluggish" => &["slow", "gradual"],
            "gradual" => &["slow", "sluggish"],
            _ => &[],
        },
        "old" | "ancient" | "elderly" | "aged" => match word {
            "old" => &["ancient", "elderly", "aged"],
            "ancient" => &["old", "elderly", "aged"],
            "elderly" => &["old", "ancient", "aged"],
            "aged" => &["old", "ancient", "elderly"],
            _ => &[],
        },
        "new" | "fresh" | "recent" | "modern" => match word {
            "new" => &["fresh", "recent", "modern"],
            "fresh" => &["new", "recent", "modern"],
            "recent" => &["new", "fresh", "modern"],
            "modern" => &["new", "fresh", "recent"],
            _ => &[],
        },
        "house" | "home" | "residence" => match word {
            "house" => &["home", "residence"],
            "home" => &["house", "residence"],
            "residence" => &["house", "home"],
            _ => &[],
        },
        "job" | "work" | "employment" | "occupation" | "career" => match word {
            "job" => &["work", "employment", "occupation", "career"],
            "work" => &["job", "employment", "occupation", "career"],
            "employment" => &["job", "work", "occupation", "career"],
            "occupation" => &["job", "work", "employment", "career"],
            "career" => &["job", "work", "employment", "occupation"],
            _ => &[],
        },
        "trip" | "travel" | "journey" | "vacation" => match word {
            "trip" => &["travel", "journey", "vacation"],
            "travel" => &["trip", "journey", "vacation"],
            "journey" => &["trip", "travel", "vacation"],
            "vacation" => &["trip", "travel", "journey"],
            _ => &[],
        },
        "food" | "meal" | "cuisine" | "dish" => match word {
            "food" => &["meal", "cuisine", "dish"],
            "meal" => &["food", "cuisine", "dish"],
            "cuisine" => &["food", "meal", "dish"],
            "dish" => &["food", "meal", "cuisine"],
            _ => &[],
        },
        "child" | "kid" | "offspring" => match word {
            "child" => &["kid", "offspring"],
            "kid" => &["child", "offspring"],
            "offspring" => &["child", "kid"],
            _ => &[],
        },
        "friend" | "buddy" | "pal" | "companion" => match word {
            "friend" => &["buddy", "pal", "companion"],
            "buddy" => &["friend", "pal", "companion"],
            "pal" => &["friend", "buddy", "companion"],
            "companion" => &["friend", "buddy", "pal"],
            _ => &[],
        },
        "money" | "cash" | "funds" | "currency" => match word {
            "money" => &["cash", "funds", "currency"],
            "cash" => &["money", "funds", "currency"],
            "funds" => &["money", "cash", "currency"],
            "currency" => &["money", "cash", "funds"],
            _ => &[],
        },
        "talk" | "speak" | "chat" | "discuss" | "conversation" => match word {
            "talk" => &["speak", "chat", "discuss", "conversation"],
            "speak" => &["talk", "chat", "discuss", "conversation"],
            "chat" => &["talk", "speak", "discuss", "conversation"],
            "discuss" => &["talk", "speak", "chat", "conversation"],
            "conversation" => &["talk", "speak", "chat", "discuss"],
            _ => &[],
        },
        "like" | "enjoy" | "prefer" | "fond" => match word {
            "like" => &["enjoy", "prefer", "fond"],
            "enjoy" => &["like", "prefer", "fond"],
            "prefer" => &["like", "enjoy", "fond"],
            "fond" => &["like", "enjoy", "prefer"],
            _ => &[],
        },
        "hate" | "dislike" | "detest" | "loathe" => match word {
            "hate" => &["dislike", "detest", "loathe"],
            "dislike" => &["hate", "detest", "loathe"],
            "detest" => &["hate", "dislike", "loathe"],
            "loathe" => &["hate", "dislike", "detest"],
            _ => &[],
        },
        "want" | "desire" | "wish" | "need" => match word {
            "want" => &["desire", "wish", "need"],
            "desire" => &["want", "wish", "need"],
            "wish" => &["want", "desire", "need"],
            "need" => &["want", "desire", "wish"],
            _ => &[],
        },
        "think" | "believe" | "consider" | "reckon" => match word {
            "think" => &["believe", "consider", "reckon"],
            "believe" => &["think", "consider", "reckon"],
            "consider" => &["think", "believe", "reckon"],
            "reckon" => &["think", "believe", "consider"],
            _ => &[],
        },
        "look" | "see" | "watch" | "observe" | "view" => match word {
            "look" => &["see", "watch", "observe", "view"],
            "see" => &["look", "watch", "observe", "view"],
            "watch" => &["look", "see", "observe", "view"],
            "observe" => &["look", "see", "watch", "view"],
            "view" => &["look", "see", "watch", "observe"],
            _ => &[],
        },
        "give" | "provide" | "offer" | "donate" => match word {
            "give" => &["provide", "offer", "donate"],
            "provide" => &["give", "offer", "donate"],
            "offer" => &["give", "provide", "donate"],
            "donate" => &["give", "provide", "offer"],
            _ => &[],
        },
        "take" | "grab" | "seize" | "accept" => match word {
            "take" => &["grab", "seize", "accept"],
            "grab" => &["take", "seize", "accept"],
            "seize" => &["take", "grab", "accept"],
            "accept" => &["take", "grab", "seize"],
            _ => &[],
        },
        "make" | "create" | "build" | "construct" => match word {
            "make" => &["create", "build", "construct"],
            "create" => &["make", "build", "construct"],
            "build" => &["make", "create", "construct"],
            "construct" => &["make", "create", "build"],
            _ => &[],
        },
        "show" | "display" | "demonstrate" | "present" | "exhibit" => match word {
            "show" => &["display", "demonstrate", "present", "exhibit"],
            "display" => &["show", "demonstrate", "present", "exhibit"],
            "demonstrate" => &["show", "display", "present", "exhibit"],
            "present" => &["show", "display", "demonstrate", "exhibit"],
            "exhibit" => &["show", "display", "demonstrate", "present"],
            _ => &[],
        },
        "tell" | "inform" | "notify" => match word {
            "tell" => &["inform", "notify"],
            "inform" => &["tell", "notify"],
            "notify" => &["tell", "inform"],
            _ => &[],
        },
        "help" | "assist" | "support" | "aid" => match word {
            "help" => &["assist", "support", "aid"],
            "assist" => &["help", "support", "aid"],
            "support" => &["help", "assist", "aid"],
            "aid" => &["help", "assist", "support"],
            _ => &[],
        },
        "move" | "relocate" | "transfer" => match word {
            "move" => &["relocate", "transfer"],
            "relocate" => &["move", "transfer"],
            "transfer" => &["move", "relocate"],
            _ => &[],
        },
        "play" | "perform" | "game" => match word {
            "play" => &["perform", "game"],
            "perform" => &["play", "game"],
            "game" => &["play", "perform"],
            _ => &[],
        },
        "run" | "execute" | "sprint" | "jog" => match word {
            "run" => &["execute", "sprint", "jog"],
            "execute" => &["run", "sprint", "jog"],
            "sprint" => &["run", "execute", "jog"],
            "jog" => &["run", "execute", "sprint"],
            _ => &[],
        },
        "eat" | "consume" | "dine" => match word {
            "eat" => &["consume", "dine"],
            "consume" => &["eat", "dine"],
            "dine" => &["eat", "consume"],
            _ => &[],
        },
        "drink" | "beverage" | "sip" => match word {
            "drink" => &["beverage", "sip"],
            "beverage" => &["drink", "sip"],
            "sip" => &["drink", "beverage"],
            _ => &[],
        },
        "sleep" | "rest" | "nap" | "slumber" => match word {
            "sleep" => &["rest", "nap", "slumber"],
            "rest" => &["sleep", "nap", "slumber"],
            "nap" => &["sleep", "rest", "slumber"],
            "slumber" => &["sleep", "rest", "nap"],
            _ => &[],
        },
        "sick" | "ill" | "unwell" => match word {
            "sick" => &["ill", "unwell"],
            "ill" => &["sick", "unwell"],
            "unwell" => &["sick", "ill"],
            _ => &[],
        },
        "pain" | "ache" | "hurt" | "sore" => match word {
            "pain" => &["ache", "hurt", "sore"],
            "ache" => &["pain", "hurt", "sore"],
            "hurt" => &["pain", "ache", "sore"],
            "sore" => &["pain", "ache", "hurt"],
            _ => &[],
        },
        "dog" | "puppy" | "canine" | "pup" => match word {
            "dog" => &["puppy", "canine", "pup"],
            "puppy" => &["dog", "canine", "pup"],
            "canine" => &["dog", "puppy", "pup"],
            "pup" => &["dog", "puppy", "canine"],
            _ => &[],
        },
        "cat" | "kitten" | "feline" => match word {
            "cat" => &["kitten", "feline"],
            "kitten" => &["cat", "feline"],
            "feline" => &["cat", "kitten"],
            _ => &[],
        },
        "book" | "novel" | "publication" => match word {
            "book" => &["novel", "publication"],
            "novel" => &["book", "publication"],
            "publication" => &["book", "novel"],
            _ => &[],
        },
        "school" | "college" | "university" | "academy" => match word {
            "school" => &["college", "university", "academy"],
            "college" => &["school", "university", "academy"],
            "university" => &["school", "college", "academy"],
            "academy" => &["school", "college", "university"],
            _ => &[],
        },
        "city" | "town" | "urban" => match word {
            "city" => &["town", "urban"],
            "town" => &["city", "urban"],
            "urban" => &["city", "town"],
            _ => &[],
        },
        "country" | "nation" | "state" => match word {
            "country" => &["nation", "state"],
            "nation" => &["country", "state"],
            "state" => &["country", "nation"],
            _ => &[],
        },
        "meet" | "encounter" | "rendezvous" => match word {
            "meet" => &["encounter", "rendezvous"],
            "encounter" => &["meet", "rendezvous"],
            "rendezvous" => &["meet", "encounter"],
            _ => &[],
        },
        "leave" | "depart" | "exit" => match word {
            "leave" => &["depart", "exit"],
            "depart" => &["leave", "exit"],
            "exit" => &["leave", "depart"],
            _ => &[],
        },
        "arrive" | "reach" | "come" => match word {
            "arrive" => &["reach", "come"],
            "reach" => &["arrive", "come"],
            "come" => &["arrive", "reach"],
            _ => &[],
        },
        "fix" | "repair" | "mend" => match word {
            "fix" => &["repair", "mend"],
            "repair" => &["fix", "mend"],
            "mend" => &["fix", "repair"],
            _ => &[],
        },
        "break" | "shatter" | "crack" | "damage" => match word {
            "break" => &["shatter", "crack", "damage"],
            "shatter" => &["break", "crack", "damage"],
            "crack" => &["break", "shatter", "damage"],
            "damage" => &["break", "shatter", "crack"],
            _ => &[],
        },
        "close" | "shut" | "near" => match word {
            "close" => &["shut", "near"],
            "shut" => &["close", "near"],
            "near" => &["close", "shut"],
            _ => &[],
        },
        "open" | "unlock" | "accessible" => match word {
            "open" => &["unlock", "accessible"],
            "unlock" => &["open", "accessible"],
            "accessible" => &["open", "unlock"],
            _ => &[],
        },
        _ => &[],
    }
}

pub(super) fn build_fts5_query(input: &str) -> String {
    let raw_tokens: Vec<&str> = input.split_whitespace().filter(|t| !t.is_empty()).collect();

    if raw_tokens.is_empty() {
        return "\"\"".to_string();
    }

    // Filter out stopwords before constructing the FTS5 query to prevent
    // common words like "the", "to", "is" from diluting BM25 scores.
    let filtered_tokens: Vec<&str> = raw_tokens
        .iter()
        .copied()
        .filter(|t| !is_stopword(&t.to_lowercase()))
        .collect();

    // If all tokens were stopwords, fall back to using the original tokens
    // so we don't produce an empty query.
    let effective_tokens = if filtered_tokens.is_empty() {
        &raw_tokens
    } else {
        &filtered_tokens
    };

    // Escape each token for FTS5 (double-quote escaping) and wrap in quotes.
    let escaped: Vec<String> = effective_tokens
        .iter()
        .map(|t| {
            let e = t.replace('"', "\"\"");
            format!("\"{e}\"")
        })
        .collect();

    // ── Synonym expansion ──
    // For each non-stopword token, look up synonyms and add them as OR terms.
    // Also apply simple_stem() to both originals and synonyms so inflected
    // forms match (e.g. "bought" stems to "bought", synonym "purchase" stems
    // to "purchas" which matches "purchased" in FTS5).
    // Capped at SYNONYM_CAP synonyms per token to avoid query explosion.
    let mut synonym_terms: Vec<String> = Vec::new();
    for token in effective_tokens {
        let lower = token.to_lowercase();
        let syns = get_synonyms(&lower);
        if syns.is_empty() {
            continue;
        }
        // Collect unique stemmed forms to avoid duplicates.
        let original_stem = simple_stem(&lower);
        for syn in syns.iter().take(SYNONYM_CAP) {
            let syn_stem = simple_stem(syn);
            // Skip if the stemmed synonym is the same as the original token
            // (would be redundant with the already-present term).
            if syn_stem == lower || syn_stem == original_stem {
                continue;
            }
            let escaped_syn = syn.replace('"', "\"\"");
            synonym_terms.push(format!("\"{escaped_syn}\""));
        }
    }

    // For 1-2 token queries, bigrams would be redundant (either a single
    // token or an exact duplicate of the full query). Just join with OR.
    if effective_tokens.len() < 3 {
        let mut parts = escaped;
        parts.extend(synonym_terms);
        return parts.join(" OR ");
    }

    // 3+ tokens: append adjacent-token bigrams as quoted phrases.
    let bigrams: Vec<String> = effective_tokens
        .windows(2)
        .map(|pair| {
            let a = pair[0].replace('"', "\"\"");
            let b = pair[1].replace('"', "\"\"");
            format!("\"{a} {b}\"")
        })
        .collect();

    let mut parts = escaped;
    parts.extend(bigrams);
    parts.extend(synonym_terms);
    parts.join(" OR ")
}

/// Encodes a slice of f32 values as little-endian bytes.
pub(super) fn encode_embedding(v: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(v.len() * 4);
    for &val in v {
        buf.extend_from_slice(&val.to_le_bytes());
    }
    buf
}

/// Decodes an embedding BLOB. Tries binary (little-endian f32) first,
/// falls back to JSON for backwards compatibility with existing data.
pub(super) fn decode_embedding(blob: &[u8]) -> Result<Vec<f32>> {
    if blob.is_empty() {
        return Ok(Vec::new());
    }
    // Binary format: length must be a multiple of 4
    if blob.len().is_multiple_of(4) {
        // Quick heuristic: JSON always starts with '[' (0x5B)
        if blob[0] != b'[' {
            return Ok(decode_binary_embedding(blob));
        }
        // First byte is '[' — could be JSON or binary coincidence.
        // Try JSON first (backwards compat), fall back to binary.
        if let Ok(v) = serde_json::from_slice::<Vec<f32>>(blob) {
            return Ok(v);
        }
        return Ok(decode_binary_embedding(blob));
    }
    // Not a multiple of 4 — must be JSON
    serde_json::from_slice(blob).context("failed to decode embedding (neither binary nor JSON)")
}

fn decode_binary_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| {
            let mut bytes = [0_u8; 4];
            bytes.copy_from_slice(chunk);
            f32::from_le_bytes(bytes)
        })
        .collect()
}

/// Dot product of two vectors. Equivalent to cosine similarity when inputs are L2-normalized.
pub(crate) fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    debug_assert!(
        (a.iter().map(|v| v * v).sum::<f32>().sqrt() - 1.0).abs() < 0.01,
        "input a is not L2-normalized"
    );
    debug_assert!(
        (b.iter().map(|v| v * v).sum::<f32>().sqrt() - 1.0).abs() < 0.01,
        "input b is not L2-normalized"
    );
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[allow(dead_code)]
pub(super) fn resolve_priority(event_type: Option<&str>, priority: Option<i64>) -> u8 {
    if let Some(value) = priority
        && (1..=5).contains(&value)
    {
        return u8::try_from(value).unwrap_or(3);
    }
    event_type
        .map(|value| {
            let event_type = value
                .parse::<EventType>()
                .unwrap_or_else(|err| match err {});
            let priority = event_type.default_priority();
            if priority == 0 {
                3
            } else {
                u8::try_from(priority).unwrap_or(3)
            }
        })
        .unwrap_or(3)
}

pub(super) fn normalize_for_dedup(content: &str) -> String {
    let collapsed = content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    collapsed.chars().take(150).collect()
}

/// Basic ISO 8601 validation: accepts date-only (`YYYY-MM-DD`) or
/// datetime with optional timezone (`YYYY-MM-DDThh:mm:ss…`).
/// This is intentionally lenient — it catches gross typos without
/// requiring a full RFC 3339 parser.
pub(super) fn validate_iso8601(s: &str) -> bool {
    // Minimum: "YYYY-MM-DD" (10 chars)
    if s.len() < 10 {
        return false;
    }
    let bytes = s.as_bytes();
    // First 4 chars must be digits (year)
    if !bytes[0..4].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // Hyphens at positions 4 and 7
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    // Month and day digits
    if !bytes[5..7].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if !bytes[8..10].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    true
}

pub(super) fn escape_like_pattern(input: &str) -> String {
    let escaped = input
        .to_lowercase()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

pub(super) fn search_result_from_row(row: &rusqlite::Row) -> rusqlite::Result<SearchResult> {
    let raw_tags: String = row.get(2)?;
    let raw_metadata: String = row.get(4)?;
    let event_type_str: Option<String> = row.get(5).ok();
    Ok(SearchResult {
        id: row.get(0)?,
        content: row.get(1)?,
        tags: parse_tags_from_db(&raw_tags),
        importance: row.get(3)?,
        metadata: parse_metadata_from_db(&raw_metadata),
        event_type: event_type_from_sql(event_type_str),
        session_id: row.get(6).ok(),
        project: row.get(7).ok(),
        entity_id: row.get(8).ok(),
        agent_type: row.get(9).ok(),
    })
}

/// Converts an `Option<EventType>` to an `Option<String>` for SQL parameter binding.
pub(super) fn event_type_to_sql(et: &Option<EventType>) -> Option<String> {
    et.as_ref().map(|e| e.to_string())
}

/// Parses an `Option<String>` from the DB into an `Option<EventType>`.
pub(super) fn event_type_from_sql(s: Option<String>) -> Option<EventType> {
    EventType::from_optional(s.as_deref())
}

/// Appends WHERE-clause fragments for every non-None field in `opts` to `sql`,
/// pushing corresponding values into `params`. `idx` is the next `?N` placeholder
/// index and is updated in place. `col_prefix` is prepended to column names
/// (e.g. `"m."` for joined queries, `""` for direct table queries).
pub(super) fn append_search_filters(
    sql: &mut String,
    params: &mut Vec<rusqlite::types::Value>,
    idx: &mut usize,
    opts: &SearchOptions,
    col_prefix: &str,
) {
    use rusqlite::types::Value as SqlValue;

    if let Some(ref event_type) = opts.event_type {
        sql.push_str(&format!(" AND {}event_type = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(event_type.to_string()));
        *idx += 1;
    }
    if let Some(ref project) = opts.project {
        sql.push_str(&format!(" AND {}project = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(project.clone()));
        *idx += 1;
    }
    if let Some(ref session_id) = opts.session_id {
        sql.push_str(&format!(" AND {}session_id = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(session_id.clone()));
        *idx += 1;
    }
    if let Some(ref entity_id) = opts.entity_id {
        sql.push_str(&format!(" AND {}entity_id = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(entity_id.clone()));
        *idx += 1;
    }
    if let Some(ref agent_type) = opts.agent_type {
        sql.push_str(&format!(" AND {}agent_type = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(agent_type.clone()));
        *idx += 1;
    }
    if let Some(importance_min) = opts.importance_min {
        sql.push_str(&format!(" AND {}importance >= ?{}", col_prefix, *idx));
        params.push(SqlValue::Real(importance_min));
        *idx += 1;
    }
    if let Some(ref created_after) = opts.created_after {
        sql.push_str(&format!(" AND {}created_at >= ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(created_after.clone()));
        *idx += 1;
    }
    if let Some(ref created_before) = opts.created_before {
        sql.push_str(&format!(" AND {}created_at <= ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(created_before.clone()));
        *idx += 1;
    }
    if let Some(ref event_after) = opts.event_after {
        sql.push_str(&format!(
            " AND COALESCE({}event_at, '{}') >= ?{}",
            col_prefix, EPOCH_FALLBACK, *idx
        ));
        params.push(SqlValue::Text(event_after.clone()));
        *idx += 1;
    }
    if let Some(ref event_before) = opts.event_before {
        sql.push_str(&format!(
            " AND COALESCE({}event_at, '{}') <= ?{}",
            col_prefix, EPOCH_FALLBACK, *idx
        ));
        params.push(SqlValue::Text(event_before.clone()));
        *idx += 1;
    }
}

pub(super) fn append_context_tag_filters(
    sql: &mut String,
    params: &mut Vec<rusqlite::types::Value>,
    idx: &mut usize,
    context_tags: Option<&[String]>,
    tags_expr: &str,
) {
    use rusqlite::types::Value as SqlValue;

    for tag in context_tags
        .into_iter()
        .flatten()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
    {
        sql.push_str(&format!(
            " AND ((json_valid({tags_expr}) AND EXISTS (SELECT 1 FROM json_each({tags_expr}) WHERE lower(value) = lower(?{idx}))) \
               OR (NOT json_valid({tags_expr}) AND {tags_expr} != '' AND instr(',' || lower({tags_expr}) || ',', ',' || lower(?{idx}) || ',') > 0))"
        ));
        params.push(SqlValue::Text(tag.to_owned()));
        *idx += 1;
    }
}

/// Converts a `Vec<SqlValue>` to a `Vec<&dyn ToSql>` for rusqlite parameter binding.
pub(super) fn to_param_refs(values: &[rusqlite::types::Value]) -> Vec<&dyn rusqlite::types::ToSql> {
    values
        .iter()
        .map(|v| v as &dyn rusqlite::types::ToSql)
        .collect()
}

pub(super) fn matches_search_options(
    candidate: &RankedSemanticCandidate,
    opts: &SearchOptions,
) -> bool {
    if let Some(ref event_type) = opts.event_type
        && candidate.result.event_type.as_ref() != Some(event_type)
    {
        return false;
    }
    if let Some(project) = opts.project.as_deref()
        && candidate.result.project.as_deref() != Some(project)
    {
        return false;
    }
    if let Some(session_id) = opts.session_id.as_deref()
        && candidate.result.session_id.as_deref() != Some(session_id)
    {
        return false;
    }
    if let Some(importance_min) = opts.importance_min
        && candidate.result.importance < importance_min
    {
        return false;
    }
    if let Some(created_after) = opts.created_after.as_deref()
        && candidate.created_at.as_str() < created_after
    {
        return false;
    }
    if let Some(created_before) = opts.created_before.as_deref()
        && candidate.created_at.as_str() > created_before
    {
        return false;
    }
    if let Some(entity_id) = opts.entity_id.as_deref()
        && candidate.entity_id.as_deref() != Some(entity_id)
    {
        return false;
    }
    if let Some(agent_type) = opts.agent_type.as_deref()
        && candidate.agent_type.as_deref() != Some(agent_type)
    {
        return false;
    }
    if let Some(event_after) = opts.event_after.as_deref()
        && candidate.event_at.as_str() < event_after
    {
        return false;
    }
    if let Some(event_before) = opts.event_before.as_deref()
        && candidate.event_at.as_str() > event_before
    {
        return false;
    }
    if let Some(context_tags) = opts.context_tags.as_ref() {
        let mut normalized_tags = context_tags
            .iter()
            .map(|tag| tag.trim())
            .filter(|tag| !tag.is_empty())
            .peekable();
        if normalized_tags.peek().is_some()
            && !normalized_tags.all(|tag| {
                candidate
                    .result
                    .tags
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(tag))
            })
        {
            return false;
        }
    }
    true
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_distance_to_similarity(distance: f64) -> f64 {
    (1.0 - distance).max(0.0)
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_delete(conn: &rusqlite::Connection, memory_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM vec_memories WHERE memory_id = ?1",
        params![memory_id],
    )
    .context("failed to delete from vec_memories")?;
    Ok(())
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_upsert(
    conn: &rusqlite::Connection,
    memory_id: &str,
    embedding: &[u8],
) -> Result<()> {
    // vec0 virtual tables don't support INSERT OR REPLACE; delete first then insert.
    vec_delete(conn, memory_id)?;
    conn.execute(
        "INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)",
        params![memory_id, embedding],
    )
    .context("failed to upsert into vec_memories")?;
    Ok(())
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_knn_search(
    conn: &rusqlite::Connection,
    query_embedding: &[f32],
    k: usize,
) -> Result<Vec<(String, f64)>> {
    let embedding_blob = encode_embedding(query_embedding);
    let k = k as i64;
    let mut stmt = conn
        .prepare("SELECT memory_id, distance FROM vec_memories WHERE embedding MATCH ?1 AND k = ?2")
        .context("failed to prepare vec KNN query")?;
    let rows = stmt
        .query_map(params![embedding_blob, k], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })
        .context("failed to execute vec KNN query")?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row.context("failed to decode vec KNN row")?);
    }
    Ok(results)
}

/// Maximum number of memory IDs hydrated per SQLite `IN (...)` batch.
#[cfg(feature = "sqlite-vec")]
pub(super) const HYDRATE_ID_CHUNK_SIZE: usize = 900;

#[cfg(feature = "sqlite-vec")]
#[derive(Debug, Clone)]
/// Fully decoded memory row used by sqlite-vec search paths after batched lookup.
pub(super) struct HydratedMemoryRow {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub importance: f64,
    pub metadata: serde_json::Value,
    pub event_type: Option<EventType>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub priority: Option<i64>,
    pub created_at: String,
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
    pub event_at: String,
}

#[cfg(feature = "sqlite-vec")]
/// Hydrates memory rows for an ordered ID list while applying the same filters as search paths.
///
/// The returned map is keyed by memory ID so callers can preserve their own ranking order.
pub(super) fn hydrate_memories_by_ids(
    conn: &rusqlite::Connection,
    ids: &[String],
    include_superseded: bool,
    opts: Option<&SearchOptions>,
    include_context_tags: bool,
) -> Result<HashMap<String, HydratedMemoryRow>> {
    use rusqlite::types::Value as SqlValue;

    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut hydrated = HashMap::with_capacity(ids.len());

    for chunk in ids.chunks(HYDRATE_ID_CHUNK_SIZE) {
        let mut sql = String::from(
            "SELECT id, content, tags, importance, metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type, event_at
             FROM memories
             WHERE id IN (",
        );
        let mut params: Vec<SqlValue> = Vec::with_capacity(chunk.len() + 12);
        for (idx, memory_id) in chunk.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            sql.push_str(&format!("?{}", idx + 1));
            params.push(SqlValue::Text(memory_id.clone()));
        }
        sql.push(')');

        let mut next_idx = chunk.len() + 1;
        if !include_superseded {
            sql.push_str(" AND superseded_by_id IS NULL");
        }
        if let Some(search_opts) = opts {
            append_search_filters(&mut sql, &mut params, &mut next_idx, search_opts, "");
            if include_context_tags {
                append_context_tag_filters(
                    &mut sql,
                    &mut params,
                    &mut next_idx,
                    search_opts.context_tags.as_deref(),
                    "tags",
                );
            }
        }

        let mut stmt = conn
            .prepare(&sql)
            .context("failed to prepare batched memory hydration query")?;
        let param_refs = to_param_refs(&params);
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(HydratedMemoryRow {
                    id: row.get::<_, String>(0)?,
                    content: row.get::<_, String>(1)?,
                    tags: parse_tags_from_db(&row.get::<_, String>(2)?),
                    importance: row.get::<_, f64>(3)?,
                    metadata: parse_metadata_from_db(&row.get::<_, String>(4)?),
                    event_type: event_type_from_sql(row.get::<_, Option<String>>(5).ok().flatten()),
                    session_id: row.get::<_, Option<String>>(6).ok().flatten(),
                    project: row.get::<_, Option<String>>(7).ok().flatten(),
                    priority: row.get::<_, Option<i64>>(8).ok().flatten(),
                    created_at: row
                        .get::<_, String>(9)
                        .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                    entity_id: row.get::<_, Option<String>>(10).ok().flatten(),
                    agent_type: row.get::<_, Option<String>>(11).ok().flatten(),
                    event_at: row
                        .get::<_, String>(12)
                        .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                })
            })
            .context("failed to execute batched memory hydration query")?;

        for row in rows {
            let row = row.context("failed to decode hydrated memory row")?;
            hydrated.insert(row.id.clone(), row);
        }
    }

    Ok(hydrated)
}

/// Result of expanding temporal references from a query string.
#[derive(Debug, Default)]
pub(super) struct TemporalExpansion {
    /// The query with temporal phrases stripped out.
    pub cleaned_query: String,
    /// ISO 8601 lower bound inferred from the temporal phrase.
    pub event_after: Option<String>,
    /// ISO 8601 upper bound inferred from the temporal phrase.
    pub event_before: Option<String>,
}

/// Detects temporal references in a query and converts them to date filters.
///
/// Recognized patterns:
/// - "today", "yesterday"
/// - "this week", "last week", "this month", "last month"
/// - "N days ago", "N weeks ago", "N months ago"
/// - "last N days", "last N weeks", "last N months", "past N days/weeks/months"
///
/// Returns the cleaned query (temporal phrase removed) and inferred date bounds.
/// If no temporal phrase is detected, returns the original query unchanged.
pub(super) fn expand_temporal_query(query: &str, now: &chrono::NaiveDate) -> TemporalExpansion {
    use chrono::{Datelike, Duration, NaiveDate};

    let lower = query.to_lowercase();

    // Try each pattern; first match wins.
    let result = if let Some(idx) = lower.find("yesterday") {
        let cleaned = remove_phrase(query, idx, 9);
        let date = (*now - Duration::days(1)).format("%Y-%m-%d").to_string();
        Some((
            cleaned,
            Some(date.clone()),
            Some(format!("{date}T23:59:59")),
        ))
    } else if let Some(idx) = lower.find("today") {
        let cleaned = remove_phrase(query, idx, 5);
        let date = now.format("%Y-%m-%d").to_string();
        Some((
            cleaned,
            Some(date.clone()),
            Some(format!("{date}T23:59:59")),
        ))
    } else if let Some(re_pos) = lower.find(" ago") {
        // "N days/weeks/months ago"
        let before = &lower[..re_pos];
        let all_words: Vec<&str> = before.split_whitespace().collect();
        if all_words.len() >= 2 {
            let unit = all_words[all_words.len() - 1];
            let num_str = all_words[all_words.len() - 2];
            num_str.parse::<i64>().ok().and_then(|n| {
                let (after, before_date) = compute_ago(now, n, unit)?;
                let phrase = format!("{num_str} {unit} ago");
                let phrase_start = lower.find(&phrase)?;
                let cleaned = remove_phrase(query, phrase_start, phrase.len());
                Some((cleaned, Some(after), Some(before_date)))
            })
        } else {
            None
        }
    } else if let Some(m) = try_prefix_n_unit(&lower, query, "last ", now) {
        Some(m)
    } else if let Some(m) = try_prefix_n_unit(&lower, query, "past ", now) {
        Some(m)
    } else if let Some(idx) = lower.find("last month") {
        let cleaned = remove_phrase(query, idx, 10);
        NaiveDate::from_ymd_opt(now.year(), now.month(), 1).and_then(|first_this| {
            let last_day_prev = first_this - Duration::days(1);
            let first_prev =
                NaiveDate::from_ymd_opt(last_day_prev.year(), last_day_prev.month(), 1)?;
            Some((
                cleaned,
                Some(first_prev.format("%Y-%m-%d").to_string()),
                Some(format!("{}T23:59:59", last_day_prev.format("%Y-%m-%d"))),
            ))
        })
    } else if let Some(idx) = lower.find("last week") {
        let cleaned = remove_phrase(query, idx, 9);
        let weekday = now.weekday().num_days_from_monday();
        let this_monday = *now - Duration::days(weekday as i64);
        let last_monday = this_monday - Duration::days(7);
        let last_sunday = this_monday - Duration::days(1);
        Some((
            cleaned,
            Some(last_monday.format("%Y-%m-%d").to_string()),
            Some(format!("{}T23:59:59", last_sunday.format("%Y-%m-%d"))),
        ))
    } else if let Some(idx) = lower.find("this week") {
        let cleaned = remove_phrase(query, idx, 9);
        let weekday = now.weekday().num_days_from_monday();
        let start = *now - Duration::days(weekday as i64);
        Some((cleaned, Some(start.format("%Y-%m-%d").to_string()), None))
    } else if let Some(idx) = lower.find("this month") {
        let cleaned = remove_phrase(query, idx, 10);
        NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
            .map(|start| (cleaned, Some(start.format("%Y-%m-%d").to_string()), None))
    } else {
        None
    };

    match result {
        Some((cleaned, after, before)) => {
            let trimmed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
            if trimmed.is_empty() {
                TemporalExpansion {
                    cleaned_query: query.to_string(),
                    event_after: after,
                    event_before: before,
                }
            } else {
                TemporalExpansion {
                    cleaned_query: trimmed,
                    event_after: after,
                    event_before: before,
                }
            }
        }
        None => TemporalExpansion {
            cleaned_query: query.to_string(),
            event_after: None,
            event_before: None,
        },
    }
}

/// Tries to match "last N unit" or "past N unit" at `prefix` position.
fn try_prefix_n_unit(
    lower: &str,
    query: &str,
    prefix: &str,
    now: &chrono::NaiveDate,
) -> Option<(String, Option<String>, Option<String>)> {
    let idx = lower.find(prefix)?;
    let rest = &lower[idx + prefix.len()..];
    let tokens: Vec<&str> = rest.split_whitespace().take(2).collect();
    if tokens.len() < 2 {
        return None;
    }
    let n: i64 = tokens[0].parse().ok()?;
    let unit = tokens[1];
    let (after, before_date) = compute_ago(now, n, unit)?;
    let phrase_len = prefix.len() + tokens[0].len() + 1 + tokens[1].len();
    let cleaned = remove_phrase(query, idx, phrase_len);
    Some((cleaned, Some(after), Some(before_date)))
}

fn remove_phrase(query: &str, start: usize, len: usize) -> String {
    let mut result = String::with_capacity(query.len());
    result.push_str(&query[..start]);
    if start + len < query.len() {
        result.push_str(&query[start + len..]);
    }
    result
}

fn compute_ago(now: &chrono::NaiveDate, n: i64, unit: &str) -> Option<(String, String)> {
    use chrono::{Datelike, Duration};
    let start = match unit.trim_end_matches('s') {
        "day" => *now - Duration::days(n),
        "week" => *now - Duration::weeks(n),
        "month" => {
            // Approximate: subtract n months
            let n_i32 = i32::try_from(n).ok()?;
            #[allow(clippy::cast_sign_loss)]
            let total_months = now.year() * 12 + now.month() as i32 - 1 - n_i32;
            let year = total_months / 12;
            let month = u32::try_from(total_months % 12 + 1).ok()?;
            let day = now.day().min(28); // safe day
            chrono::NaiveDate::from_ymd_opt(year, month, day)?
        }
        _ => return None,
    };
    Some((
        start.format("%Y-%m-%d").to_string(),
        format!("{}T23:59:59", now.format("%Y-%m-%d")),
    ))
}

/// Stopwords for entity extraction from queries — words that appear capitalized
/// but are not entities (question words, auxiliaries, months, etc.)
const ENTITY_STOPWORDS: &[&str] = &[
    "What",
    "How",
    "Why",
    "When",
    "Where",
    "Which",
    "Who",
    "Whose",
    "Whom",
    "Is",
    "Are",
    "Was",
    "Were",
    "Do",
    "Does",
    "Did",
    "Has",
    "Have",
    "Had",
    "Will",
    "Would",
    "Could",
    "Should",
    "Can",
    "May",
    "Might",
    "Must",
    "Being",
    "Been",
    "Having",
    "The",
    "This",
    "That",
    "These",
    "Those",
    "It",
    "Its",
    "Yes",
    "No",
    "Not",
    "Very",
    "Also",
    "Just",
    "Even",
    "Still",
    "January",
    "February",
    "March",
    "April",
    // "May" already listed above (line 1686)
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
    "National",
    "American",
    "European",
    "Asian",
    "African",
    "International",
    "Global",
    "Local",
    "Regional",
    "Western",
    "Eastern",
    "Northern",
    "Southern",
    "Answer",
    "Based",
    "According",
    "Since",
    "Because",
    "Although",
    "However",
    "Therefore",
    "Furthermore",
    "Moreover",
    "Nevertheless",
    "Likely",
    "Probably",
    "Certainly",
    "Actually",
    "Recently",
    "Today",
    "Yesterday",
    "Tomorrow",
];

/// Extract capitalized proper nouns (entity names) from a query string.
///
/// Skips sentence-initial words, stopwords, and words < 2 chars.
/// Also extracts possessives (e.g., "John's" -> "John").
pub(super) fn extract_query_entities(query: &str) -> Vec<String> {
    let mut entities = Vec::new();
    let mut seen = std::collections::HashSet::new();
    static STOPWORDS_SET: std::sync::LazyLock<std::collections::HashSet<&str>> =
        std::sync::LazyLock::new(|| ENTITY_STOPWORDS.iter().copied().collect());
    let stopwords = &*STOPWORDS_SET;

    let words: Vec<&str> = query.split_whitespace().collect();

    for (i, word) in words.iter().enumerate() {
        let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();

        if clean.len() < 2 {
            continue;
        }
        if stopwords.contains(clean.as_str()) {
            continue;
        }
        if i == 0 {
            continue;
        }
        if i > 0 {
            let prev = words[i - 1];
            if prev.ends_with('.') || prev.ends_with('?') || prev.ends_with('!') {
                continue;
            }
        }
        if clean.len() > 1
            && clean.chars().next().is_some_and(|c| c.is_uppercase())
            && clean.chars().skip(1).all(|c| c.is_lowercase())
            && seen.insert(clean.clone())
        {
            entities.push(clean);
        }
    }

    // Extract possessives: "Name's" -> "Name"
    for word in &words {
        if let Some(pos) = word.find("'s") {
            let name = &word[..pos];
            if name.len() >= 2
                && name.chars().next().is_some_and(|c| c.is_uppercase())
                && name.chars().skip(1).all(|c| c.is_lowercase())
                && !stopwords.contains(name)
                && seen.insert(name.to_string())
            {
                entities.push(name.to_string());
            }
        }
    }

    entities
}

/// Extract semantic topic keywords from a query, excluding entities and common words.
///
/// Returns up to 5 unique topic words that are 4+ chars and not in the skip list.
pub(super) fn extract_topic_keywords(query: &str, exclude_entities: &[String]) -> Vec<String> {
    let exclude_lower: std::collections::HashSet<String> =
        exclude_entities.iter().map(|e| e.to_lowercase()).collect();

    static SKIP_WORDS: std::sync::LazyLock<std::collections::HashSet<&str>> =
        std::sync::LazyLock::new(|| {
            [
                "what",
                "when",
                "where",
                "which",
                "who",
                "whom",
                "whose",
                "how",
                "why",
                "that",
                "this",
                "these",
                "those",
                "there",
                "here",
                "then",
                "than",
                "been",
                "being",
                "have",
                "having",
                "does",
                "doing",
                "done",
                "will",
                "would",
                "could",
                "should",
                "shall",
                "might",
                "must",
                "about",
                "after",
                "also",
                "another",
                "away",
                "back",
                "because",
                "before",
                "between",
                "both",
                "came",
                "come",
                "coming",
                "each",
                "even",
                "every",
                "first",
                "from",
                "gets",
                "give",
                "given",
                "goes",
                "going",
                "gone",
                "good",
                "great",
                "into",
                "just",
                "keep",
                "kind",
                "know",
                "known",
                "last",
                "left",
                "like",
                "liked",
                "likely",
                "long",
                "look",
                "made",
                "make",
                "making",
                "many",
                "more",
                "most",
                "much",
                "need",
                "never",
                "next",
                "once",
                "only",
                "other",
                "over",
                "part",
                "past",
                "people",
                "perhaps",
                "place",
                "point",
                "probably",
                "pursue",
                "quite",
                "rather",
                "really",
                "right",
                "said",
                "same",
                "seem",
                "show",
                "side",
                "since",
                "some",
                "something",
                "sometimes",
                "still",
                "such",
                "sure",
                "take",
                "tell",
                "them",
                "they",
                "thing",
                "things",
                "think",
                "time",
                "told",
                "took",
                "turn",
                "under",
                "upon",
                "used",
                "using",
                "very",
                "want",
                "well",
                "went",
                "were",
                "while",
                "with",
                "within",
                "without",
                "work",
                "year",
                "years",
                "your",
                "able",
                "already",
                "always",
                "around",
                "called",
                "certain",
                "during",
                "either",
                "else",
                "enough",
                "ever",
                "fact",
                "find",
                "found",
                "hard",
                "help",
                "high",
                "hold",
                "home",
                "however",
                "important",
                "include",
                "indeed",
                "inside",
                "instead",
                "itself",
                "large",
                "later",
                "least",
                "less",
                "life",
                "line",
                "little",
                "live",
                "lived",
                "lives",
                "living",
                "longer",
                "mean",
                "means",
                "mention",
                "mentioned",
                "name",
                "near",
                "number",
                "often",
                "open",
                "order",
                "others",
                "outside",
                "particular",
                "play",
                "possible",
                "prefer",
                "pretty",
                "provide",
                "real",
                "reason",
                "regarding",
                "remember",
                "result",
                "seems",
                "sense",
                "several",
                "simply",
                "small",
                "sort",
                "speak",
                "start",
                "state",
                "talk",
                "together",
                "true",
                "type",
                "types",
                "usually",
                "various",
                "ways",
            ]
            .iter()
            .copied()
            .collect()
        });
    let skip_words = &*SKIP_WORDS;

    let lower = query.to_lowercase();

    let mut topics = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut current_word = String::new();
    for ch in lower.chars() {
        if ch.is_ascii_lowercase() {
            current_word.push(ch);
        } else {
            if current_word.len() >= 4
                && !skip_words.contains(current_word.as_str())
                && !exclude_lower.contains(&current_word)
                && seen.insert(current_word.clone())
            {
                topics.push(current_word.clone());
                if topics.len() >= 5 {
                    return topics;
                }
            }
            current_word.clear();
        }
    }
    if current_word.len() >= 4
        && !skip_words.contains(current_word.as_str())
        && !exclude_lower.contains(&current_word)
        && seen.insert(current_word.clone())
    {
        topics.push(current_word);
    }

    topics
}

/// Generate decomposed sub-queries from extracted entities and topics.
///
/// Returns the original query first, then entity-based and topic-based variations.
/// Max 2 entities, max 3 topics per entity.
pub(super) fn generate_sub_queries(
    query: &str,
    entities: &[String],
    topics: &[String],
) -> Vec<String> {
    let mut queries = vec![query.to_string()];

    for entity in entities.iter().take(2) {
        queries.push(entity.clone());
        for topic in topics.iter().take(3) {
            queries.push(format!("{entity} {topic}"));
        }
        if topics.iter().any(|t| {
            matches!(
                t.as_str(),
                "career" | "jobs" | "work" | "occupation" | "employment"
            )
        }) {
            queries.push(format!("{entity} interests goals plans"));
        }
    }

    if entities.is_empty() && !topics.is_empty() {
        for topic in topics.iter().take(3) {
            queries.push(topic.clone());
        }
    }

    queries
}

/// Content fingerprint for dedup: first 320 chars, normalized, lowered, ASCII-only.
pub(super) fn content_fingerprint(content: &str) -> String {
    let cleaned: String = content
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace())
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > 320 {
        collapsed[..320].to_string()
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts5_query_empty_input() {
        assert_eq!(build_fts5_query(""), "\"\"");
        assert_eq!(build_fts5_query("   "), "\"\"");
    }

    #[test]
    fn fts5_query_single_token() {
        assert_eq!(build_fts5_query("database"), "\"database\"");
    }

    #[test]
    fn fts5_query_two_tokens_no_bigrams() {
        // Two tokens: bigrams would duplicate the full query, so skip them.
        assert_eq!(
            build_fts5_query("database connection"),
            "\"database\" OR \"connection\""
        );
    }

    #[test]
    fn fts5_query_three_tokens_with_bigrams() {
        let q = build_fts5_query("database connection pool");
        // Original tokens + bigrams, no synonyms for these words
        assert!(q.starts_with("\"database\" OR \"connection\" OR \"pool\""));
        assert!(q.contains("\"database connection\""));
        assert!(q.contains("\"connection pool\""));
    }

    #[test]
    fn fts5_query_four_tokens_with_bigrams() {
        // "the" is a stopword and gets filtered; remaining 3 tokens get bigrams
        // "quick" has synonyms: fast, rapid, swift
        let q = build_fts5_query("the quick brown fox");
        assert!(q.contains("\"quick\""));
        assert!(q.contains("\"brown\""));
        assert!(q.contains("\"fox\""));
        assert!(q.contains("\"quick brown\""));
        assert!(q.contains("\"brown fox\""));
        // Synonym expansion for "quick"
        assert!(q.contains("\"fast\""));
        assert!(q.contains("\"rapid\""));
        assert!(q.contains("\"swift\""));
    }

    #[test]
    fn fts5_query_special_chars_escaped() {
        // Double-quotes in tokens are escaped by doubling.
        assert_eq!(
            build_fts5_query("say \"hello\" world"),
            "\"say\" OR \"\"\"hello\"\"\" OR \"world\" \
             OR \"say \"\"hello\"\"\" OR \"\"\"hello\"\" world\""
        );
    }

    #[test]
    fn fts5_query_extra_whitespace_collapsed() {
        assert_eq!(
            build_fts5_query("  database   connection   pool  "),
            "\"database\" OR \"connection\" OR \"pool\" \
             OR \"database connection\" OR \"connection pool\""
        );
    }

    #[test]
    fn temporal_today() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("errors today", &now);
        assert_eq!(exp.cleaned_query, "errors");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-08"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-03-08T23:59:59"));
    }

    #[test]
    fn temporal_yesterday() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("what happened yesterday", &now);
        assert_eq!(exp.cleaned_query, "what happened");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-07"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-03-07T23:59:59"));
    }

    #[test]
    fn temporal_n_days_ago() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("decisions 3 days ago", &now);
        assert_eq!(exp.cleaned_query, "decisions");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-05"));
    }

    #[test]
    fn temporal_last_n_days() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("bugs last 7 days", &now);
        assert_eq!(exp.cleaned_query, "bugs");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn temporal_this_week() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(); // Sunday
        let exp = expand_temporal_query("decisions this week", &now);
        assert_eq!(exp.cleaned_query, "decisions");
        // March 8 2026 is Sunday, weekday=6 from Monday. Monday = March 2.
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-02"));
        assert!(exp.event_before.is_none()); // open-ended
    }

    #[test]
    fn temporal_last_week() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("errors last week", &now);
        assert_eq!(exp.cleaned_query, "errors");
        assert_eq!(exp.event_after.as_deref(), Some("2026-02-23"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-03-01T23:59:59"));
    }

    #[test]
    fn temporal_this_month() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let exp = expand_temporal_query("lessons this month", &now);
        assert_eq!(exp.cleaned_query, "lessons");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn temporal_last_month() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let exp = expand_temporal_query("bugs last month", &now);
        assert_eq!(exp.cleaned_query, "bugs");
        assert_eq!(exp.event_after.as_deref(), Some("2026-02-01"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-02-28T23:59:59"));
    }

    #[test]
    fn temporal_no_match() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("database connection pool", &now);
        assert_eq!(exp.cleaned_query, "database connection pool");
        assert!(exp.event_after.is_none());
        assert!(exp.event_before.is_none());
    }

    #[test]
    fn temporal_query_only_temporal() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("today", &now);
        // When only temporal phrase, keep original query for search
        assert_eq!(exp.cleaned_query, "today");
        assert!(exp.event_after.is_some());
    }

    #[test]
    fn temporal_past_n_weeks() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("errors past 2 weeks", &now);
        assert_eq!(exp.cleaned_query, "errors");
        assert_eq!(exp.event_after.as_deref(), Some("2026-02-22"));
    }

    // ── is_keyword_query tests ──────────────────────────────────────────

    #[test]
    fn keyword_backtick_code() {
        assert!(is_keyword_query("`FooBar`"));
        assert!(is_keyword_query("find `my_func` usage"));
    }

    #[test]
    fn keyword_file_path() {
        assert!(is_keyword_query("src/main.rs"));
        assert!(is_keyword_query("./config.toml"));
        assert!(is_keyword_query("~/projects/foo"));
        assert!(is_keyword_query("/usr/local/bin"));
    }

    #[test]
    fn keyword_camel_case() {
        assert!(is_keyword_query("SqliteStorage"));
        assert!(is_keyword_query("McpMemoryServer"));
        assert!(!is_keyword_query("sqlite"));
    }

    #[test]
    fn keyword_snake_case() {
        assert!(is_keyword_query("query_cache_key"));
        assert!(is_keyword_query("embed_batch"));
        assert!(!is_keyword_query("query"));
    }

    #[test]
    fn keyword_natural_language_not_keyword() {
        assert!(!is_keyword_query("what framework are we using"));
        assert!(!is_keyword_query("database connection pool"));
        assert!(!is_keyword_query(""));
    }

    // ── classify_query_intent tests ─────────────────────────────────────

    #[test]
    fn intent_keyword_backtick() {
        assert_eq!(classify_query_intent("`FooBar`"), QueryIntent::Keyword);
    }

    #[test]
    fn intent_keyword_file_path() {
        assert_eq!(classify_query_intent("src/main.rs"), QueryIntent::Keyword);
    }

    #[test]
    fn intent_keyword_camel_case() {
        assert_eq!(classify_query_intent("SqliteStorage"), QueryIntent::Keyword);
    }

    #[test]
    fn intent_factual_who() {
        assert_eq!(
            classify_query_intent("Who created the memory system?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_what() {
        assert_eq!(
            classify_query_intent("What database does the project use?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_when() {
        assert_eq!(
            classify_query_intent("When did we deploy the fix?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_where() {
        assert_eq!(
            classify_query_intent("Where is the config file stored?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_how_many() {
        assert_eq!(
            classify_query_intent("How many times has she visited?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_how_long() {
        assert_eq!(
            classify_query_intent("How long has the project been running?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_how_often() {
        assert_eq!(
            classify_query_intent("How often does she exercise?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_conceptual_why() {
        assert_eq!(
            classify_query_intent("Why did we choose SQLite?"),
            QueryIntent::Conceptual
        );
    }

    #[test]
    fn intent_conceptual_how() {
        assert_eq!(
            classify_query_intent("How does the scoring pipeline work?"),
            QueryIntent::Conceptual
        );
    }

    #[test]
    fn intent_conceptual_explain() {
        assert_eq!(
            classify_query_intent("Explain the RRF fusion strategy"),
            QueryIntent::Conceptual
        );
    }

    #[test]
    fn intent_conceptual_difference() {
        assert_eq!(
            classify_query_intent("What is the difference between FTS and vector search?"),
            QueryIntent::Conceptual
        );
    }

    #[test]
    fn intent_conceptual_compared_to() {
        assert_eq!(
            classify_query_intent("performance of SQLite compared to Postgres"),
            QueryIntent::Conceptual
        );
    }

    #[test]
    fn intent_general_statement() {
        assert_eq!(
            classify_query_intent("database connection pool"),
            QueryIntent::General
        );
    }

    #[test]
    fn intent_general_would() {
        assert_eq!(
            classify_query_intent("Would she want to pursue that career?"),
            QueryIntent::General
        );
    }

    #[test]
    fn intent_general_empty() {
        assert_eq!(classify_query_intent(""), QueryIntent::General);
    }

    // ── IntentProfile tests ──────────────────────────────────────────────

    #[test]
    fn intent_profile_keyword_disables_vec() {
        let profile = IntentProfile::for_intent(QueryIntent::Keyword);
        assert!((profile.vec_weight_mult - 0.0).abs() < 1e-9);
    }

    #[test]
    fn intent_profile_factual_boosts_fts() {
        let profile = IntentProfile::for_intent(QueryIntent::Factual);
        assert!(profile.fts_weight_mult > 1.0);
        assert!(profile.word_overlap_mult > 1.0);
    }

    #[test]
    fn intent_profile_conceptual_boosts_vec() {
        let profile = IntentProfile::for_intent(QueryIntent::Conceptual);
        assert!(profile.vec_weight_mult > 1.0);
        assert!(profile.fts_weight_mult < 1.0);
    }

    #[test]
    fn intent_profile_general_is_neutral() {
        let profile = IntentProfile::for_intent(QueryIntent::General);
        assert!((profile.vec_weight_mult - 1.0).abs() < 1e-9);
        assert!((profile.fts_weight_mult - 1.0).abs() < 1e-9);
        assert!((profile.word_overlap_mult - 1.0).abs() < 1e-9);
        assert!((profile.top_k_mult - 1.0).abs() < 1e-9);
    }

    // ── is_lock_error / retry_on_lock tests ────────────────────────────

    #[test]
    fn is_lock_error_detects_busy() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                extended_code: 5,
            },
            Some("database is locked".to_string()),
        );
        assert!(is_lock_error(&err));
    }

    #[test]
    fn is_lock_error_rejects_other_errors() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::ReadOnly,
                extended_code: 8,
            },
            None,
        );
        assert!(!is_lock_error(&err));

        let err2 = rusqlite::Error::QueryReturnedNoRows;
        assert!(!is_lock_error(&err2));
    }

    #[test]
    fn retry_on_lock_returns_immediately_for_non_lock_error() {
        let mut attempts = 0u32;
        let result: std::result::Result<(), rusqlite::Error> = retry_on_lock(|| {
            attempts += 1;
            Err(rusqlite::Error::QueryReturnedNoRows)
        });
        assert!(result.is_err());
        assert_eq!(attempts, 1, "should not retry for non-lock errors");
    }

    #[test]
    fn retry_on_lock_succeeds_on_first_try() {
        let mut attempts = 0u32;
        let result = retry_on_lock(|| {
            attempts += 1;
            Ok(42)
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 1);
    }

    #[test]
    fn retry_on_lock_retries_on_busy_then_succeeds() {
        let mut attempts = 0u32;
        let result = retry_on_lock(|| {
            attempts += 1;
            if attempts < 3 {
                Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error {
                        code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                        extended_code: 5,
                    },
                    None,
                ))
            } else {
                Ok("success")
            }
        });
        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempts, 3);
    }

    #[test]
    fn retry_on_lock_exhausts_retries() {
        let mut attempts = 0u32;
        let result: std::result::Result<(), rusqlite::Error> = retry_on_lock(|| {
            attempts += 1;
            Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error {
                    code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                    extended_code: 5,
                },
                None,
            ))
        });
        assert!(result.is_err());
        assert_eq!(
            attempts, 5,
            "should attempt exactly RETRY_MAX_ATTEMPTS times"
        );
    }

    // ── FTS5 stopword filtering tests ─────────────────────────────────

    #[test]
    fn fts5_query_filters_stopwords() {
        // "to" and "the" are stopwords; only "path" and "database" should remain
        assert_eq!(
            build_fts5_query("path to the database"),
            "\"path\" OR \"database\""
        );
    }

    #[test]
    fn fts5_query_all_stopwords_fallback() {
        // When all tokens are stopwords, fall back to original tokens (with bigrams for 3+)
        assert_eq!(
            build_fts5_query("is it the"),
            "\"is\" OR \"it\" OR \"the\" OR \"is it\" OR \"it the\""
        );
    }

    #[test]
    fn fts5_query_stopwords_with_bigrams() {
        // "how to deploy the application" → stopwords "how", "to", "the" removed
        // → "deploy", "application" (2 tokens, no bigrams, no synonyms)
        assert_eq!(
            build_fts5_query("how to deploy the application"),
            "\"deploy\" OR \"application\""
        );
    }

    // ── Synonym expansion tests ──────────────────────────────────────

    #[test]
    fn synonym_map_bidirectional() {
        // Every word in a synonym group should map back to the others
        let groups: &[&[&str]] = &[
            &["buy", "purchase", "bought"],
            &["movie", "film"],
            &["doctor", "physician", "dr"],
            &["car", "automobile", "vehicle"],
            &["house", "home", "residence"],
            &["dog", "puppy", "canine", "pup"],
        ];
        for group in groups {
            for &word in *group {
                let syns = get_synonyms(word);
                assert!(
                    !syns.is_empty(),
                    "get_synonyms({word:?}) returned empty slice"
                );
                // Every other word in the group should be a synonym
                for &other in *group {
                    if other == word {
                        continue;
                    }
                    assert!(
                        syns.contains(&other),
                        "get_synonyms({word:?}) missing {other:?}, got {syns:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn synonym_map_no_match() {
        assert!(get_synonyms("database").is_empty());
        assert!(get_synonyms("connection").is_empty());
        assert!(get_synonyms("xyzzy").is_empty());
    }

    #[test]
    fn fts5_query_single_token_with_synonym() {
        // "movie" has synonym "film"
        let q = build_fts5_query("movie");
        assert!(q.contains("\"movie\""), "missing original: {q}");
        assert!(q.contains("\"film\""), "missing synonym 'film': {q}");
    }

    #[test]
    fn fts5_query_two_tokens_with_synonyms() {
        // "buy car" → "buy" has synonyms (purchase, bought), "car" has synonyms (automobile, vehicle)
        let q = build_fts5_query("buy car");
        assert!(q.contains("\"buy\""), "missing original 'buy': {q}");
        assert!(q.contains("\"car\""), "missing original 'car': {q}");
        assert!(
            q.contains("\"purchase\""),
            "missing synonym 'purchase': {q}"
        );
        assert!(q.contains("\"bought\""), "missing synonym 'bought': {q}");
        assert!(
            q.contains("\"automobile\""),
            "missing synonym 'automobile': {q}"
        );
        assert!(q.contains("\"vehicle\""), "missing synonym 'vehicle': {q}");
    }

    #[test]
    fn fts5_query_synonym_cap_respected() {
        // "phone" has 3 synonyms: telephone, mobile, cell — all should appear (cap=3)
        let q = build_fts5_query("phone");
        assert!(q.contains("\"phone\""));
        // Count synonym terms (all should be present since cap is 3)
        let synonym_count = ["telephone", "mobile", "cell"]
            .iter()
            .filter(|s| q.contains(&format!("\"{s}\"")))
            .count();
        assert!(
            synonym_count <= SYNONYM_CAP,
            "got {synonym_count} synonyms, expected at most {SYNONYM_CAP}"
        );
    }

    #[test]
    fn fts5_query_no_synonym_for_non_synonym_words() {
        // "database" has no synonyms, query should be unchanged
        assert_eq!(build_fts5_query("database"), "\"database\"");
    }

    #[test]
    fn fts5_query_synonym_deduplication() {
        // Synonyms whose stems match the original token should be skipped.
        // This is a general property test — no synonym should produce a
        // quoted term identical to the original token.
        let q = build_fts5_query("help");
        // "help" synonyms: assist, support, aid — all different stems
        assert!(q.contains("\"help\""));
        assert!(q.contains("\"assist\""));
        assert!(q.contains("\"support\""));
        assert!(q.contains("\"aid\""));
    }

    #[test]
    fn fts5_query_synonym_with_bigrams() {
        // 3+ tokens where some have synonyms
        // "buy fast car" → "buy"(purchase,bought), "fast"(quick,rapid,swift), "car"(automobile,vehicle)
        let q = build_fts5_query("buy fast car");
        // Original tokens
        assert!(q.contains("\"buy\""), "missing 'buy': {q}");
        assert!(q.contains("\"fast\""), "missing 'fast': {q}");
        assert!(q.contains("\"car\""), "missing 'car': {q}");
        // Bigrams
        assert!(q.contains("\"buy fast\""), "missing bigram 'buy fast': {q}");
        assert!(q.contains("\"fast car\""), "missing bigram 'fast car': {q}");
        // Synonyms
        assert!(
            q.contains("\"purchase\""),
            "missing synonym 'purchase': {q}"
        );
        assert!(q.contains("\"quick\""), "missing synonym 'quick': {q}");
        assert!(
            q.contains("\"automobile\""),
            "missing synonym 'automobile': {q}"
        );
    }

    #[test]
    fn test_extract_entities_from_tags() {
        let tags = vec![
            "entity:people:alice".to_string(),
            "entity:tools:react".to_string(),
            "locomo-test".to_string(),
            "session:1".to_string(),
        ];
        let entities = extract_entities_from_tags(&tags);
        assert_eq!(entities.len(), 2);
        assert!(entities.contains(&"entity:people:alice".to_string()));
        assert!(entities.contains(&"entity:tools:react".to_string()));
    }

    #[test]
    fn test_extract_entities_from_tags_empty() {
        let tags: Vec<String> = vec!["locomo-test".to_string()];
        let entities = extract_entities_from_tags(&tags);
        assert!(entities.is_empty());
    }

    #[test]
    fn fts5_query_stopword_synonym_interaction() {
        // Stopwords are filtered first; synonyms only apply to non-stopwords
        // "the movie" → "the" filtered → "movie" with synonym "film"
        let q = build_fts5_query("the movie");
        assert!(q.contains("\"movie\""));
        assert!(q.contains("\"film\""));
        assert!(!q.contains("\"the\""));
    }

    #[test]
    fn test_dynamic_limit_mult_multi_hop() {
        assert!(
            (detect_dynamic_limit_mult("What is Amanda's sister and how are they related") - 2.0)
                .abs()
                < 1e-9
        );
        assert!(
            (detect_dynamic_limit_mult("Tell me about the relationship between Alice and Bob")
                - 2.0)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn test_dynamic_limit_mult_temporal() {
        assert!((detect_dynamic_limit_mult("What happened last month") - 1.5).abs() < 1e-9);
        assert!((detect_dynamic_limit_mult("Events over the past year") - 1.5).abs() < 1e-9);
    }

    #[test]
    fn test_dynamic_limit_mult_simple() {
        assert!((detect_dynamic_limit_mult("What is Alice's job?") - 1.0).abs() < 1e-9);
        assert!((detect_dynamic_limit_mult("Tell me about the weather") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_intent_profile_suggested_limit_mult() {
        let p = IntentProfile::for_intent(QueryIntent::Conceptual);
        assert!((p.suggested_limit_mult - 1.3).abs() < 1e-9);
        let p = IntentProfile::for_intent(QueryIntent::General);
        assert!((p.suggested_limit_mult - 1.0).abs() < 1e-9);
    }

    // ── extract_query_entities tests ───────────────────────────────────

    #[test]
    fn test_extract_query_entities() {
        let entities = extract_query_entities("Would Caroline pursue writing?");
        assert!(
            entities.contains(&"Caroline".to_string()),
            "expected Caroline in {entities:?}"
        );
        assert!(
            !entities.contains(&"Would".to_string()),
            "should skip question word Would"
        );
    }

    #[test]
    fn test_extract_query_entities_possessive() {
        let entities = extract_query_entities("What is Amanda's sister's career?");
        assert!(
            entities.contains(&"Amanda".to_string()),
            "expected Amanda in {entities:?}"
        );
    }

    // ── extract_topic_keywords tests ───────────────────────────────────

    #[test]
    fn test_extract_topic_keywords() {
        let topics = extract_topic_keywords(
            "Would Caroline pursue writing as a career?",
            &["Caroline".to_string()],
        );
        assert!(
            topics.contains(&"writing".to_string()),
            "expected 'writing' in {topics:?}"
        );
        assert!(
            topics.contains(&"career".to_string()),
            "expected 'career' in {topics:?}"
        );
        assert!(
            !topics.contains(&"caroline".to_string()),
            "should exclude entity 'caroline'"
        );
    }

    // ── generate_sub_queries tests ─────────────────────────────────────

    #[test]
    fn test_generate_sub_queries() {
        let queries = generate_sub_queries(
            "Would Caroline pursue writing?",
            &["Caroline".to_string()],
            &["writing".to_string()],
        );
        assert_eq!(queries[0], "Would Caroline pursue writing?");
        assert!(queries.contains(&"Caroline".to_string()));
        assert!(queries.contains(&"Caroline writing".to_string()));
    }

    #[test]
    fn test_generate_sub_queries_no_entities() {
        let queries = generate_sub_queries("Tell me about writing", &[], &["writing".to_string()]);
        assert_eq!(queries[0], "Tell me about writing");
        assert!(queries.contains(&"writing".to_string()));
    }
}
