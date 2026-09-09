//! Stdio/configuration adapter; the selected local runtime owns the workflow.

use std::io::{Read, Write};

use anyhow::{Result, ensure};
use mag::memory_core::llm::{LlmConfig, build_llm_backend};
use mag::{
    INTELLIGENCE_PROMPT_VERSION, IntelligenceRequest, LocalMemoryRuntime, MAX_INTELLIGENCE_BYTES,
    intelligence_output_schema,
};
use serde_json::json;

use crate::cli::IntelligenceProducerArgs;

impl IntelligenceProducerArgs {
    fn config(&self) -> Result<LlmConfig> {
        let url = reqwest::Url::parse(&self.base_url)
            .map_err(|_| anyhow::anyhow!("invalid intelligence endpoint URL"))?;
        ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "intelligence endpoint must be HTTP(S) without credentials, query or fragment"
        );
        ensure!(!self.model.trim().is_empty(), "model must not be empty");
        let mut config = LlmConfig::local_default();
        config.base_url = Some(url.as_str().trim_end_matches('/').to_string());
        config.model.clone_from(&self.model);
        config.timeout_secs = self.timeout_seconds;
        config.max_tokens = self.max_tokens;
        config.concurrency_limit = Some(1);
        Ok(config)
    }
}

/// Reads exactly one request and writes its unrepaired completion to stdout.
pub async fn run(args: &IntelligenceProducerArgs) -> Result<()> {
    let config = args.config()?;
    if args.describe {
        let description = json!({
            "model_profile": {
                "role": "generation",
                "runtime": "openai_compatible_http",
                "model": config.model,
                "base_url": config.base_url,
                "temperature": config.temperature,
                "max_tokens": config.max_tokens,
                "timeout_secs": config.timeout_secs,
                "concurrency_limit": 1,
                "prompt_version": INTELLIGENCE_PROMPT_VERSION,
                "output_mode": if args.json_schema { "json_schema" } else { "unconstrained" },
                "output_schema": args.json_schema.then(intelligence_output_schema),
                "verification": "configured_not_authenticated",
                "revision": null,
                "checksums": null,
                "quantization": null,
                "licence": null,
                "output_transformations": ["existing provider trims surrounding whitespace"]
            },
            "embedding_space_identity": null
        });
        println!("{}", serde_json::to_string(&description)?);
        return Ok(());
    }
    let mut input = Vec::new();
    std::io::stdin()
        .take(u64::try_from(MAX_INTELLIGENCE_BYTES)? + 1)
        .read_to_end(&mut input)?;
    ensure!(
        input.len() <= MAX_INTELLIGENCE_BYTES,
        "intelligence request exceeds byte limit"
    );
    let request: IntelligenceRequest = serde_json::from_slice(&input).map_err(|_| {
        anyhow::anyhow!("invalid intelligence request; expected answer-blind protocol v1 JSON")
    })?;
    let backend = build_llm_backend(config)
        .map_err(|_| anyhow::anyhow!("could not initialize intelligence backend"))?;
    let completion = if args.json_schema {
        LocalMemoryRuntime::produce_intelligence_with_schema(backend.as_ref(), &request).await?
    } else {
        LocalMemoryRuntime::produce_intelligence(backend.as_ref(), &request).await?
    };
    std::io::stdout().lock().write_all(completion.as_bytes())?;
    Ok(())
}
