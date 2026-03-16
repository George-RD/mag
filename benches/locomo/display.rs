use std::collections::BTreeMap;

use crate::types::{CategoryResult, LoCoMoSummary};

pub(crate) fn pct(correct: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        correct as f64 / total as f64 * 100.0
    }
}

fn avg_f1(cat: &CategoryResult) -> f64 {
    if cat.total == 0 {
        0.0
    } else {
        cat.f1_sum / cat.total as f64 * 100.0
    }
}

fn avg_evidence(cat: &CategoryResult) -> f64 {
    if cat.total == 0 {
        0.0
    } else {
        cat.evidence_recall_sum / cat.total as f64 * 100.0
    }
}

fn grade(percentage: f64) -> &'static str {
    if percentage >= 90.0 {
        "A"
    } else if percentage >= 75.0 {
        "B"
    } else if percentage >= 60.0 {
        "C"
    } else if percentage >= 40.0 {
        "D"
    } else {
        "F"
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
    println!("------------------------------------------------------------------------");
}

pub(crate) fn record_result(
    by_category: &mut BTreeMap<String, CategoryResult>,
    category: &str,
    passed: bool,
    f1: f64,
    evidence_recall: f64,
    detail: Option<String>,
) {
    let entry = by_category.entry(category.to_string()).or_default();
    entry.total += 1;
    if passed {
        entry.correct += 1;
    }
    entry.f1_sum += f1;
    entry.evidence_recall_sum += evidence_recall;
    if let Some(line) = detail {
        entry.details.push(line);
    }
}
