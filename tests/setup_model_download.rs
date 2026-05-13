// tests/setup_model_download.rs

#![cfg(feature = "real-embeddings")]

use mag::test_helpers::with_temp_home;

/// When models are already present, model_dir() should resolve correctly.
#[test]
fn setup_skips_download_when_models_present() {
    with_temp_home(|home| {
        let model_dir = home
            .join(".mag")
            .join("models")
            .join("bge-small-en-v1.5-int8");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.onnx"), "dummy").unwrap();
        std::fs::write(model_dir.join("tokenizer.json"), "dummy").unwrap();

        let resolved = mag::memory_core::embedder::model_dir().unwrap();
        assert!(resolved.ends_with("bge-small-en-v1.5-int8"));
        assert!(resolved.join("model.onnx").exists());
    });
}

/// When models are missing, model_dir() should still resolve the expected path.
#[test]
fn setup_resolves_model_dir_even_when_missing() {
    with_temp_home(|_home| {
        let model_dir = mag::memory_core::embedder::model_dir().unwrap();
        assert!(model_dir.ends_with("bge-small-en-v1.5-int8"));
        assert!(!model_dir.join("model.onnx").exists());
    });
}

/// cross_encoder_model_dir() should resolve correctly under temp HOME.
#[test]
fn setup_resolves_cross_encoder_dir() {
    with_temp_home(|_home| {
        let ce_dir = mag::memory_core::reranker::cross_encoder_model_dir().unwrap();
        assert!(ce_dir.ends_with("ms-marco-MiniLM-L-6-v2"));
    });
}
