use std::collections::BTreeMap;
use std::io::Write;
use std::time::Instant;

use anyhow::{Result, anyhow};
use mag::benchmarking::BenchmarkMetadata;

use mag::memory_core::storage::sqlite::SqliteStorage;
use mag::memory_core::*;

use crate::ABSTENTION_FALLBACK_SCORE;
use crate::helpers::{PeakRss, grade, pct, record_result, summarize_totals, truncate};
use crate::judge::{llm_judge_eval, official_judge_type};
use crate::types::{CategoryResult, Hit, OfficialQuestion, OfficialSummary};

pub(crate) fn official_categories() -> Vec<(&'static str, &'static str)> {
    vec![
        ("single-session-user", "Single-Session User"),
        ("single-session-assistant", "Single-Session Assistant"),
        ("single-session-preference", "Single-Session Preference"),
        ("multi-session", "Multi-Session"),
        ("temporal-reasoning", "Temporal Reasoning"),
        ("knowledge-update", "Knowledge Update"),
    ]
}

/// Compute task-averaged score: mean of per-category percentages.
/// This is how omega reports their score — weights each category equally.
pub(crate) fn compute_task_averaged(results: &BTreeMap<String, CategoryResult>) -> f64 {
    let mut cat_pcts = Vec::new();
    for (key, _label) in official_categories() {
        if let Some(cat) = results.get(key)
            && cat.total > 0
        {
            cat_pcts.push(pct(cat.correct, cat.total));
        }
    }
    if cat_pcts.is_empty() {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    {
        cat_pcts.iter().sum::<f64>() / cat_pcts.len() as f64
    }
}

pub(crate) fn load_official_dataset(path: &std::path::Path) -> Result<Vec<OfficialQuestion>> {
    let file = std::fs::File::open(path)
        .map_err(|e| anyhow!("failed to open dataset at {}: {e}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let questions: Vec<OfficialQuestion> = serde_json::from_reader(reader)
        .map_err(|e| anyhow!("failed to parse dataset JSON: {e}"))?;
    Ok(questions)
}

async fn seed_official_question(
    storage: &SqliteStorage,
    question: &OfficialQuestion,
) -> Result<usize> {
    let mut count = 0usize;
    if question.haystack_session_ids.len() != question.haystack_sessions.len() {
        return Err(anyhow!(
            "question {} has mismatched haystack data: {} session ids vs {} session payloads",
            question.question_id,
            question.haystack_session_ids.len(),
            question.haystack_sessions.len()
        ));
    }
    for (session_idx, (session_id, turns)) in question
        .haystack_session_ids
        .iter()
        .zip(question.haystack_sessions.iter())
        .enumerate()
    {
        for (turn_idx, turn) in turns.iter().enumerate() {
            if turn.content.trim().is_empty() {
                continue;
            }
            let memory_id = format!("off-{}-s{session_idx}-t{turn_idx}", question.question_id);
            let input = MemoryInput {
                content: String::new(),
                id: None,
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                event_type: Some(EventType::Unknown("observation".to_string())),
                session_id: Some(session_id.clone()),
                project: None,
                priority: Some(3),
                entity_id: None,
                agent_type: Some(turn.role.clone()),
                ttl_seconds: None,
                referenced_date: None,
            };
            storage.store(&memory_id, &turn.content, &input).await?;
            count += 1;
        }
    }
    Ok(count)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_official_benchmark(
    questions: &[OfficialQuestion],
    dataset_total: usize,
    metadata: BenchmarkMetadata,
    embedder: std::sync::Arc<OnnxEmbedder>,
    verbose: bool,
    llm_judge: bool,
    top_k: usize,
    rss: &mut PeakRss,
) -> Result<OfficialSummary> {
    let total = questions.len();
    let mut results = BTreeMap::<String, CategoryResult>::new();
    let mut total_memories = 0usize;
    let mut total_query_ms = 0u128;
    let overall_start = Instant::now();

    let no_filter = SearchOptions {
        event_type: None,
        project: None,
        session_id: None,
        include_superseded: None,
        importance_min: None,
        created_after: None,
        created_before: None,
        context_tags: None,
        entity_id: None,
        agent_type: None,
        event_after: None,
        event_before: None,
        explain: None,
    };

    for (index, question) in questions.iter().enumerate() {
        // Fresh database per question — isolates haystacks as paper intends.
        let storage = SqliteStorage::new_in_memory_with_embedder(embedder.clone())?;

        // Seed haystack sessions as memories.
        let seeded = seed_official_question(&storage, question).await?;
        total_memories += seeded;
        rss.sample();

        // Query.
        let query_start = Instant::now();
        let hits = {
            let advanced = <SqliteStorage as AdvancedSearcher>::advanced_search(
                &storage,
                &question.question,
                top_k,
                &no_filter,
            )
            .await;
            match advanced {
                Ok(items) if !items.is_empty() => items
                    .into_iter()
                    .map(|item| Hit {
                        content: item.content,
                        score: item.score,
                    })
                    .collect::<Vec<_>>(),
                Ok(_) => {
                    // Empty advanced_search = abstention. Fall back to basic search.
                    let items = <SqliteStorage as Searcher>::search(
                        &storage,
                        &question.question,
                        top_k,
                        &no_filter,
                    )
                    .await
                    .map_err(|e| {
                        anyhow!("basic search failed for Q{}: {e}", question.question_id)
                    })?;
                    items
                        .into_iter()
                        .map(|item| Hit {
                            content: item.content,
                            score: ABSTENTION_FALLBACK_SCORE,
                        })
                        .collect()
                }
                Err(err) => {
                    eprintln!(
                        "warning: advanced_search failed for Q{} ({err}); falling back to basic search",
                        question.question_id
                    );
                    let items = <SqliteStorage as Searcher>::search(
                        &storage,
                        &question.question,
                        top_k,
                        &no_filter,
                    )
                    .await
                    .map_err(|e| {
                        anyhow!(
                            "basic fallback search failed for Q{}: {e}",
                            question.question_id
                        )
                    })?;
                    items
                        .into_iter()
                        .map(|item| Hit {
                            content: item.content,
                            score: ABSTENTION_FALLBACK_SCORE,
                        })
                        .collect()
                }
            }
        };
        let query_ms = query_start.elapsed().as_millis();
        total_query_ms += query_ms;
        rss.sample();

        // Evaluate.
        let actual = hits
            .iter()
            .map(|hit| hit.content.as_str())
            .collect::<Vec<_>>()
            .join("\n---\n");

        let passed = if llm_judge {
            let judge_type = official_judge_type(&question.question_type);
            match llm_judge_eval(&question.question, &question.answer, &actual, judge_type).await {
                Ok((verdict, _tokens)) => verdict,
                Err(err) => {
                    eprintln!(
                        "warning: LLM judge failed for Q{}, using substring fallback: {err}",
                        question.question_id
                    );
                    actual
                        .to_lowercase()
                        .contains(&question.answer.to_lowercase())
                }
            }
        } else {
            // Substring match: expected answer appears in any of the top-k results.
            actual
                .to_lowercase()
                .contains(&question.answer.to_lowercase())
        };

        let detail = if verbose {
            let status = if passed { "PASS" } else { "FAIL" };
            Some(format!(
                "  [{status}] Q{}: {} → E: {} ({seeded} mems, {query_ms}ms)",
                question.question_id,
                truncate(&question.question, 50),
                truncate(&question.answer, 40),
            ))
        } else {
            None
        };
        record_result(&mut results, &question.question_type, passed, detail);

        // Progress on stderr.
        let cat_key = &question.question_type;
        let cat = results.get(cat_key).cloned().unwrap_or_default();
        let status_char = if passed { '✓' } else { '✗' };
        eprint!(
            "\r[{}/{}] {status_char} {} — {}/{} correct ({seeded} mems, {query_ms}ms)            ",
            index + 1,
            total,
            truncate(cat_key, 25),
            cat.correct,
            cat.total,
        );
        let _ = std::io::stderr().flush();
    }
    eprintln!(); // Finish progress line.

    let overall_seconds = overall_start.elapsed().as_secs_f64();
    let (raw_correct, _raw_total, raw_pct) = summarize_totals(&results);
    let task_averaged = compute_task_averaged(&results);
    #[allow(clippy::cast_precision_loss)]
    let avg_mems = if total > 0 {
        total_memories as f64 / total as f64
    } else {
        0.0
    };
    #[allow(clippy::cast_precision_loss)]
    let avg_query = if total > 0 {
        total_query_ms as f64 / total as f64
    } else {
        0.0
    };

    Ok(OfficialSummary {
        metadata,
        dataset: "LongMemEval_S".to_string(),
        total_questions: dataset_total,
        questions_evaluated: total,
        total_memories_ingested: total_memories,
        total_duration_seconds: overall_seconds,
        avg_memories_per_question: avg_mems,
        avg_query_ms: avg_query,
        peak_rss_kb: rss.peak_kb,
        raw_correct,
        raw_percentage: raw_pct,
        task_averaged_percentage: task_averaged,
        categories: results,
    })
}

pub(crate) fn print_official_results(summary: &OfficialSummary) {
    println!();
    println!("========================================================================");
    println!("  MAG — Official LongMemEval_S Benchmark Results");
    println!("========================================================================");
    println!(
        "  Dataset: {} ({}/{} questions evaluated)",
        summary.dataset, summary.questions_evaluated, summary.total_questions
    );
    println!("  Source: {}", summary.metadata.dataset_source);
    println!("  Cache:  {}", summary.metadata.dataset_path);
    println!(
        "  Memories ingested: {} (avg {:.1}/question)",
        summary.total_memories_ingested, summary.avg_memories_per_question
    );
    println!(
        "  Duration: {:.1}s (avg {:.0}ms/query)",
        summary.total_duration_seconds, summary.avg_query_ms
    );
    println!("  Peak RSS: {} KB", summary.peak_rss_kb);
    println!();

    for (key, label) in official_categories() {
        if let Some(cat) = summary.categories.get(key) {
            let percent = pct(cat.correct, cat.total);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let filled = (percent / 5.0).floor() as usize;
            let bar = format!(
                "{}{}",
                "#".repeat(filled),
                "-".repeat(20usize.saturating_sub(filled))
            );
            println!(
                "  {label:30} {:>3}/{:<3}  [{bar}] {:>5.1}%  ({})",
                cat.correct,
                cat.total,
                percent,
                grade(percent)
            );
            for line in &cat.details {
                println!("{line}");
            }
        }
    }

    println!();
    println!("------------------------------------------------------------------------");
    println!(
        "  RAW:            {}/{} = {:.1}%",
        summary.raw_correct, summary.questions_evaluated, summary.raw_percentage
    );
    println!(
        "  TASK-AVERAGED:  {:.1}%  (mean of per-category %, omega-comparable)",
        summary.task_averaged_percentage
    );
    println!("------------------------------------------------------------------------");
}
