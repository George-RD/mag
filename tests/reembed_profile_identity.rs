#![cfg(feature = "real-embeddings")]
use anyhow::Result;
use mag::memory_core::OnnxEmbedder;
use mag::memory_core::embedding_model::bge_small_en_v1_5_embedding_model;
use std::sync::Arc;
#[test]
fn custom_same_dimension_model_cannot_claim_the_pinned_bge_identity() -> Result<()> {
    let custom = OnnxEmbedder::with_model(
        "custom-384",
        "https://example.invalid/model.onnx",
        "https://example.invalid/tokenizer.json",
        384,
        "last_hidden_state",
    )?;
    let result = bge_small_en_v1_5_embedding_model(Arc::new(custom));
    let error = result
        .err()
        .expect("unverified custom model must not claim BGE identity");
    assert!(error.to_string().contains("pinned BGE"), "{error:#}");
    Ok(())
}
#[test]
fn default_model_can_claim_the_pinned_bge_identity_without_loading_artifacts() -> Result<()> {
    let model = bge_small_en_v1_5_embedding_model(Arc::new(OnnxEmbedder::new()?))?;
    let profile = model
        .model_profile()
        .expect("default model has a pinned profile");
    assert_eq!(
        profile.embedding_space_identity(),
        model.embedding_space_identity()
    );
    Ok(())
}
