//! Recovered non-generative runtime diagnostic. Uses the same production runtime
//! and BGE adapter as MAG, in isolated throwaway databases; never touches user data.
use anyhow::{Context, Result, bail};
use clap::Parser;
use mag::benchmarking;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Instant;
mod backend;
mod dataset;
mod families;
mod metrics;
mod report;
mod resources;
mod runner;
#[path = "../bench_utils/stats.rs"]
mod stats;
use backend::EmbedderChoice;
use report::{EvalSummary, FamilySummary, ValidationSummary};
use resources::PeakRss;
const DEFAULT_DATASET_DIR: &str = "data/runtime_behaviour_eval/v1";
const ALL_FAMILIES: [&str; 8] = [
    "entities",
    "temporal",
    "relationships",
    "lifecycle",
    "supersession",
    "grouping",
    "provenance",
    "questions",
];
#[derive(Debug, Parser)]
#[command(
    name = "memory_runtime_eval",
    about = "Non-generative MAG runtime behaviour diagnostic"
)]
struct Args {
    #[arg(long, default_value = DEFAULT_DATASET_DIR)]
    dataset: PathBuf,
    #[arg(long, value_enum, default_value_t = EmbedderChoice::default())]
    embedder: EmbedderChoice,
    #[arg(long)]
    family: Vec<String>,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    validate_only: bool,
    #[arg(long)]
    quiet: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let started = Instant::now();
    let mut rss = PeakRss::default();
    rss.sample();

    let (data, manifest, sha256) = dataset::load(&args.dataset)?;
    let checks = dataset::validate(&data, &manifest, &sha256);
    let validity = dataset::validity_percentage(&checks);
    let valid = checks.iter().all(|check| check.passed);

    let metadata = benchmarking::benchmark_metadata_from_parts(
        "memory_runtime_eval",
        if args.dataset.as_path() == std::path::Path::new(DEFAULT_DATASET_DIR) {
            "repo-local"
        } else {
            "user-supplied"
        },
        &args.dataset.join("dataset.json").to_string_lossy(),
    );

    if args.validate_only || !valid {
        if args.json {
            let summary = ValidationSummary {
                metadata,
                dataset_version: data.dataset_version.clone(),
                dataset_sha256: sha256.clone(),
                schema_validity_percentage: validity,
                schema_checks: checks,
            };
            println!("{}", serde_json::to_string_pretty(&summary)?);
        } else {
            for check in &checks {
                println!("{}: {} {:?}", check.name, check.passed, check.detail);
            }
        }
        if !valid {
            std::process::exit(1);
        }
        return Ok(());
    }

    let selected: BTreeSet<String> = if args.family.is_empty() {
        ALL_FAMILIES
            .iter()
            .map(|name| (*name).to_string())
            .collect()
    } else {
        for name in &args.family {
            if !ALL_FAMILIES.contains(&name.as_str()) {
                bail!(
                    "unknown family: {name} (expected one of {})",
                    ALL_FAMILIES.join(", ")
                );
            }
        }
        args.family.iter().cloned().collect()
    };

    let load_started = Instant::now();
    let (backend, embedder_name) = backend::build(args.embedder)?;
    backend.warm_up().context("embedder warm-up failed")?;
    let model_load_ms = load_started.elapsed().as_secs_f64() * 1000.0;
    rss.sample();

    let db_dir = resources::Workspace::new()?;
    let runtime = tokio::runtime::Runtime::new()?;
    let output = runtime.block_on(runner::run(&data, &backend, &db_dir.0, &selected, &mut rss))?;
    rss.sample();

    let profile = backend.profile();
    let summary = EvalSummary {
        metadata,
        dataset_version: data.dataset_version.clone(),
        dataset_sha256: sha256,
        schema_validity_percentage: validity,
        schema_checks: checks,
        embedder_name,
        embedding_dimension: backend.dimension(),
        embedding_space_identity: output.embedding_space,
        model_profile: profile.as_ref().map(report::profile_summary),
        model_profile_reason: if profile.is_some() {
            None
        } else {
            Some("placeholder is a test stand-in without model/profile provenance".to_string())
        },
        tokens: None,
        tokens_reason: "no generative model in the evaluated path".to_string(),
        model_startup_and_warmup_ms: model_load_ms,
        total_duration_seconds: started.elapsed().as_secs_f64(),
        peak_rss_kb: rss.peak_kb,
        ram_measurement: resources::method().to_string(),
        seeded_memories: output.seeded,
        retained_memories: output.retained,
        selected_families: selected.len(),
        total_families: ALL_FAMILIES.len(),
        families: output
            .outcomes
            .iter()
            .map(|outcome| (outcome.name.to_string(), FamilySummary::from(outcome)))
            .collect(),
        historical_unimplemented_annotations: data.unimplemented.clone(),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        report::print_report(&summary, &output.outcomes, args.quiet);
    }

    Ok(())
}
