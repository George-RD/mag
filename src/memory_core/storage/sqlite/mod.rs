use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[cfg(test)]
use crate::memory_core::embedder::Embedder;
use crate::memory_core::{
    AdvancedSearcher, CheckpointInput, CheckpointManager, Deleter, EventType, ExpirationSweeper,
    FeedbackRecorder, GraphNode, GraphTraverser, LessonQuerier, ListResult, Lister,
    MaintenanceManager, MemoryInput, MemoryUpdate, PhraseSearcher, ProfileManager, Recents,
    Relationship, RelationshipQuerier, ReminderManager, Retriever, ScoringParams, SearchOptions,
    SearchResult, Searcher, SemanticResult, SemanticSearcher, SimilarFinder, StatsProvider,
    Storage, Tagger, Updater, VersionChainQuerier, WelcomeProvider, is_stopword,
    jaccard_similarity, simple_stem,
};

use cache::{CachedQuery, QUERY_CACHE_TTL_SECS};

/// Cosine similarity threshold for auto-supersession detection (primary signal).
/// Semantic similarity catches updates even when wording changes significantly.
const SUPERSESSION_COSINE_THRESHOLD: f32 = 0.70;

/// Jaccard similarity threshold for auto-supersession detection (secondary signal).
/// Ensures topical overlap — prevents cross-topic false matches from cosine alone.
/// Must be well below dedup thresholds (0.80-0.85).
const SUPERSESSION_JACCARD_THRESHOLD: f64 = 0.30;

// ── Submodule declarations ──────────────────────────────────────────────

mod admin;
mod advanced;
mod cache;
mod conn_pool;
mod crud;
mod embedding_codec;
mod entities;
mod graph;
mod helpers;
mod hot_cache;
mod hot_cache_mgmt;
mod io;
mod lifecycle;
mod nlp;
pub(super) mod pipeline;
mod query_classifier;
mod relationships;
mod schema;
mod search;
mod session;
mod storage;
mod temporal;

#[cfg(test)]
mod tests;

// ── Re-exports ──────────────────────────────────────────────────────────

use storage::StoreOutcome;
pub use storage::{InitMode, RankedSemanticCandidate, SqliteStorage};

use conn_pool::retry_on_lock;
pub(crate) use embedding_codec::dot_product;
use embedding_codec::{decode_embedding, encode_embedding};
#[cfg(test)]
use helpers::normalize_for_dedup;
use helpers::{
    EPOCH_FALLBACK, append_search_filters, build_fts5_query, canonical_hash, content_hash,
    escape_like_pattern, event_type_from_sql, event_type_to_sql, parse_metadata_from_db,
    parse_tags_from_db, query_cache_key, search_result_from_row, to_param_refs, validate_iso8601,
};
#[cfg(test)]
use hot_cache::{HOT_CACHE_REFRESH_SECS, HotTierCache};
use temporal::expand_temporal_query;

#[cfg(feature = "sqlite-vec")]
use helpers::{
    hydrate_memories_by_ids, vec_delete, vec_distance_to_similarity, vec_knn_search, vec_upsert,
};

#[cfg(feature = "sqlite-vec")]
use storage::ensure_vec_extension_registered;
