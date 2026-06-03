//! AgentMemoryHarness — minimal three-method interface between agents and MAG.
//!
//! This trait is intentionally small (observe / retrieve_context / trace) so
//! that benchmarks and agent loops can swap storage implementations without
//! changing their orchestration code.
//!
//! # Phase 2a scope
//!
//! - Trait definition only.
//! - No in-process implementations yet (those ship with Phase 2b MemoryArena
//!   integration).
//! - The AMA-Bench adapter uses the Python-side harness via MCP stdio.

use anyhow::Result;
use serde_json::Value;

/// A single turn in an agent trajectory.
///
/// All fields are optional — callers pass whatever is relevant for their
/// domain (software engineering, web navigation, embodied AI, etc.).
#[derive(Debug, Clone, Default)]
pub struct AgentTurn {
    /// Turn index within the episode.
    pub turn_idx: usize,
    /// Agent action (e.g. tool call, code edit, navigation command).
    pub action: String,
    /// Environment observation (e.g. tool output, test result, page HTML).
    pub observation: String,
    /// Unstructured metadata (timestamps, tool names, file paths, etc.).
    pub metadata: Value,
}

impl AgentTurn {
    /// Build a turn from action + observation text.
    pub fn new(turn_idx: usize, action: impl Into<String>, observation: impl Into<String>) -> Self {
        Self {
            turn_idx,
            action: action.into(),
            observation: observation.into(),
            metadata: Value::Null,
        }
    }

    /// Attach metadata and return self for chaining.
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Serialise to the AMA-Bench trajectory text format.
    pub fn to_text(&self) -> String {
        let mut parts = vec![format!("Step {}:", self.turn_idx)];
        if !self.action.is_empty() {
            parts.push(format!("Action: {}", self.action));
        }
        if !self.observation.is_empty() {
            parts.push(format!("Observation: {}", self.observation));
        }
        parts.join("\n")
    }
}

/// Minimal harness between an agent loop and a memory backend.
///
/// Implementations may use in-process storage (e.g. `SqliteStorage`) or
/// communicate with a remote MAG server via MCP.
pub trait AgentMemoryHarness: Send + Sync {
    /// Ingest a single turn into memory.
    fn observe(&self, turn: &AgentTurn) -> Result<()>;

    /// Retrieve relevant context for a query.
    ///
    /// Callers compose the returned string into their prompts — the harness
    /// does not perform injection itself.
    fn retrieve_context(&self, query: &str) -> Result<String>;

    /// Emit an unstructured JSON trace event.
    ///
    /// Events are appended to `~/.mag/traces/<run-id>.jsonl` by typical
    /// implementations, but the trait does not prescribe a sink.
    fn trace(&self, event: Value) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_turn_to_text() {
        let turn = AgentTurn::new(3, "Edit src/main.rs", "Added println! for debugging");
        let text = turn.to_text();
        assert!(text.contains("Step 3:"));
        assert!(text.contains("Action: Edit src/main.rs"));
        assert!(text.contains("Observation: Added println! for debugging"));
    }

    #[test]
    fn agent_turn_empty_fields_omitted() {
        let turn = AgentTurn::new(0, "", "hello");
        let text = turn.to_text();
        assert!(text.contains("Step 0:"));
        assert!(!text.contains("Action:"));
        assert!(text.contains("Observation: hello"));
    }
}
