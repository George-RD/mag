use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct OpenAiChatRequest {
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u16,
    pub messages: Vec<OpenAiMessage>,
}

#[derive(Debug, Serialize)]
pub struct OpenAiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiChatResponse {
    pub choices: Vec<OpenAiChoice>,
    #[serde(default)]
    #[allow(dead_code)]
    pub usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiUsage {
    #[allow(dead_code)]
    pub prompt_tokens: u64,
    #[allow(dead_code)]
    pub completion_tokens: u64,
    #[allow(dead_code)]
    pub total_tokens: u64,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiChoice {
    pub message: OpenAiResponseMessage,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiResponseMessage {
    pub content: String,
}
