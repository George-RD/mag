from pathlib import Path
root = Path('.')

def edit(name, old, new):
    path = root / name
    data = path.read_text()
    assert data.count(old) == 1, (name, data.count(old), old[:80])
    path.write_text(data.replace(old, new))

edit('src/memory_core/llm.rs', '    /// Generate a structured completion as JSON.\n', '''    /// Request native schema constraints while preserving the raw completion.
    ///
    /// Unlike `complete_structured`, this must not alter the prompt, override
    /// decoding settings, parse or repair output, retry, or fall back silently.
    /// Unsupported providers fail explicitly before making a request.
    async fn complete_constrained(
        &self,
        _prompt: &str,
        _system: Option<&str>,
        _schema: &serde_json::Value,
    ) -> Result<String> {
        anyhow::bail!("LLM backend does not support raw schema-constrained completion")
    }

    /// Generate a structured completion as JSON.
''')
p = root / 'src/memory_core/llm.rs'
s = p.read_text()
start = s.index('impl OpenAiProvider {')
end = s.index('\n/// Anthropic provider.', start)
s = s[:start] + '''impl OpenAiProvider {
    pub fn new(config: LlmConfig) -> Result<Self> {
        let client = LlmClient::new(config)?;
        Ok(Self { client })
    }

    /// One shared HTTP exchange; callers explicitly own any output transformation.
    async fn request_completion(
        &self,
        prompt: &str,
        system: Option<&str>,
        temperature: f32,
        schema: Option<&serde_json::Value>,
    ) -> Result<String> {
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
        let mut body = serde_json::json!({
            "model": self.client.config.model,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": self.client.config.max_tokens,
        });
        if let Some(schema) = schema {
            body["response_format"] = serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "structured_response",
                    "schema": schema,
                    "strict": true
                }
            });
        }
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
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("LLM response missing content: {}", text))
    }
}

#[async_trait]
impl LlmBackend for OpenAiProvider {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        self.request_completion(prompt, system, self.client.config.temperature, None)
            .await
            .map(|text| text.trim().to_owned())
    }

    async fn complete_constrained(
        &self,
        prompt: &str,
        system: Option<&str>,
        schema: &serde_json::Value,
    ) -> Result<String> {
        self.request_completion(prompt, system, self.client.config.temperature, Some(schema))
            .await
            .map(|text| text.trim().to_owned())
    }

    async fn complete_structured(
        &self,
        prompt: &str,
        system: Option<&str>,
        schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        // Preserve the existing repairing API's temperature and fence handling.
        // Evaluation deliberately uses complete_constrained instead.
        let content = self.request_completion(prompt, system, 0.0, Some(schema)).await?;
        let cleaned = content
            .trim()
            .strip_prefix("```json")
            .or_else(|| content.trim().strip_prefix("```"))
            .map(|s| s.strip_suffix("```").unwrap_or(s).trim())
            .unwrap_or(&content);
        serde_json::from_str(cleaned).context("LLM structured response content parse error")
    }
}
''' + s[end:]
p.write_text(s)
edit('src/local_memory_runtime/intelligence.rs', 'impl LocalMemoryRuntime {\n', '''/// Fixed output-shape schema. It contains no case labels, source IDs or answers.
///
/// This is only a requested decoding constraint: the independent scorer still
/// checks nonblank values, uniqueness, source support and task correctness.
pub fn intelligence_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["items"],
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["value", "source_ids"],
                    "properties": {
                        "value": {"type": "string", "minLength": 1},
                        "source_ids": {
                            "type": "array", "minItems": 1,
                            "items": {"type": "string", "minLength": 1}
                        }
                    }
                }
            }
        }
    })
}

impl LocalMemoryRuntime {
''')
edit('src/local_memory_runtime/intelligence.rs', '''        let prompt = request.prompt()?;
        let completion = backend.complete(&prompt, Some(SYSTEM_PROMPT)).await.map_err(|_| {
''', '''        Self::produce_intelligence_output(backend, request, None).await
    }

    /// Requests the fixed native output schema once, without repair or fallback.
    /// Validation, prompt, bounds and error redaction are shared with plain mode.
    pub async fn produce_intelligence_with_schema(
        backend: &dyn LlmBackend,
        request: &IntelligenceRequest,
    ) -> Result<String> {
        let schema = intelligence_output_schema();
        Self::produce_intelligence_output(backend, request, Some(&schema)).await
    }

    async fn produce_intelligence_output(
        backend: &dyn LlmBackend,
        request: &IntelligenceRequest,
        schema: Option<&serde_json::Value>,
    ) -> Result<String> {
        let prompt = request.prompt()?;
        let result = match schema {
            Some(schema) => backend.complete_constrained(&prompt, Some(SYSTEM_PROMPT), schema).await,
            None => backend.complete(&prompt, Some(SYSTEM_PROMPT)).await,
        };
        let completion = result.map_err(|_| {
''')
edit('src/lib.rs', '    MAX_INTELLIGENCE_BYTES,\n', '    MAX_INTELLIGENCE_BYTES, intelligence_output_schema,\n')
edit('src/cli.rs', '''    /// Describe configured, unverified settings without stdin or model access.
''', '''    /// Request native JSON-schema constraints; never repair output or fall back.
    #[arg(long)]
    pub json_schema: bool,
    /// Describe configured, unverified settings without stdin or model access.
''')
edit('src/intelligence_cli.rs', '    INTELLIGENCE_PROMPT_VERSION, IntelligenceRequest, LocalMemoryRuntime, MAX_INTELLIGENCE_BYTES,\n', '    INTELLIGENCE_PROMPT_VERSION, IntelligenceRequest, LocalMemoryRuntime, MAX_INTELLIGENCE_BYTES,\n    intelligence_output_schema,\n')
edit('src/intelligence_cli.rs', '                "prompt_version": INTELLIGENCE_PROMPT_VERSION,\n', '''                "prompt_version": INTELLIGENCE_PROMPT_VERSION,
                "output_mode": if args.json_schema { "json_schema" } else { "unconstrained" },
                "output_schema": args.json_schema.then(intelligence_output_schema),
''')
edit('src/intelligence_cli.rs', '''    let completion = LocalMemoryRuntime::produce_intelligence(backend.as_ref(), &request).await?;
''', '''    let completion = if args.json_schema {
        LocalMemoryRuntime::produce_intelligence_with_schema(backend.as_ref(), &request).await?
    } else {
        LocalMemoryRuntime::produce_intelligence(backend.as_ref(), &request).await?
    };
''')
