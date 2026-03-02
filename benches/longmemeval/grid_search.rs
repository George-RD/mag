use std::cmp::Ordering;
use std::time::Instant;

use anyhow::{Result, anyhow};

use romega_memory::memory_core::storage::sqlite::SqliteStorage;
use romega_memory::memory_core::{OnnxEmbedder, ScoringParams};

use crate::helpers::{PeakRss, category_percentage, compact_decimal, summarize_totals, truncate};
use crate::local::{run_benchmark, seed_memories};
use crate::types::{
    GridSearchResult, GridSearchResultSummary, GridSearchSummary, ScoringParamsSnapshot,
};

pub(crate) fn grid_search_label(params: &ScoringParams) -> String {
    format!(
        "vec={}_decay={}_wo={}_gn={}_if={}_ab={}",
        compact_decimal(params.rrf_weight_vec),
        compact_decimal(params.time_decay_days),
        compact_decimal(params.word_overlap_weight),
        compact_decimal(params.graph_neighbor_factor),
        compact_decimal(params.importance_floor),
        compact_decimal(params.abstention_min_text),
    )
}

fn grid_search_params() -> Vec<(String, ScoringParams)> {
    let mut combinations = Vec::new();
    for rrf_weight_vec in [1.0, 1.5, 2.0, 2.5] {
        for time_decay_days in [0.0, 15.0, 30.0, 60.0, 120.0] {
            for word_overlap_weight in [0.0, 0.25, 0.5, 0.75] {
                for graph_neighbor_factor in [0.0, 0.2, 0.4, 0.6] {
                    for importance_floor in [0.3, 0.5, 0.7] {
                        for abstention_min_text in [0.25, 0.30, 0.35] {
                            let params = ScoringParams {
                                rrf_weight_vec,
                                time_decay_days,
                                word_overlap_weight,
                                graph_neighbor_factor,
                                importance_floor,
                                abstention_min_text,
                                ..ScoringParams::default()
                            };
                            let label = grid_search_label(&params);
                            combinations.push((label, params));
                        }
                    }
                }
            }
        }
    }
    combinations
}

fn sort_grid_results(results: &mut [GridSearchResult]) {
    results.sort_by(|left, right| {
        right.total_correct.cmp(&left.total_correct).then_with(|| {
            right
                .overall_percentage
                .partial_cmp(&left.overall_percentage)
                .unwrap_or(Ordering::Equal)
        })
    });
}

pub(crate) fn format_scoring_params_literal(params: &ScoringParams) -> String {
    format!(
        "ScoringParams {{\n    rrf_k: {:.1},\n    rrf_weight_vec: {:.2},\n    rrf_weight_fts: {:.2},\n    abstention_min_text: {:.2},\n    graph_neighbor_factor: {:.2},\n    graph_min_edge_weight: {:.2},\n    word_overlap_weight: {:.2},\n    jaccard_weight: {:.2},\n    importance_floor: {:.2},\n    importance_scale: {:.2},\n    context_tag_weight: {:.2},\n    time_decay_days: {:.1},\n    priority_base: {:.2},\n    priority_scale: {:.2},\n    feedback_heavy_suppress: {:.2},\n    feedback_strong_suppress: {:.2},\n    feedback_positive_scale: {:.2},\n    feedback_positive_cap: {:.2},\n    feedback_heavy_threshold: {},\n    neighbor_word_overlap_weight: {:.2},\n    neighbor_importance_floor: {:.2},\n    neighbor_importance_scale: {:.2},\n    graph_seed_min: {},\n    graph_seed_max: {},\n}}",
        params.rrf_k,
        params.rrf_weight_vec,
        params.rrf_weight_fts,
        params.abstention_min_text,
        params.graph_neighbor_factor,
        params.graph_min_edge_weight,
        params.word_overlap_weight,
        params.jaccard_weight,
        params.importance_floor,
        params.importance_scale,
        params.context_tag_weight,
        params.time_decay_days,
        params.priority_base,
        params.priority_scale,
        params.feedback_heavy_suppress,
        params.feedback_strong_suppress,
        params.feedback_positive_scale,
        params.feedback_positive_cap,
        params.feedback_heavy_threshold,
        params.neighbor_word_overlap_weight,
        params.neighbor_importance_floor,
        params.neighbor_importance_scale,
        params.graph_seed_min,
        params.graph_seed_max,
    )
}

fn as_grid_search_summary(result: &GridSearchResult) -> GridSearchResultSummary {
    GridSearchResultSummary {
        label: result.label.clone(),
        params: ScoringParamsSnapshot::from(&result.params),
        total_correct: result.total_correct,
        total_questions: result.total_questions,
        overall_percentage: result.overall_percentage,
        categories: result.categories.clone(),
        duration_ms: result.duration_ms,
    }
}

pub(crate) fn build_grid_search_summary(
    results: &[GridSearchResult],
    duration_seconds: f64,
) -> Result<GridSearchSummary> {
    let mut ranked = results.to_vec();
    sort_grid_results(&mut ranked);
    ranked
        .first()
        .ok_or_else(|| anyhow!("grid search produced no results"))?;
    let top_10 = ranked
        .iter()
        .take(10)
        .map(as_grid_search_summary)
        .collect::<Vec<_>>();
    let all_results = ranked
        .iter()
        .map(as_grid_search_summary)
        .collect::<Vec<_>>();

    Ok(GridSearchSummary {
        grid_size: ranked.len(),
        duration_seconds,
        top_10,
        results: all_results,
    })
}

pub(crate) fn print_grid_search_report(
    results: &[GridSearchResult],
    duration_seconds: f64,
) -> Result<()> {
    let mut ranked = results.to_vec();
    sort_grid_results(&mut ranked);
    let best = ranked
        .first()
        .ok_or_else(|| anyhow!("grid search produced no results"))?;
    let default_label = grid_search_label(&ScoringParams::default());
    let default_result = ranked.iter().find(|result| result.label == default_label);

    println!();
    println!(
        "========================================================================================================================================"
    );
    println!("  ROMEGA LongMemEval Grid Search Results");
    println!(
        "========================================================================================================================================"
    );
    println!("Grid size: {} combinations", ranked.len());
    println!("Total duration: {duration_seconds:.1}s");
    println!();
    println!(
        "  {:>4} {:<56} {:>8} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "Rank", "Label", "Overall", "IE", "MS", "TR", "KU", "AB"
    );
    for (index, result) in ranked.iter().take(10).enumerate() {
        println!(
            "  {:>4} {:<56} {:>7.1}% {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
            index + 1,
            truncate(result.label.as_str(), 56),
            result.overall_percentage,
            category_percentage(&result.categories, "information_extraction"),
            category_percentage(&result.categories, "multi_session"),
            category_percentage(&result.categories, "temporal"),
            category_percentage(&result.categories, "knowledge_update"),
            category_percentage(&result.categories, "abstention"),
        );
    }

    println!();
    println!("Best config label: {}", best.label);
    println!(
        "Best overall: {}/{} = {:.1}%",
        best.total_correct, best.total_questions, best.overall_percentage
    );
    println!();
    println!("Best ScoringParams (copy-paste):");
    println!("{}", format_scoring_params_literal(&best.params));

    if let Some(default_result) = default_result {
        let correct_delta = best.total_correct as isize - default_result.total_correct as isize;
        let pct_delta = best.overall_percentage - default_result.overall_percentage;
        println!();
        println!(
            "Vs default ({}): {:+} correct, {:+.1} percentage points",
            default_result.label, correct_delta, pct_delta
        );
    }

    Ok(())
}

pub(crate) async fn run_grid_search(verbose: bool) -> Result<Vec<GridSearchResult>> {
    let parameter_sets = grid_search_params();
    let total = parameter_sets.len();
    let mut results = Vec::with_capacity(total);

    let embedder = std::sync::Arc::new(OnnxEmbedder::new()?);
    for (index, (label, params)) in parameter_sets.into_iter().enumerate() {
        let start = Instant::now();
        let storage = SqliteStorage::new_in_memory_with_embedder(embedder.clone())?
            .with_scoring_params(params.clone());
        let mut rss = PeakRss::default();
        rss.sample();

        seed_memories(&storage, &mut rss).await?;
        let categories = run_benchmark(
            &storage,
            false,
            &mut rss,
            params.abstention_min_text as f32,
            3,
        )
        .await?;
        let (total_correct, total_questions, overall_percentage) = summarize_totals(&categories);
        let duration_ms = start.elapsed().as_millis();

        eprintln!(
            "[{}/{}] {}: {}/{} = {:.1}%",
            index + 1,
            total,
            label,
            total_correct,
            total_questions,
            overall_percentage
        );
        if verbose {
            eprintln!("  duration={duration_ms} ms peak_rss={} KB", rss.peak_kb);
        }

        results.push(GridSearchResult {
            label,
            params,
            total_correct,
            total_questions,
            overall_percentage,
            categories,
            duration_ms,
        });
    }

    Ok(results)
}
