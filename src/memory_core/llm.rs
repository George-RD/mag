//! Optional LLM backend for extraction, reflection, and E2E answer generation.
//!
//! Gated by the `llm` feature. When disabled, MAG falls back to rule-based
//! extraction and retrieval-only answering.
//!
//! Providers: OpenAI, Anthropic, and local OpenAI-compatible runtimes.
//! The local-first default profile is LFM2.5 1.2B Instruct. Direct in-process
//! ONNX causal generation is a planned backend; embeddings already run in ONNX.
#![allow(dead_code)]
// Dead-code allowed: this is new infrastructure (Phase 1). Public APIs will be
// consumed by Phase 2 (extraction) and Phase 3 (reflection). Removing this
// directive is part of the Phase 2 delivery checklist.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Default model exposed by the current local OpenAI-compatible transport.
///
/// This is the Ollama model identifier for Liquid AI's LFM2.5 1.2B Instruct
/// checkpoint. It is intentionally small enough for ordinary modern laptops.
pub const DEFAULT_LOCAL_LLM_MODEL: &str = "LiquidAI/lfm2.5-1.2b-instruct";
/// Default endpoint for a local OpenAI-compatible runtime.
pub const DEFAULT_LOCAL_LLM_BASE_URL: &str = "http://localhost:11434/v1";
/// Target checkpoint for the planned in-process ONNX causal-LM backend.
pub const TARGET_ONNX_LOCAL_LLM_MODEL: &str = "LiquidAI/LFM2.5-1.2B-Instruct-ONNX";
/// Future speed candidate. Do not route production tasks here without eval parity.
pub const EXPERIMENTAL_SMALL_ONNX_LLM_MODEL: &str = "LiquidAI/LFM2.5-350M-ONNX";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Which LLM provider to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum LlmProvider {
    OpenAI,
    Anthropic,
    Ollama,
}

/// Runtime configuration for the LLM backend.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_concurrency_limit_opt")]
    pub concurrency_limit: Option<usize>,
}

fn default_timeout_secs() -> u64 {
    60
}
fn default_max_tokens() -> u32 {
    512
}
fn default_temperature() -> f32 {
    0.0
}
fn default_concurrency_limit() -> usize {
    4
}
fn default_concurrency_limit_opt() -> Option<usize> {
    Some(4)
}

impl LlmConfig {
    /// Load from environment variables with `MAG_LLM_` prefix.
    ///
    /// Variables:
    ///   MAG_LLM_PROVIDER    → openai | anthropic | ollama
    ///   MAG_LLM_MODEL       → model name (defaults to LFM2.5 1.2B for ollama)
    ///   MAG_LLM_API_KEY     → API key (optional for local)
    ///   MAG_LLM_BASE_URL    → custom endpoint (optional)
    ///   MAG_LLM_TIMEOUT     → timeout in seconds (default 60)
    ///   MAG_LLM_MAX_TOKENS  → max tokens (default 512)
    ///   MAG_LLM_CONCURRENCY → max concurrent requests (default 4)
    pub fn from_env() -> Option<Self> {
        let provider = std::env::var("MAG_LLM_PROVIDER").ok()?;
        let provider = match provider.to_lowercase().as_str() {
            "openai" => LlmProvider::OpenAI,
            "anthropic" => LlmProvider::Anthropic,
            "ollama" => LlmProvider::Ollama,
            _ => return None,
        };
        let model = match std::env::var("MAG_LLM_MODEL") {
            Ok(model) => model,
            Err(_) if provider == LlmProvider::Ollama => DEFAULT_LOCAL_LLM_MODEL.to_string(),
            Err(_) => return None,
        };
        let api_key = std::env::var("MAG_LLM_API_KEY").ok();
        let base_url = std::env::var("MAG_LLM_BASE_URL").ok();
        let timeout_secs = std::env::var("MAG_LLM_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(default_timeout_secs);
        let max_tokens = std::env::var("MAG_LLM_MAX_TOKENS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(default_max_tokens);
        let temperature = std::env::var("MAG_LLM_TEMPERATURE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(default_temperature);
        let concurrency_limit = std::env::var("MAG_LLM_CONCURRENCY")
            .ok()
            .and_then(|s| s.parse().ok());
        Some(Self {
            provider,
            model,
            api_key,
            base_url,
            timeout_secs,
            max_tokens,
            temperature,
            concurrency_limit,
        })
    }
    /// Load explicit environment configuration, otherwise use the local-first
    /// LFM2.5 1.2B profile.
    pub fn from_env_or_local_default() -> Self {
        Self::from_env().unwrap_or_default()
    }

    /// Default OpenAI configuration.
    pub fn openai(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            provider: LlmProvider::OpenAI,
            model: model.into(),
            api_key: Some(api_key.into()),
            base_url: None,
            timeout_secs: default_timeout_secs(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            concurrency_limit: None,
        }
    }

    /// Default local/Ollama configuration.
    pub fn ollama(model: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            provider: LlmProvider::Ollama,
            model: model.into(),
            api_key: None,
            base_url: Some(base_url.into()),
            timeout_secs: default_timeout_secs(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            concurrency_limit: None,
        }
    }

    /// Local-first default: LFM2.5 1.2B Instruct through an OpenAI-compatible
    /// runtime. This is the temporary transport until direct ONNX generation is
    /// implemented behind the same `LlmBackend` boundary.
    pub fn local_default() -> Self {
        let mut config = Self::ollama(DEFAULT_LOCAL_LLM_MODEL, DEFAULT_LOCAL_LLM_BASE_URL);
        config.temperature = 0.1;
        config
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self::local_default()
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Abstraction over LLM providers for generation and structured extraction.
#[async_trait]
pub trait LlmBackend: Send + Sync {
    /// Generate a text completion for the given prompt.
    async fn complete(&self, prompt: &str, system: Option<&str>) -> Result<String>;
    /// Generate a structured completion as JSON.
    ///
    /// The default implementation asks the model to emit JSON and then
    /// returns the parsed Value. Providers with native structured-output APIs
    /// (e.g. OpenAI `response_format`) may override this for better
    /// reliability.
    async fn complete_structured(
        &self,
        prompt: &str,
        system: Option<&str>,
        _schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let json_prompt = format!(
            "{prompt}\n\nYou must respond with valid JSON only. \
             Do not include markdown code fences, explanatory text, or comments."
        );
        let raw = self.complete(&json_prompt, system).await?;
        // Strip markdown fences if the model ignored the instruction.
        let cleaned = raw
            .trim()
            .strip_prefix("```json")
            .or_else(|| raw.trim().strip_prefix("```"))
            .map(|s| s.strip_suffix("```").unwrap_or(s).trim())
            .unwrap_or(&raw);
        serde_json::from_str(cleaned).context("LLM structured response parse error")
    }
}

// ---------------------------------------------------------------------------
// Shared client
// ---------------------------------------------------------------------------

/// Shared HTTP client configuration for LLM providers.
pub struct LlmClient {
    pub config: LlmConfig,
    pub http: reqwest::Client,
    /// Bounded concurrency semaphore. Defaults to 4 concurrent requests.
    pub semaphore: std::sync::Arc<tokio::sync::Semaphore>,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .context("failed to build LLM HTTP client")?;
        let permits = config
            .concurrency_limit
            .unwrap_or_else(default_concurrency_limit);
        let permits = permits.max(1);
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(permits));
        Ok(Self {
            config,
            http,
            semaphore,
        })
    }

    pub fn base_url(&self) -> &str {
        self.config
            .base_url
            .as_deref()
            .unwrap_or(match self.config.provider {
                LlmProvider::OpenAI => "https://api.openai.com/v1",
                LlmProvider::Anthropic => "https://api.anthropic.com/v1",
                LlmProvider::Ollama => "http://localhost:11434/v1",
            })
    }
}

// ---------------------------------------------------------------------------
// Provider implementations
// ---------------------------------------------------------------------------

/// OpenAI-compatible provider (covers OpenAI, Ollama, LM Studio, etc.).
pub struct OpenAiProvider {
    client: LlmClient,
}

impl OpenAiProvider {
    pub fn new(config: LlmConfig) -> Result<Self> {
        let client = LlmClient::new(config)?;
        Ok(Self { client })
    }
}

#[async_trait]
impl LlmBackend for OpenAiProvider {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        let _permit = self
            .client
            .semaphore
            .acquire()
            .await
            .context("LLM concurrency limit reached")?;
        let url = format!("{}/chat/completions", self.client.base_url());
        let mut messages = Vec::new();
        if let Some(sys) = system {
            messages.push(serde_json::json!({"role": "system", "content": sys}));
        }
        messages.push(serde_json::json!({"role": "user", "content": prompt}));

        let body = serde_json::json!({
            "model": self.client.config.model,
            "messages": messages,
            "temperature": self.client.config.temperature,
            "max_tokens": self.client.config.max_tokens,
        });

        let mut request = self.client.http.post(&url).json(&body);
        if let Some(key) = &self.client.config.api_key {
            request = request.bearer_auth(key);
        }

        let response = request.send().await.context("LLM request failed")?;
        let status = response.status();
        let text = response.text().await.context("LLM response read failed")?;
        if !status.is_success() {
            anyhow::bail!("LLM API error ({}): {}", status, text);
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&text).context("LLM response JSON parse failed")?;
        parsed
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|content| content.as_str())
            .map(|s| s.trim().to_string())
            .ok_or_else(|| anyhow::anyhow!("LLM response missing content: {}", text))
    }

    async fn complete_structured(
        &self,
        prompt: &str,
        system: Option<&str>,
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let _permit = self
            .client
            .semaphore
            .acquire()
            .await
            .context("LLM concurrency limit reached")?;
        let url = format!("{}/chat/completions", self.client.base_url());
        let mut messages = Vec::new();
        if let Some(sys) = system {
            messages.push(serde_json::json!({"role": "system", "content": sys}));
        }
        messages.push(serde_json::json!({"role": "user", "content": prompt}));
        let body = serde_json::json!({
            "model": self.client.config.model,
            "messages": messages,
            "temperature": 0.0,
            "max_tokens": self.client.config.max_tokens,
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "structured_response",
                    "schema": schema,
                    "strict": true
                }
            }
        });

        let mut request = self.client.http.post(&url).json(&body);
        if let Some(key) = &self.client.config.api_key {
            request = request.bearer_auth(key);
        }

        let response = request
            .send()
            .await
            .context("LLM structured request failed")?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("LLM structured response read failed")?;
        if !status.is_success() {
            anyhow::bail!("LLM API error ({}): {}", status, text);
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&text).context("LLM structured response JSON parse failed")?;
        let content = parsed
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|content| content.as_str())
            .ok_or_else(|| anyhow::anyhow!("LLM structured response missing content: {}", text))?;

        let cleaned = content
            .trim()
            .strip_prefix("```json")
            .or_else(|| content.trim().strip_prefix("```"))
            .map(|s| s.strip_suffix("```").unwrap_or(s).trim())
            .unwrap_or(content);
        serde_json::from_str(cleaned).context("LLM structured response content parse error")
    }
}

/// Anthropic provider.
///
/// Uses the Messages API. `complete_structured` falls back to the trait default
/// because Anthropic's native structured-output support (tool use) is more
/// complex to wire generically.
pub struct AnthropicProvider {
    client: LlmClient,
}

impl AnthropicProvider {
    pub fn new(config: LlmConfig) -> Result<Self> {
        let client = LlmClient::new(config)?;
        Ok(Self { client })
    }
}

#[async_trait]
impl LlmBackend for AnthropicProvider {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        let _permit = self
            .client
            .semaphore
            .acquire()
            .await
            .context("LLM concurrency limit reached")?;
        let url = format!("{}/messages", self.client.base_url());
        let mut body = serde_json::json!({
            "model": self.client.config.model,
            "max_tokens": self.client.config.max_tokens,
            "temperature": self.client.config.temperature,
            "messages": [{"role": "user", "content": prompt}],
        });
        if let Some(sys) = system {
            body["system"] = serde_json::Value::String(sys.to_string());
        }

        let mut request = self
            .client
            .http
            .post(&url)
            .header("anthropic-version", "2023-06-01")
            .json(&body);
        if let Some(key) = &self.client.config.api_key {
            request = request.header("x-api-key", key);
        }

        let response = request.send().await.context("Anthropic request failed")?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("Anthropic response read failed")?;
        if !status.is_success() {
            anyhow::bail!("Anthropic API error ({}): {}", status, text);
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&text).context("Anthropic response JSON parse failed")?;
        parsed
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|block| block.get("text"))
            .and_then(|text| text.as_str())
            .map(|s| s.trim().to_string())
            .ok_or_else(|| anyhow::anyhow!("Anthropic response missing content: {}", text))
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/// Build an `LlmBackend` from configuration.
pub fn build_llm_backend(config: LlmConfig) -> Result<Arc<dyn LlmBackend>> {
    match config.provider {
        LlmProvider::OpenAI | LlmProvider::Ollama => Ok(Arc::new(OpenAiProvider::new(config)?)),
        LlmProvider::Anthropic => Ok(Arc::new(AnthropicProvider::new(config)?)),
    }
}

/// Convenience: try to build an LLM backend from environment.
/// Returns `None` if `MAG_LLM_PROVIDER` is unset.
pub fn llm_backend_from_env() -> Option<Arc<dyn LlmBackend>> {
    let config = LlmConfig::from_env()?;
    build_llm_backend(config).ok()
}

/// Build the explicit environment backend or the local-first LFM2.5 default.
pub fn llm_backend_from_env_or_local_default() -> Result<Arc<dyn LlmBackend>> {
    build_llm_backend(LlmConfig::from_env_or_local_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_local_lfm25_1_2b() {
        let config = LlmConfig::default();
        assert_eq!(config.provider, LlmProvider::Ollama);
        assert_eq!(config.model, DEFAULT_LOCAL_LLM_MODEL);
        assert_eq!(config.base_url.as_deref(), Some(DEFAULT_LOCAL_LLM_BASE_URL));
        assert_eq!(config.temperature, 0.1);
    }

    #[test]
    fn onnx_targets_are_recorded_for_runtime_work() {
        assert!(TARGET_ONNX_LOCAL_LLM_MODEL.ends_with("-ONNX"));
        assert!(EXPERIMENTAL_SMALL_ONNX_LLM_MODEL.ends_with("-ONNX"));
    }
}
