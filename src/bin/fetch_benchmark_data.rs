use anyhow::Result;
use clap::{Parser, ValueEnum};
use mag::benchmarking::{self, DatasetKind};
use serde_json::json;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DatasetArg {
    Longmemeval,
    Locomo,
    All,
}

#[derive(Debug, Parser)]
#[command(name = "fetch_benchmark_data")]
#[command(about = "Download benchmark datasets into the MAG cache")]
struct Args {
    #[arg(long, value_enum, default_value_t = DatasetArg::All)]
    dataset: DatasetArg,
    #[arg(long)]
    force_refresh: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut resolved = Vec::new();

    for dataset in match args.dataset {
        DatasetArg::Longmemeval => vec![DatasetKind::LongMemEval],
        DatasetArg::Locomo => vec![DatasetKind::LoCoMo10],
        DatasetArg::All => vec![DatasetKind::LongMemEval, DatasetKind::LoCoMo10],
    } {
        let artifact =
            benchmarking::resolve_dataset(dataset, None, args.force_refresh, false).await?;
        resolved.push(json!({
            "dataset": match dataset {
                DatasetKind::LongMemEval => "longmemeval",
                DatasetKind::LoCoMo10 => "locomo",
            },
            "source_url": artifact.source_url,
            "path": artifact.path,
        }));
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({ "datasets": resolved }))?
    );
    Ok(())
}
