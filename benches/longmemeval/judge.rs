use std::collections::BTreeMap;
use std::env;
use std::sync::OnceLock;
use std::time::Duration as StdDuration;

use anyhow::{Result, anyhow};
use dotenvy::dotenv;

use crate::helpers::{record_result, truncate};
use crate::types::{
    CategoryResult, JudgeCostEstimate, OpenAiChatRequest, OpenAiChatResponse, OpenAiMessage,
    QuestionEvaluation,
};

pub(crate) static OPENAI_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
pub(crate) static OPENAI_API_KEY: OnceLock<String> = OnceLock::new();
pub(crate) static OPENAI_MODEL: OnceLock<String> = OnceLock::new();

pub(crate) fn load_api_key_from_dotenv() {
    dotenv().ok();
}

pub(crate) fn init_llm_judge(model: &str) -> Result<()> {
    // Validate API key first — before setting any global state.
    let api_key = env::var("OPENAI_API_KEY")
        .map_err(|_| anyhow!("--llm-judge requires OPENAI_API_KEY (env var or .env file)"))?;

    // Check for conflicting re-initialization.
    if let Some(existing) = OPENAI_MODEL.get()
        && existing != model
    {
        return Err(anyhow!(
            "LLM judge already initialized with model '{}', cannot reinitialize with '{}'",
            existing,
            model
        ));
    }

    // Initialize all globals (idempotent — OnceLock ignores subsequent sets).
    if OPENAI_MODEL.get().is_none() {
        OPENAI_MODEL
            .set(model.to_string())
            .map_err(|_| anyhow!("LLM judge model initialization race"))?;
    }
    if OPENAI_API_KEY.get().is_none() {
        OPENAI_API_KEY
            .set(api_key)
            .map_err(|_| anyhow!("LLM judge API key initialization race"))?;
    }
    if OPENAI_CLIENT.get().is_none() {
        let client = reqwest::Client::builder()
            .timeout(StdDuration::from_secs(30))
            .build()?;
        OPENAI_CLIENT
            .set(client)
            .map_err(|_| anyhow!("LLM judge client initialization race"))?;
    }

    // Post-initialization verification guards against TOCTOU race on OPENAI_MODEL.
    let final_model = OPENAI_MODEL.get().expect("OPENAI_MODEL was just set above");
    if final_model != model {
        return Err(anyhow!(
            "LLM judge model mismatch: expected '{}', found '{}'",
            model,
            final_model
        ));
    }

    Ok(())
}

pub(crate) fn llm_prompt(
    question: &str,
    expected: &str,
    actual: &str,
    question_type: &str,
) -> String {
    let instruction = match question_type {
        "temporal" => {
            "Does the response contain the correct answer? Answer yes or no only. Do not penalize off-by-one errors for days."
        }
        "knowledge-update" => {
            "Does the response contain the correct answer? Answer yes or no only. The response may contain multiple memories — answer yes if ANY of them contains the expected updated information, even if older versions are also present."
        }
        "abstention" => {
            "Does the model correctly identify the question as unanswerable? Answer yes or no only."
        }
        _ => {
            "Does the response contain the correct answer? Answer yes or no only. The response may contain multiple memories — answer yes if ANY of them contains the expected information."
        }
    };
    format!(
        "Question:\n{question}\n\nExpected answer:\n{expected}\n\nModel response:\n{actual}\n\n{instruction}"
    )
}

pub(crate) fn judge_input_tokens_estimate(
    question: &str,
    expected: &str,
    actual: &str,
    question_type: &str,
) -> usize {
    let chars = llm_prompt(question, expected, actual, question_type)
        .chars()
        .count();
    chars.div_ceil(4)
}

pub(crate) fn input_rate_per_million(model: &str) -> f64 {
    if model.starts_with("gpt-4o-mini") {
        crate::INPUT_RATE_PER_1M_GPT_4O_MINI
    } else if model.starts_with("gpt-4.1")
        || model.starts_with("gpt-4-1")
        || model.starts_with("gpt-4o")
    {
        crate::INPUT_RATE_PER_1M_GPT_4_1
    } else {
        crate::INPUT_RATE_PER_1M_GPT_4O_MINI
    }
}

/// Map official question types to LLM judge instruction variants.
pub(crate) fn official_judge_type(question_type: &str) -> &'static str {
    match question_type {
        "temporal" | "temporal-reasoning" => "temporal",
        "knowledge-update" => "knowledge-update",
        _ => "standard",
    }
}

pub(crate) async fn llm_judge_eval(
    question: &str,
    expected: &str,
    actual: &str,
    question_type: &str,
) -> Result<(bool, usize)> {
    let client = OPENAI_CLIENT
        .get()
        .ok_or_else(|| anyhow!("LLM judge client not initialized"))?;
    let api_key = OPENAI_API_KEY
        .get()
        .ok_or_else(|| anyhow!("LLM judge API key not initialized"))?;
    let model = OPENAI_MODEL
        .get()
        .cloned()
        .unwrap_or_else(|| crate::DEFAULT_JUDGE_MODEL.to_string());
    let body = OpenAiChatRequest {
        model,
        temperature: 0.0,
        max_tokens: 10,
        messages: vec![OpenAiMessage {
            role: "user".to_string(),
            content: llm_prompt(question, expected, actual, question_type),
        }],
    };
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    let parsed: OpenAiChatResponse = response.json().await?;
    let prompt_tokens = parsed
        .usage
        .as_ref()
        .map(|u| {
            u.prompt_tokens.try_into().unwrap_or_else(|_| {
                eprintln!(
                    "warn: prompt_tokens {} overflows usize, using 1",
                    u.prompt_tokens
                );
                1usize
            })
        })
        .unwrap_or(0);
    let answer = parsed
        .choices
        .first()
        .map(|choice| choice.message.content.to_lowercase())
        .ok_or_else(|| anyhow!("OpenAI response missing choices"))?;
    let normalized = answer.trim();
    let passed = normalized.strip_prefix("yes").is_some_and(|rest| {
        rest.is_empty() || rest.starts_with([' ', '.', ',', ':', ';', '!', '?'])
    });
    Ok((passed, prompt_tokens))
}

pub(crate) async fn run_llm_judge(
    evals: &[QuestionEvaluation],
    verbose: bool,
) -> (BTreeMap<String, CategoryResult>, JudgeCostEstimate, usize) {
    let mut results = BTreeMap::<String, CategoryResult>::new();
    let model = OPENAI_MODEL
        .get()
        .cloned()
        .unwrap_or_else(|| crate::DEFAULT_JUDGE_MODEL.to_string());
    let rate = input_rate_per_million(model.as_str());
    let mut input_tokens = 0usize;
    let mut fallback_count = 0usize;

    for eval in evals {
        // Abstention uses score-threshold gating, NOT content matching.
        // Always use the substring (threshold) result for abstention.
        if eval.category == "abstention" {
            let detail = if verbose {
                let status = if eval.substring_passed {
                    "PASS"
                } else {
                    "FAIL"
                };
                Some(format!(
                    "  [{status}] Q: {}  E: {}",
                    truncate(eval.question.as_str(), 60),
                    truncate(eval.expected.as_str(), 40)
                ))
            } else {
                None
            };
            record_result(&mut results, "abstention", eval.substring_passed, detail);
            continue;
        }
        let judged = llm_judge_eval(
            eval.question.as_str(),
            eval.expected.as_str(),
            eval.actual.as_str(),
            official_judge_type(eval.question_type.as_str()),
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let passed = match judged {
            Ok((v, tokens)) => {
                input_tokens += tokens;
                v
            }
            Err(err) => {
                fallback_count += 1;
                input_tokens += judge_input_tokens_estimate(
                    eval.question.as_str(),
                    eval.expected.as_str(),
                    eval.actual.as_str(),
                    official_judge_type(eval.question_type.as_str()),
                );
                eprintln!(
                    "warning: LLM judge failed for category '{}', using substring fallback: {}",
                    eval.category, err
                );
                eval.substring_passed
            }
        };

        let detail = if verbose {
            let status = if passed { "PASS" } else { "FAIL" };
            Some(format!(
                "  [{status}] Q: {}  E: {}",
                truncate(eval.question.as_str(), 60),
                truncate(eval.expected.as_str(), 40)
            ))
        } else {
            None
        };
        record_result(&mut results, eval.category.as_str(), passed, detail);
    }

    #[allow(clippy::cast_precision_loss)]
    let estimated_input_cost_usd = input_tokens as f64 / 1_000_000.0 * rate;
    let cost = JudgeCostEstimate {
        model,
        input_tokens_estimate: input_tokens,
        input_rate_per_million_usd: rate,
        estimated_input_cost_usd,
    };
    (results, cost, fallback_count)
}
