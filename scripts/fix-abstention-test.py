#!/usr/bin/env python3
"""Make the unrelated-query abstention test use meaningful controlled vectors."""

from pathlib import Path

path = Path(__file__).resolve().parents[1] / "src/memory_core/storage/sqlite/tests.rs"
text = path.read_text(encoding="utf-8")

embedder_anchor = '''impl Embedder for KeywordEmbedder {
    fn dimension(&self) -> usize {
        4
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        if text.contains("alpha") {
            Ok(vec![1.0, 0.0, 0.0, 0.0])
        } else if text.contains("beta") {
            Ok(vec![0.993_883_7, 0.110_431_53, 0.0, 0.0]) // L2-normalized [0.9, 0.1, 0, 0]
        } else {
            Ok(vec![0.0, 0.0, 1.0, 0.0])
        }
    }
}
'''
embedder_replacement = embedder_anchor + '''
#[derive(Debug, Clone)]
struct TopicEmbedder;

impl Embedder for TopicEmbedder {
    fn dimension(&self) -> usize {
        4
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let text = text.to_ascii_lowercase();
        if ["rust", "tokio", "sqlite"]
            .iter()
            .any(|topic| text.contains(topic))
        {
            Ok(vec![1.0, 0.0, 0.0, 0.0])
        } else if ["french", "pastry", "croissant"]
            .iter()
            .any(|topic| text.contains(topic))
        {
            Ok(vec![0.0, 1.0, 0.0, 0.0])
        } else {
            Ok(vec![0.0, 0.0, 1.0, 0.0])
        }
    }
}
'''
if "struct TopicEmbedder" not in text:
    if embedder_anchor not in text:
        raise SystemExit("KeywordEmbedder anchor not found")
    text = text.replace(embedder_anchor, embedder_replacement, 1)

old_test = '''async fn test_abstention_gate_fires_for_unrelated_query() {
    // Store memories about Rust programming, then query about something
    // completely unrelated (French pastry). The abstention gate should
    // fire because max text_overlap will be well below the 0.30 threshold.
    let storage = SqliteStorage::new_in_memory().unwrap();'''
new_test = '''async fn test_abstention_gate_fires_for_unrelated_query() {
    // Store memories about Rust programming, then query about something
    // completely unrelated (French pastry). Use controlled vectors here:
    // PlaceholderEmbedder is a hash fixture, not a semantic model, and its
    // all-positive vectors can create accidental high-cosine rescue matches.
    let storage = SqliteStorage::new_in_memory_with_embedder(std::sync::Arc::new(TopicEmbedder))
        .unwrap();'''
if old_test in text:
    text = text.replace(old_test, new_test, 1)
elif new_test not in text:
    raise SystemExit("abstention test anchor not found")

path.write_text(text, encoding="utf-8")
