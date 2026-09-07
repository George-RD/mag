#![cfg(feature = "real-embeddings")]

// These tests exercise the native ONNX Runtime without downloading a model.
// Compiling the production adapters also guards all six anyhow boundaries.

#[test]
fn production_cpu_builder_settings_are_accepted() -> anyhow::Result<()> {
    let cpu_ep = ort::ep::CPU::default().with_arena_allocator(false).build();
    let _builder = ort::session::Session::builder()?
        .with_execution_providers([cpu_ep])
        .map_err(ort::Error::<()>::from)?
        .with_intra_threads(num_cpus::get())
        .map_err(ort::Error::<()>::from)?
        .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
        .map_err(ort::Error::<()>::from)?;
    Ok(())
}

#[test]
fn recoverable_builder_errors_retain_ort_diagnostics_in_anyhow() -> anyhow::Result<()> {
    // An interior NUL is rejected before the log ID can reach the C API.
    let Err(original) = ort::session::Session::builder()?.with_log_id("mag\0invalid") else {
        panic!("an ONNX log ID with an interior NUL must fail");
    };
    let expected_code = original.code();
    let expected_message = original.to_string();

    let error = anyhow::Error::new(ort::Error::<()>::from(original))
        .context("failed to configure ONNX session");
    let preserved = error
        .downcast_ref::<ort::Error<()>>()
        .expect("the typed ORT error must survive conversion and context");

    assert_eq!(preserved.code(), expected_code);
    assert_eq!(preserved.message(), expected_message);
    assert_eq!(error.to_string(), "failed to configure ONNX session");
    Ok(())
}
