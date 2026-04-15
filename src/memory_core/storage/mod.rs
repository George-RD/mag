/// In-memory HashMap-backed storage implementation.
pub mod memory;
/// SQLite-backed storage implementation.
pub mod sqlite;

pub use sqlite::{InitMode, SqliteStorage};
