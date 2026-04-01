use std::collections::BTreeMap;

use crate::bench_utils::formatting::{grade, pct};
use crate::types::{CategoryResult, LoCoMoSummary, SweepRow};

fn avg_f1(cat: &CategoryResult) -> f64 {
    if cat.total == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        {
            cat.f1_sum / cat.total as f64 * 100.0
        }
    }
}

fn avg_evidence(cat: &CategoryResult) -> f64 {
    if cat.total == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        {
            cat.evidence_recall_sum / cat.total as f64 * 100.0
        }
    }
}

pub(crate) fn locomo_categories() -> Vec<(&'static str, &'static str)> {
    vec![
        ("single-hop", "Single-Hop QA"),
        ("temporal", "Temporal Reasoning"),
        ("multi-hop", "Multi-Hop QA"),
        ("open-domain", "Open-Domain"),
        ("adversarial", "Adversarial"),
    ]
}

pub(crate) fn print_results(summary: &LoCoMoSummary) {
    // Substring mode uses token_f1 on concatenated retrieval text.
    let score_col = match summary.scoring_mode.as_str() {
        "word-overlap" => "WdOvlp",
        "llm-f1" => "LlmF1",
        "e2e-word-overlap" => "E2eWO",
        "substring" => "TokF1",
        _ => "TokF1",
    };

    println!();
    println!("========================================================================");
    println!("  MAG — LoCoMo Benchmark Results");
    println!("========================================================================");
    println!("  Dataset: {}", summary.dataset);
    println!("  Scoring: {}", summary.scoring_mode);
    println!("  Source:  {}", summary.metadata.dataset_source);
    println!("  Cache:   {}", summary.metadata.dataset_path);
    println!(
        "  Samples: {}  Questions: {}",
        summary.samples_evaluated, summary.questions_evaluated
    );
    println!(
        "  Memories ingested: {}  Duration: {:.1}s  Avg query: {:.0}ms",
        summary.total_memories_ingested, summary.total_duration_seconds, summary.avg_query_ms
    );
    println!("  Peak RSS: {} KB", summary.peak_rss_kb);
    println!("  Embedder: {}", summary.embedder_name);
    {
        #[allow(clippy::cast_precision_loss)]
        let seed_time_s = summary.total_seed_ms as f64 / 1000.0;
        println!(
            "  Embed calls: {}  Avg embed: {:.1}ms  Seed time: {:.1}s",
            summary.total_embed_calls, summary.avg_embed_ms, seed_time_s
        );
    }

    // Graph edge stats.
    if !summary.graph_edge_totals.is_empty() {
        let total_edges: i64 = summary.graph_edge_totals.values().sum();
        let breakdown: Vec<String> = summary
            .graph_edge_totals
            .iter()
            .map(|(rel_type, count)| format!("{count} {rel_type}"))
            .collect();
        println!(
            "  Graph edges: {} total ({})",
            total_edges,
            breakdown.join(", ")
        );
    }

    println!();

    // Header.
    println!(
        "  {:22} {:>7}  {:>7}  {:>7}  {:>10}",
        "Category", "Substr", score_col, "Ev.Rec", "Count"
    );
    println!("  {}", "-".repeat(60));

    for (key, label) in locomo_categories() {
        if let Some(cat) = summary.categories.get(key) {
            let substr_pct = pct(cat.correct, cat.total);
            let f1_pct = avg_f1(cat);
            let ev_pct = avg_evidence(cat);
            println!(
                "  {:22} {:>6.1}%  {:>6.1}%  {:>6.1}%  {:>4}/{:<4} ({})",
                label,
                substr_pct,
                f1_pct,
                ev_pct,
                cat.correct,
                cat.total,
                grade(f1_pct),
            );
            for line in &cat.details {
                println!("{line}");
            }
        }
    }

    let score_label = match summary.scoring_mode.as_str() {
        "word-overlap" => "WORD OVERLAP",
        "llm-f1" => "MEAN LLM F1",
        "e2e-word-overlap" => "E2E WORD OVERLAP",
        _ => "MEAN TOKEN F1",
    };

    println!();
    println!("------------------------------------------------------------------------");
    println!(
        "  SUBSTRING:        {}/{} = {:.1}%",
        summary.raw_correct, summary.questions_evaluated, summary.raw_percentage
    );
    println!("  {score_label:16}{:.1}%", summary.mean_f1 * 100.0);
    println!(
        "  MEAN EV. RECALL:  {:.1}%",
        summary.mean_evidence_recall * 100.0
    );
    println!("  HIT@1:            {:.1}%", summary.hit_at_1 * 100.0);
    println!("  HIT@3:            {:.1}%", summary.hit_at_3 * 100.0);
    println!("  HIT@5:            {:.1}%", summary.hit_at_5 * 100.0);
    println!("------------------------------------------------------------------------");
}

pub(crate) fn record_result(
    by_category: &mut BTreeMap<String, CategoryResult>,
    category: &str,
    passed: bool,
    f1: f64,
    evidence_recall: f64,
    h1: bool,
    h3: bool,
    h5: bool,
    detail: Option<String>,
) {
    let entry = by_category.entry(category.to_string()).or_default();
    entry.total += 1;
    if passed {
        entry.correct += 1;
    }
    entry.f1_sum += f1;
    entry.evidence_recall_sum += evidence_recall;
    if h1 {
        entry.hit_at_1 += 1;
    }
    if h3 {
        entry.hit_at_3 += 1;
    }
    if h5 {
        entry.hit_at_5 += 1;
    }
    if let Some(line) = detail {
        entry.details.push(line);
    }
}

/// Print the graph factor sweep comparison table.
///
/// Each entry is `(factor, per-category (f1_sum, count))`.
pub(crate) fn print_sweep_results(results: &[SweepRow]) {
    let cats = locomo_categories();
    // Short column labels for each category.
    let cat_labels: Vec<&str> = cats.iter().map(|(_, label)| *label).collect();
    let cat_short: Vec<&str> = cats
        .iter()
        .map(|(key, _)| match *key {
            "single-hop" => "1-Hop",
            "temporal" => "Temp",
            "multi-hop" => "M-Hop",
            "open-domain" => "Open",
            "adversarial" => "Adv",
            other => other,
        })
        .collect();

    println!();
    println!("========================================================================");
    println!("  Graph Factor Sweep");
    println!("========================================================================");

    // Header.
    print!("  {:>6}", "Factor");
    print!("  {:>7}", "Overall");
    for short in &cat_short {
        print!("  {:>7}", short);
    }
    println!();
    print!("  {:>6}", "------");
    print!("  {:>7}", "-------");
    for _ in &cat_short {
        print!("  {:>7}", "-------");
    }
    println!();

    for (factor, cat_scores) in results {
        // Compute overall score.
        let mut overall_f1_sum = 0.0f64;
        let mut overall_count = 0usize;
        for (f1_sum, count) in cat_scores.values() {
            overall_f1_sum += f1_sum;
            overall_count += count;
        }
        #[allow(clippy::cast_precision_loss)]
        let overall_pct = if overall_count == 0 {
            0.0
        } else {
            overall_f1_sum / overall_count as f64 * 100.0
        };

        print!("  {:>6.2}", factor);
        print!("  {:>6.1}%", overall_pct);

        for (key, _label) in &cats {
            #[allow(clippy::cast_precision_loss)]
            let cat_pct = cat_scores
                .get(*key)
                .map(|(f1_sum, count)| {
                    if *count == 0 {
                        0.0
                    } else {
                        f1_sum / *count as f64 * 100.0
                    }
                })
                .unwrap_or(0.0);
            print!("  {:>6.1}%", cat_pct);
        }
        println!();
    }

    println!("========================================================================");

    // Suppress unused variable warnings from cat_labels.
    let _ = cat_labels;
}
