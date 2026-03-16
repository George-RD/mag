use anyhow::{Result, anyhow};

use crate::types::LoCoMoSample;

pub(crate) fn load_dataset(path: &std::path::Path) -> Result<Vec<LoCoMoSample>> {
    let file = std::fs::File::open(path)
        .map_err(|e| anyhow!("failed to open dataset at {}: {e}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let samples: Vec<LoCoMoSample> = serde_json::from_reader(reader)
        .map_err(|e| anyhow!("failed to parse dataset JSON: {e}"))?;
    Ok(samples)
}
