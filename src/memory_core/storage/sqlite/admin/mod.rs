//! Admin sub-modules for the SQLite storage backend.
//!
//! This module aggregates four logical areas of administrative functionality:
//!
//! - [`backup`] — database snapshot creation, rotation, listing, and restore
//! - [`maintenance`] — health checks, consolidation, compaction, and session clearing
//! - [`welcome`] — session-start context assembly (`WelcomeProvider`)
//! - [`stats`] — aggregated analytics (`StatsProvider`)
//!
//! All impl blocks target [`super::SqliteStorage`] and are kept in separate
//! files purely for readability — the public API is unchanged.

mod backup;
mod maintenance;
mod stats;
mod welcome;
