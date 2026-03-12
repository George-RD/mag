use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app_paths;

const LONGMEMEVAL_DATASET_URLS: &[&str] = &[
    "https://huggingface.co/datasets/LIXINYI33/longmemeval-s/resolve/main/longmemeval_s_cleaned.json",
    "https://huggingface.co/datasets/kellyhongg/cleaned-longmemeval-s/resolve/main/longmemeval_s_cleaned.json",
    "https://github.com/xiaowu0162/longmemeval-cleaned/raw/main/longmemeval_s_cleaned.json",
];
const LOCOMO_DATASET_URLS: &[&str] = &[
    "https://raw.githubusercontent.com/snap-research/locomo/main/data/locomo10.json",
    "https://github.com/snap-research/locomo/raw/main/data/locomo10.json",
];

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkMetadata {
    pub benchmark: String,
    pub command: String,
    pub date: String,
    pub commit: Option<String>,
    pub machine: String,
    pub dataset_source: String,
    pub dataset_path: String,
}

#[derive(Debug, Clone)]
pub struct DatasetArtifact {
    pub source_url: String,
    pub path: PathBuf,
    pub temporary: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum DatasetKind {
    LongMemEval,
    LoCoMo10,
}

impl DatasetKind {
    fn cache_subdir(self) -> &'static str {
        match self {
            Self::LongMemEval => "longmemeval",
            Self::LoCoMo10 => "locomo",
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::LongMemEval => "longmemeval_s_cleaned.json",
            Self::LoCoMo10 => "locomo10.json",
        }
    }

    fn source_urls(self) -> &'static [&'static str] {
        match self {
            Self::LongMemEval => LONGMEMEVAL_DATASET_URLS,
            Self::LoCoMo10 => LOCOMO_DATASET_URLS,
        }
    }
}

pub async fn resolve_dataset(
    kind: DatasetKind,
    dataset_path: Option<PathBuf>,
    force_refresh: bool,
    temporary: bool,
) -> Result<DatasetArtifact> {
    if dataset_path.is_some() && (force_refresh || temporary) {
        return Err(anyhow!(
            "--dataset-path cannot be combined with --force-refresh or --temp-dataset"
        ));
    }
    if let Some(path) = dataset_path {
        validate_json_file(&path)?;
        return Ok(DatasetArtifact {
            source_url: "user-supplied".to_string(),
            path,
            temporary: false,
        });
    }

    let cache_path = if temporary {
        temporary_dataset_path(kind)
    } else {
        benchmark_cache_path(kind)?
    };
    if force_refresh || !cache_path.exists() {
        let source_url = download_from_sources(kind.source_urls(), &cache_path).await?;
        validate_json_file(&cache_path)?;
        if !temporary {
            write_source_metadata(&cache_path, &source_url)?;
        }
        return Ok(DatasetArtifact {
            source_url,
            path: cache_path,
            temporary,
        });
    }
    validate_json_file(&cache_path)?;
    Ok(DatasetArtifact {
        source_url: read_source_metadata(&cache_path)
            .unwrap_or_else(|| kind.source_urls()[0].to_string()),
        path: cache_path,
        temporary,
    })
}

pub fn benchmark_cache_path(kind: DatasetKind) -> Result<PathBuf> {
    let cache_root = app_paths::resolve_app_paths()?.benchmark_root;
    Ok(cache_root.join(kind.cache_subdir()).join(kind.filename()))
}

pub fn benchmark_metadata(benchmark: &str, dataset: &DatasetArtifact) -> BenchmarkMetadata {
    benchmark_metadata_from_parts(
        benchmark,
        &dataset.source_url,
        &dataset.path.display().to_string(),
    )
}

pub fn benchmark_metadata_from_parts(
    benchmark: &str,
    dataset_source: &str,
    dataset_path: &str,
) -> BenchmarkMetadata {
    BenchmarkMetadata {
        benchmark: benchmark.to_string(),
        command: sanitize_command(std::env::args()),
        date: Utc::now().to_rfc3339(),
        commit: git_commit(),
        machine: machine_descriptor(),
        dataset_source: dataset_source.to_string(),
        dataset_path: sanitize_dataset_path(dataset_path),
    }
}

fn validate_json_file(path: &Path) -> Result<()> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open dataset at {}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);
    let mut de = serde_json::Deserializer::from_reader(&mut reader);
    serde::de::IgnoredAny::deserialize(&mut de)
        .with_context(|| format!("failed to parse JSON dataset at {}", path.display()))?;
    de.end()
        .with_context(|| format!("trailing content in JSON dataset at {}", path.display()))?;
    Ok(())
}

async fn download_from_sources(urls: &[&str], path: &Path) -> Result<String> {
    let mut failures = Vec::new();
    for url in urls {
        match download_file(url, path).await {
            Ok(()) => return Ok((*url).to_string()),
            Err(err) => failures.push(format!("{url}: {err}")),
        }
    }
    Err(anyhow!(
        "failed to download benchmark dataset from any public source:\n{}",
        failures.join("\n")
    ))
}

async fn download_file(url: &str, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create dataset cache dir {}", parent.display()))?;
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build benchmark download client")?;

    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to download benchmark dataset from {url}"))?
        .error_for_status()
        .with_context(|| format!("benchmark dataset download failed for {url}"))?;
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read benchmark dataset body from {url}"))?;

    let mut part_name = path
        .file_name()
        .ok_or_else(|| anyhow!("invalid dataset file name for {}", path.display()))?
        .to_os_string();
    part_name.push(".part");
    let part_path = path.with_file_name(part_name);
    let write_result: Result<()> = async {
        tokio::fs::write(&part_path, &bytes)
            .await
            .with_context(|| format!("failed to write {}", part_path.display()))?;
        let validate_path = part_path.clone();
        tokio::task::spawn_blocking(move || validate_json_file(&validate_path))
            .await
            .context("dataset validation task failed")??;
        tokio::fs::rename(&part_path, path)
            .await
            .with_context(|| format!("failed to finalize {}", path.display()))?;
        Ok(())
    }
    .await;
    if let Err(err) = write_result {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(err);
    }
    Ok(())
}

fn read_source_metadata(path: &Path) -> Option<String> {
    let meta_path = source_metadata_path(path);
    let text = std::fs::read_to_string(meta_path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn write_source_metadata(path: &Path, source_url: &str) -> Result<()> {
    std::fs::write(source_metadata_path(path), source_url)
        .with_context(|| format!("failed to write source metadata for {}", path.display()))
}

fn source_metadata_path(path: &Path) -> PathBuf {
    let mut suffix = path.file_name().unwrap_or_default().to_os_string();
    suffix.push(".source-url");
    path.with_file_name(suffix)
}

fn temporary_dataset_path(kind: DatasetKind) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%d%H%M%S");
    let filename = format!("{stamp}-{}", kind.filename());
    std::env::temp_dir()
        .join("mag-benchmarks")
        .join(kind.cache_subdir())
        .join(filename)
}

impl DatasetArtifact {
    pub fn cleanup(&mut self) -> Result<()> {
        if !self.temporary || !self.path.exists() {
            return Ok(());
        }
        std::fs::remove_file(&self.path).with_context(|| {
            format!("failed to remove temporary dataset {}", self.path.display())
        })?;
        self.temporary = false;
        Ok(())
    }
}

impl Drop for DatasetArtifact {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn sanitize_command(args: impl IntoIterator<Item = String>) -> String {
    args.into_iter()
        .map(|arg| quote_shell_arg(&sanitize_arg(&arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_arg(arg: &str) -> String {
    if looks_like_path(arg) {
        "<redacted_path>".to_string()
    } else {
        arg.to_string()
    }
}

fn quote_shell_arg(arg: &str) -> String {
    if arg
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\\' | '$' | '`'))
    {
        format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

fn sanitize_dataset_path(dataset_path: &str) -> String {
    Path::new(dataset_path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "<redacted_path>".to_string())
}

fn looks_like_path(arg: &str) -> bool {
    Path::new(arg).is_absolute() || arg.starts_with('~') || arg.contains('/') || arg.contains('\\')
}

fn git_commit() -> Option<String> {
    command_stdout("git", &["rev-parse", "HEAD"])
}

fn machine_descriptor() -> String {
    let mut parts = vec![
        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        format!("{} CPU", num_cpus::get()),
    ];
    if let Some(model) = command_stdout("sysctl", &["-n", "hw.model"]) {
        parts.insert(0, model);
    }
    parts.join(", ")
}

fn command_stdout(cmd: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(cmd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn resolve_dataset_uses_explicit_path_without_downloading() {
        let path =
            std::env::temp_dir().join(format!("mag-benchmark-fixture-{}.json", Uuid::new_v4()));
        std::fs::write(&path, r#"[{"question":"hi"}]"#).unwrap();

        let dataset = resolve_dataset(DatasetKind::LongMemEval, Some(path.clone()), false, false)
            .await
            .unwrap();
        assert_eq!(dataset.path, path);
        assert_eq!(dataset.source_url, "user-supplied");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_metadata_round_trip() {
        let path = std::env::temp_dir().join(format!("mag-benchmark-meta-{}.json", Uuid::new_v4()));
        write_source_metadata(&path, "https://example.com/dataset.json").unwrap();
        assert_eq!(
            read_source_metadata(&path).as_deref(),
            Some("https://example.com/dataset.json")
        );
        let _ = std::fs::remove_file(source_metadata_path(&path));
    }
}
