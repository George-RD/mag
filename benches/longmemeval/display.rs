use std::collections::BTreeMap;

use crate::helpers::{categories, grade, pct};
use crate::types::CategoryResult;

pub(crate) fn print_results(results: &BTreeMap<String, CategoryResult>) -> (usize, usize, f64) {
    println!();
    println!("============================================================");
    println!("  ROMEGA LongMemEval Benchmark Results");
    println!("============================================================");

    let mut total_correct = 0usize;
    let mut total_questions = 0usize;
    for (key, label) in categories() {
        if let Some(cat) = results.get(key) {
            let percent = pct(cat.correct, cat.total);
            let filled = (percent / 5.0).floor() as usize;
            let bar = format!(
                "{}{}",
                "#".repeat(filled),
                "-".repeat(20usize.saturating_sub(filled))
            );
            println!(
                "\n  {label:30} {:3}/{:3}  [{bar}] {:5.1}%  ({})",
                cat.correct,
                cat.total,
                percent,
                grade(percent)
            );
            for line in &cat.details {
                println!("{line}");
            }
            total_correct += cat.correct;
            total_questions += cat.total;
        }
    }

    let overall = pct(total_correct, total_questions);
    println!("\n------------------------------------------------------------");
    println!("  OVERALL: {total_correct}/{total_questions} = {overall:.1}%");
    println!("------------------------------------------------------------");
    (total_correct, total_questions, overall)
}

pub(crate) fn print_side_by_side_results(
    substring: &BTreeMap<String, CategoryResult>,
    llm: &BTreeMap<String, CategoryResult>,
) -> ((usize, usize, f64), (usize, usize, f64)) {
    println!();
    println!(
        "===================================================================================================================="
    );
    println!("  ROMEGA LongMemEval Benchmark Results (Substring vs LLM Judge)");
    println!(
        "===================================================================================================================="
    );
    println!(
        "  {:30} {:>24} {:>24}",
        "Category", "Substring", "LLM Judge"
    );

    let mut sub_correct = 0usize;
    let mut sub_total = 0usize;
    let mut llm_correct = 0usize;
    let mut llm_total = 0usize;

    for (key, label) in categories() {
        let sub = substring.get(key).cloned().unwrap_or_default();
        let llm_cat = llm.get(key).cloned().unwrap_or_default();
        let sub_pct = pct(sub.correct, sub.total);
        let llm_pct = pct(llm_cat.correct, llm_cat.total);
        println!(
            "  {label:30} {:>3}/{:<3} {:>6.1}% {:>3}   {:>3}/{:<3} {:>6.1}% {:>3}",
            sub.correct,
            sub.total,
            sub_pct,
            grade(sub_pct),
            llm_cat.correct,
            llm_cat.total,
            llm_pct,
            grade(llm_pct),
        );
        sub_correct += sub.correct;
        sub_total += sub.total;
        llm_correct += llm_cat.correct;
        llm_total += llm_cat.total;
    }

    let sub_overall = pct(sub_correct, sub_total);
    let llm_overall = pct(llm_correct, llm_total);
    println!(
        "--------------------------------------------------------------------------------------------------------------------"
    );
    println!(
        "  {:30} {:>3}/{:<3} {:>6.1}% {:>3}   {:>3}/{:<3} {:>6.1}% {:>3}",
        "OVERALL",
        sub_correct,
        sub_total,
        sub_overall,
        grade(sub_overall),
        llm_correct,
        llm_total,
        llm_overall,
        grade(llm_overall),
    );
    println!(
        "--------------------------------------------------------------------------------------------------------------------"
    );

    (
        (sub_correct, sub_total, sub_overall),
        (llm_correct, llm_total, llm_overall),
    )
}
