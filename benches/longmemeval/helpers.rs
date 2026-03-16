use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use anyhow::Result;

use crate::types::{CategoryResult, Hit, QuestionEvaluation};

// ── Peak RSS tracking ─────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub(crate) struct PeakRss {
    pub peak_kb: u64,
}

impl PeakRss {
    pub fn sample(&mut self) {
        if let Ok(kb) = current_rss_kb()
            && kb > self.peak_kb
        {
            self.peak_kb = kb;
        }
    }
}

#[cfg(target_os = "macos")]
fn current_rss_kb() -> Result<u64> {
    use std::process::Command;
    let pid = std::process::id();
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let kb: u64 = text.trim().parse()?;
    Ok(kb)
}

#[cfg(not(target_os = "macos"))]
fn current_rss_kb() -> Result<u64> {
    Ok(0)
}

// ── Formatting helpers ────────────────────────────────────────────────────

pub(crate) fn iso(ts: chrono::DateTime<chrono::Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(crate) fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub(crate) fn substring_match(hits: &[Hit], expected_substring: &str) -> bool {
    let expected = expected_substring.to_lowercase();
    hits.iter()
        .any(|hit| hit.content.to_lowercase().contains(expected.as_str()))
}

// ── Category and grading helpers ──────────────────────────────────────────

pub(crate) fn categories() -> Vec<(&'static str, &'static str)> {
    vec![
        ("information_extraction", "Information Extraction"),
        ("multi_session", "Multi-Session Reasoning"),
        ("temporal", "Temporal Reasoning"),
        ("knowledge_update", "Knowledge Update"),
        ("abstention", "Abstention"),
    ]
}

pub(crate) fn grade(percentage: f64) -> &'static str {
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

pub(crate) fn pct(correct: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    { correct as f64 / total as f64 * 100.0 }
}

pub(crate) fn summarize_totals(
    categories: &BTreeMap<String, CategoryResult>,
) -> (usize, usize, f64) {
    let mut total_correct = 0usize;
    let mut total_questions = 0usize;
    for cat in categories.values() {
        total_correct += cat.correct;
        total_questions += cat.total;
    }
    (
        total_correct,
        total_questions,
        pct(total_correct, total_questions),
    )
}

pub(crate) fn category_percentage(categories: &BTreeMap<String, CategoryResult>, key: &str) -> f64 {
    categories
        .get(key)
        .map(|cat| pct(cat.correct, cat.total))
        .unwrap_or(0.0)
}

pub(crate) fn compact_decimal(value: f64) -> String {
    let formatted = format!("{value:.2}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

// ── Result recording ──────────────────────────────────────────────────────

pub(crate) fn record_result(
    by_category: &mut BTreeMap<String, CategoryResult>,
    category: &str,
    passed: bool,
    detail: Option<String>,
) {
    let entry = by_category.entry(category.to_string()).or_default();
    entry.total += 1;
    if passed {
        entry.correct += 1;
    }
    if let Some(line) = detail {
        entry.details.push(line);
    }
}

pub(crate) fn question_type_for_category(category: &str) -> &'static str {
    match category {
        "information_extraction" | "multi_session" => "standard",
        "temporal" => "temporal",
        "knowledge_update" => "knowledge-update",
        "abstention" => "abstention",
        _ => "standard",
    }
}

// ── Question evaluation tracking ──────────────────────────────────────────

static BENCH_EVALS: OnceLock<Mutex<Vec<QuestionEvaluation>>> = OnceLock::new();

pub(crate) fn bench_evals() -> &'static Mutex<Vec<QuestionEvaluation>> {
    BENCH_EVALS.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn clear_question_evals() {
    match bench_evals().lock() {
        Ok(mut guard) => guard.clear(),
        Err(e) => {
            eprintln!("warning: failed to lock evaluation data: {e}");
        }
    }
}

pub(crate) fn snapshot_question_evals() -> Vec<QuestionEvaluation> {
    bench_evals()
        .lock()
        .unwrap_or_else(|e| panic!("evaluation data mutex poisoned: {e}"))
        .clone()
}

pub(crate) fn record_question_eval(
    category: &str,
    question: &str,
    expected: &str,
    actual: &str,
    substring_passed: bool,
) {
    let eval = QuestionEvaluation {
        category: category.to_string(),
        question_type: question_type_for_category(category).to_string(),
        question: question.to_string(),
        expected: expected.to_string(),
        actual: actual.to_string(),
        substring_passed,
    };
    match bench_evals().lock() {
        Ok(mut guard) => guard.push(eval),
        Err(e) => {
            eprintln!("warning: failed to lock evaluation data: {e}");
        }
    }
}
