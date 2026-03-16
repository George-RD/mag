use std::env;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Result, anyhow};
use dotenvy::dotenv;

use crate::types::{OpenAiChatRequest, OpenAiChatResponse, OpenAiMessage, RetrievalHit};

pub(crate) const OPENAI_URL: &str = "https://api.openai.com/v1/chat/completions";

static LLM_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static LLM_API_KEY: OnceLock<Option<String>> = OnceLock::new();
static LLM_MODEL: OnceLock<String> = OnceLock::new();
static LLM_URL: OnceLock<String> = OnceLock::new();

pub(crate) fn load_api_key_from_dotenv() {
    dotenv().ok();
    dotenvy::from_filename(".env.local").ok();
}

fn set_globals(model: &str, url: &str, api_key: Option<String>) -> Result<()> {
    if let Some(existing) = LLM_MODEL.get()
        && existing != model
    {
        return Err(anyhow!(
            "LLM already initialized with model '{}', cannot reinitialize with '{}'",
            existing,
            model
        ));
    }

    // OnceLock::set returns Err if already set -- safe to ignore since we
    // already checked for conflicting model above, and the bench is single-threaded.
    let _ = LLM_MODEL.set(model.to_string());
    let _ = LLM_URL.set(url.to_string());
    let _ = LLM_API_KEY.set(api_key);
    let _ = LLM_CLIENT.set(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?,
    );
    Ok(())
}

/// Initialize for OpenAI (requires OPENAI_API_KEY).
pub(crate) fn init_llm(model: &str, url: &str) -> Result<()> {
    let api_key = env::var("OPENAI_API_KEY")
        .map_err(|_| anyhow!("--llm-judge requires OPENAI_API_KEY (env var or .env file)"))?;
    set_globals(model, url, Some(api_key))
}

/// Initialize for a local server (no API key needed).
pub(crate) fn init_llm_local(model: &str, url: &str) -> Result<()> {
    set_globals(model, url, None)
}

/// Generate an answer from retrieved context + question using an LLM.
///
/// This is the "generate" step of retrieve-then-generate. It sends the
/// retrieved memories as context and asks the LLM to answer the question.
pub(crate) async fn generate_answer(question: &str, hits: &[RetrievalHit]) -> Result<String> {
    let client = LLM_CLIENT
        .get()
        .ok_or_else(|| anyhow!("LLM client not initialized"))?;
    let url = LLM_URL
        .get()
        .ok_or_else(|| anyhow!("LLM URL not initialized"))?;
    let api_key = LLM_API_KEY.get().and_then(|k| k.as_deref());
    let model = LLM_MODEL
        .get()
        .cloned()
        .unwrap_or_else(|| crate::DEFAULT_LLM_MODEL.to_string());

    let context = hits
        .iter()
        .enumerate()
        .map(|(i, hit)| {
            let date_suffix = hit
                .metadata
                .get("date")
                .and_then(|v| v.as_str())
                .map(|d| format!(" [Date: {d}]"))
                .unwrap_or_default();
            format!("[{}] {}{}", i + 1, hit.content, date_suffix)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Based on the following conversation excerpts, answer the question concisely.\n\n\
         Context:\n{context}\n\n\
         Question: {question}\n\n\
         Answer:"
    );

    let body = OpenAiChatRequest {
        model,
        temperature: 0.0,
        max_tokens: 256,
        messages: vec![OpenAiMessage {
            role: "user".to_string(),
            content: prompt,
        }],
    };

    let mut request = client.post(url).json(&body);
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }

    let response = request.send().await?.error_for_status()?;

    let parsed: OpenAiChatResponse = response.json().await?;
    let answer = parsed
        .choices
        .first()
        .map(|choice| choice.message.content.trim().to_string())
        .ok_or_else(|| anyhow!("LLM response missing choices"))?;

    Ok(answer)
}
