use anyhow::Result;
#[cfg(feature = "real-embeddings")]
use clap::{Parser, ValueEnum};
#[cfg(feature = "real-embeddings")]
use mag::benchmarking::{self, DatasetKind};
#[cfg(feature = "real-embeddings")]
use serde_json::json;

#[cfg(feature = "real-embeddings")]
#[derive(Debug, Clone, Copy, ValueEnum)]
enum DatasetArg {
    Longmemeval,
    Locomo,
    All,
}

#[cfg(feature = "real-embeddings")]
#[derive(Debug, Parser)]
#[command(name = "fetch_benchmark_data")]
#[command(about = "Download benchmark datasets into the MAG cache")]
struct Args {
    #[arg(long, value_enum, default_value_t = DatasetArg::All)]
    dataset: DatasetArg,
    #[arg(long)]
    force_refresh: bool,
}

#[cfg(feature = "real-embeddings")]
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

#[cfg(not(feature = "real-embeddings"))]
fn main() -> Result<()> {
    Err(anyhow::anyhow!(
        "fetch_benchmark_data requires the `real-embeddings` feature"
    ))
}
