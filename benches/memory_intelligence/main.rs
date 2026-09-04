//! Memory-intelligence evaluation harness.
//!
//! Seeds a small annotated corpus through `LocalMemoryRuntime::store_raw` and
//! scores what MAG does with it: entity tagging, relative-date retrieval,
//! inferred relationships, TTL expiry, auto-supersession, clustering,
//! provenance on derived rows, and question retrieval with abstention.
//!
//! The binary writes no files. `--json` prints the summary to stdout and
//! suppresses human output; persisting a run is `scripts/memory-intelligence-eval.sh`'s
//! job.
//!
//! Seeding uses `store_raw`, not `store`: `store` routes through the
//! compatibility `PlaceholderPipeline`, which prefixes `"processed: "` into the
//! stored content, the FTS index and the embedding.

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::{Parser, ValueEnum};
use uuid::Uuid;

use mag::LocalMemoryRuntime;
use mag::benchmarking;
use mag::memory_core::MemoryInput;
use mag::memory_core::embedder::{Embedder, PlaceholderEmbedder};
use mag::memory_core::embedding_model::{
    LocalResourceExpectations, RetrieverArtifactChecksum, RetrieverModelProfile,
    RetrieverModelProfileSpec,
};

#[allow(dead_code)]
#[path = "../bench_utils/mod.rs"]
mod bench_utils;
mod dataset;
mod families;
mod metrics;
mod report;

use bench_utils::metrics::PeakRss;
use families::{FamilyOutcome, SeededGroup, SupersessionPair};
use report::{EvalSummary, FamilySummary, ModelProfileSummary, ValidationSummary};

/// Families in report order.
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

/// Seed group holding the corpus every read-side family scores against.
const GROUP_CORPUS: &str = "corpus";
/// Seed group holding the labelled duplicate clusters.
const GROUP_GROUPING: &str = "grouping";
/// Seed group holding the TTL cases.
const GROUP_LIFECYCLE: &str = "lifecycle";
/// Seed group holding the near-duplicates `auto_compact` retires.
const GROUP_PROVENANCE: &str = "provenance";

/// Seconds to wait after seeding the lifecycle group so its one-second TTL
/// elapses before the sweep.
const LIFECYCLE_TTL_WAIT_SECONDS: u64 = 2;

/// Pinned artefact metadata for the default MAG embedding model.
///
/// Checksums and byte counts were taken from the files MAG downloads into
/// `$MAG_DATA_ROOT/models/bge-small-en-v1.5-int8/`. `peak_ram_bytes` is an
/// expectation, not a guarantee: on a four-CPU Linux container, full
/// `--embedder bge-small` runs reached a `VmHWM` between 92356 KB (release) and
/// 148536 KB (debug), and the recorded figure rounds the largest of those up to
/// 256 MiB of headroom.
// Only the profile-backed embedder and its unit test read this.
#[cfg_attr(not(feature = "real-embeddings"), allow(dead_code))]
const BGE_SMALL_PROFILE: RetrieverModelProfileSpec = RetrieverModelProfileSpec {
    model_id: "Xenova/bge-small-en-v1.5",
    revision: "ea104dacec62c0de699686887e3f920caeb4f3e3",
    checksums: &[
        RetrieverArtifactChecksum {
            artifact: "model.onnx",
            sha256: "bf64d05457cb391fa88d045faf5927a15ea36d96228ddf23ea970087afdc1197",
        },
        RetrieverArtifactChecksum {
            artifact: "tokenizer.json",
            sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
        },
    ],
    role: "dense-embedding",
    runtime: "onnxruntime",
    quantization: "int8",
    output_dimensions: 384,
    pooling: "mean",
    query_transform: "none",
    document_transform: "none",
    max_input_tokens: 512,
    licence: "MIT",
    local_resources: LocalResourceExpectations {
        model_disk_bytes: 33_760_831 + 711_396,
        peak_ram_bytes: 256 * 1024 * 1024,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
enum EmbedderChoice {
    /// SHA-256 stand-in, 32 dimensions. Builds and runs with no features.
    Placeholder,
    /// The default MAG ONNX model, reached through the legacy `Embedder` trait.
    #[default]
    BgeSmall,
    /// The same ONNX model behind a pinned `RetrieverModelProfile`.
    ProfileBgeSmall,
}

impl EmbedderChoice {
    fn as_str(self) -> &'static str {
        match self {
            Self::Placeholder => "placeholder",
            Self::BgeSmall => "bge-small",
            Self::ProfileBgeSmall => "profile-bge-small",
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "memory_intelligence_eval")]
#[command(about = "Memory-intelligence evaluation for MAG")]
struct Args {
    /// Directory holding dataset.json and manifest.json.
    #[arg(long, default_value = "data/memory_intelligence_eval/v1")]
    dataset: PathBuf,
    /// Embedding model under test.
    #[arg(long, value_enum, default_value_t = EmbedderChoice::default())]
    embedder: EmbedderChoice,
    /// Score only the named family. Repeatable; defaults to all families.
    #[arg(long)]
    family: Vec<String>,
    /// Print the JSON summary to stdout and suppress human output.
    #[arg(long)]
    json: bool,
    /// Run schema validation and exit.
    #[arg(long)]
    validate_only: bool,
    /// Print the header and the summary table without the per-family detail.
    #[arg(long)]
    quiet: bool,
}

/// The embedding backend under test.
enum Backend {
    /// Anything reached through the role-neutral `Embedder` trait. MAG wraps it
    /// in its own compatibility adapter, so `model_profile()` is `None`.
    Legacy(Arc<dyn Embedder>),
    #[cfg(feature = "real-embeddings")]
    /// A harness-owned `EmbeddingModel` carrying a pinned profile.
    Profile(Arc<dyn mag::memory_core::EmbeddingModel>),
}

impl Backend {
    fn dimension(&self) -> usize {
        match self {
            Self::Legacy(embedder) => embedder.dimension(),
            #[cfg(feature = "real-embeddings")]
            Self::Profile(model) => model.dimension(),
        }
    }

    /// Embeds one string so a lazily initialised model finishes loading.
    fn warm_up(&self) -> Result<()> {
        match self {
            Self::Legacy(embedder) => embedder.embed("warm up").map(|_| ()),
            #[cfg(feature = "real-embeddings")]
            Self::Profile(model) => model
                .embed_for(mag::memory_core::EmbeddingInputKind::Document, "warm up")
                .map(|_| ()),
        }
    }

    fn profile(&self) -> Option<RetrieverModelProfile> {
        match self {
            Self::Legacy(_) => None,
            #[cfg(feature = "real-embeddings")]
            Self::Profile(model) => model.model_profile(),
        }
    }

    fn open(&self, path: PathBuf) -> Result<LocalMemoryRuntime> {
        match self {
            Self::Legacy(embedder) => LocalMemoryRuntime::new_with_path(path, embedder.clone()),
            #[cfg(feature = "real-embeddings")]
            Self::Profile(model) => {
                let storage =
                    mag::memory_core::storage::sqlite::SqliteStorage::new_with_path_and_embedding_model(
                        path,
                        model.clone(),
                    )?;
                Ok(LocalMemoryRuntime::from_storage(storage))
            }
        }
    }
}

/// Wraps `OnnxEmbedder` in an `EmbeddingModel` that declares a pinned profile.
///
/// bge-small-en-v1.5 applies no query or document prefix, so the input kind does
/// not change the text that reaches the model.
#[cfg(feature = "real-embeddings")]
struct ProfileEmbeddingModel {
    inner: mag::memory_core::OnnxEmbedder,
    profile: RetrieverModelProfile,
    identity: String,
}

#[cfg(feature = "real-embeddings")]
impl mag::memory_core::EmbeddingModel for ProfileEmbeddingModel {
    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn embedding_space_identity(&self) -> &str {
        &self.identity
    }

    fn model_profile(&self) -> Option<RetrieverModelProfile> {
        Some(self.profile)
    }

    fn embed_for(
        &self,
        _input: mag::memory_core::EmbeddingInputKind,
        text: &str,
    ) -> Result<Vec<f32>> {
        self.inner.embed(text)
    }

    fn embed_batch_for(
        &self,
        _input: mag::memory_core::EmbeddingInputKind,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>> {
        self.inner.embed_batch(texts)
    }
}

fn build_backend(choice: EmbedderChoice) -> Result<(Backend, String)> {
    match choice {
        EmbedderChoice::Placeholder => Ok((
            Backend::Legacy(Arc::new(PlaceholderEmbedder)),
            "placeholder".to_string(),
        )),
        #[cfg(feature = "real-embeddings")]
        EmbedderChoice::BgeSmall => Ok((
            Backend::Legacy(Arc::new(mag::memory_core::OnnxEmbedder::new()?)),
            "bge-small-en-v1.5-int8".to_string(),
        )),
        #[cfg(feature = "real-embeddings")]
        EmbedderChoice::ProfileBgeSmall => {
            let profile = RetrieverModelProfile::new(BGE_SMALL_PROFILE)
                .context("pinned bge-small retriever profile is invalid")?;
            let model = ProfileEmbeddingModel {
                inner: mag::memory_core::OnnxEmbedder::new()?,
                identity: profile.embedding_space_identity(),
                profile,
            };
            Ok((
                Backend::Profile(Arc::new(model)),
                "bge-small-en-v1.5-int8 (profile-backed)".to_string(),
            ))
        }
        #[cfg(not(feature = "real-embeddings"))]
        other => bail!(
            "--embedder {} needs the real-embeddings feature; rebuild with --features real-embeddings or use --embedder placeholder",
            other.as_str()
        ),
    }
}

/// Reads the embedding-space identity MAG persisted for a database.
///
/// This is a metadata read for the run header, not a behavioural observation:
/// no public runtime method exposes `runtime_metadata`, and reconstructing the
/// string in the harness would duplicate production logic.
fn persisted_embedding_space(db_path: &Path) -> Result<String> {
    let conn = rusqlite::Connection::open(db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    conn.query_row(
        "SELECT value FROM runtime_metadata WHERE key = 'embedding_space_identity'",
        [],
        |row| row.get::<_, String>(0),
    )
    .context("database has no persisted embedding-space identity")
}

/// Converts a `day_offset` into the ISO 8601 string `MemoryInput::referenced_date`
/// accepts. That field sets the `event_at` column, which is what the relative-date
/// filters in `advanced_search` compare against.
fn referenced_date(day_offset: Option<i64>, today: NaiveDate) -> Option<String> {
    let offset = day_offset?;
    let date = today.checked_add_signed(chrono::Duration::try_days(offset)?)?;
    Some(format!("{}T12:00:00Z", date.format("%Y-%m-%d")))
}

async fn seed_group(
    backend: &Backend,
    db_path: PathBuf,
    seeds: &[&dataset::Seed],
    today: NaiveDate,
) -> Result<SeededGroup> {
    let runtime = backend.open(db_path)?;
    let mut key_to_id = BTreeMap::new();
    let mut id_to_key = BTreeMap::new();

    for seed in seeds {
        let id = Uuid::new_v4().to_string();
        let mut input = MemoryInput {
            content: seed.content.clone(),
            id: Some(id.clone()),
            tags: seed.tags.clone(),
            importance: seed.importance,
            session_id: Some(seed.session_id.clone()),
            ttl_seconds: seed.ttl_seconds,
            referenced_date: referenced_date(seed.day_offset, today),
            source_type: Some("memory_intelligence_eval".to_string()),
            ..MemoryInput::default()
        };
        input.apply_event_type_defaults(Some(&seed.event_type));
        runtime
            .store_raw(&id, &seed.content, &input)
            .await
            .with_context(|| format!("failed to seed {}", seed.key))?;
        key_to_id.insert(seed.key.clone(), id.clone());
        id_to_key.insert(id, seed.key.clone());
    }

    let stored = families::stored_ids(&runtime).await?;
    let retained_ids: BTreeSet<String> = key_to_id
        .values()
        .filter(|id| stored.contains(*id))
        .cloned()
        .collect();

    Ok(SeededGroup {
        runtime,
        key_to_id,
        id_to_key,
        content_to_key: families::content_index(seeds),
        seeded: seeds.len(),
        retained: retained_ids.len(),
        retained_ids,
    })
}

fn seeds_in_group<'a>(data: &'a dataset::Dataset, group: &str) -> Vec<&'a dataset::Seed> {
    data.seed.iter().filter(|s| s.group == group).collect()
}

struct RunOutput {
    outcomes: Vec<FamilyOutcome>,
    embedding_space: String,
    seeded: usize,
    retained: usize,
}

#[allow(clippy::too_many_lines)]
async fn run(
    data: &dataset::Dataset,
    backend: &Backend,
    db_dir: &Path,
    selected: &BTreeSet<String>,
    rss: &mut PeakRss,
) -> Result<RunOutput> {
    let today = chrono::Local::now().date_naive();
    let mut outcomes: Vec<FamilyOutcome> = Vec::new();
    let mut seeded = 0usize;
    let mut retained = 0usize;
    let mut embedding_space: Option<String> = None;

    let note_space = |path: &Path, space: &mut Option<String>| {
        if space.is_none()
            && let Ok(identity) = persisted_embedding_space(path)
        {
            *space = Some(identity);
        }
    };

    let needs_corpus = ["entities", "temporal", "relationships", "questions"]
        .iter()
        .any(|name| selected.contains(*name));

    if needs_corpus {
        let path = db_dir.join("corpus.db");
        let group = seed_group(
            backend,
            path.clone(),
            &seeds_in_group(data, GROUP_CORPUS),
            today,
        )
        .await?;
        note_space(&path, &mut embedding_space);
        seeded += group.seeded;
        retained += group.retained;
        rss.sample();

        if selected.contains("entities") {
            outcomes.push(families::entities(&group, &data.entities).await?);
        }
        if selected.contains("temporal") {
            outcomes.push(families::temporal(&group, &data.temporal).await?);
        }
        if selected.contains("relationships") {
            outcomes.push(families::relationships(&group, &data.relationships).await?);
        }
        if selected.contains("questions") {
            outcomes.push(families::questions(&group, &data.questions).await?);
        }
        rss.sample();
    }

    if selected.contains("lifecycle") {
        let path = db_dir.join("lifecycle.db");
        let group = seed_group(
            backend,
            path.clone(),
            &seeds_in_group(data, GROUP_LIFECYCLE),
            today,
        )
        .await?;
        note_space(&path, &mut embedding_space);
        seeded += group.seeded;
        retained += group.retained;
        tokio::time::sleep(std::time::Duration::from_secs(LIFECYCLE_TTL_WAIT_SECONDS)).await;
        outcomes.push(families::lifecycle(&group, &data.lifecycle).await?);
        rss.sample();
    }

    if selected.contains("supersession") {
        let mut groups = Vec::new();
        for case in &data.supersession {
            let seeds: Vec<&dataset::Seed> = data
                .seed
                .iter()
                .filter(|s| s.key == case.old || s.key == case.new)
                .collect();
            let group_name = seeds
                .first()
                .map_or_else(|| case.kind.clone(), |seed| seed.group.clone());
            let path = db_dir.join(format!("{group_name}.db"));
            let group = seed_group(backend, path.clone(), &seeds, today).await?;
            note_space(&path, &mut embedding_space);
            seeded += group.seeded;
            retained += group.retained;
            groups.push(group);
        }
        let pairs: Vec<SupersessionPair<'_>> = data
            .supersession
            .iter()
            .zip(groups.iter())
            .map(|(case, group)| SupersessionPair { case, group })
            .collect();
        outcomes.push(families::supersession(&pairs).await?);
        rss.sample();
    }

    if selected.contains("grouping") {
        let path = db_dir.join("grouping.db");
        let group = seed_group(
            backend,
            path.clone(),
            &seeds_in_group(data, GROUP_GROUPING),
            today,
        )
        .await?;
        note_space(&path, &mut embedding_space);
        seeded += group.seeded;
        retained += group.retained;
        outcomes.push(families::grouping(&group, &data.grouping).await?);
        rss.sample();
    }

    if selected.contains("provenance") {
        // The provenance family applies auto_compact, which supersedes rows, so
        // it needs its own database. It also needs its own seeds: the grouping
        // seeds are deliberately distinct enough to survive content dedup, and
        // auto_compact retires nothing when no near-duplicate reaches the store.
        let path = db_dir.join("provenance.db");
        let group = seed_group(
            backend,
            path.clone(),
            &seeds_in_group(data, GROUP_PROVENANCE),
            today,
        )
        .await?;
        note_space(&path, &mut embedding_space);
        seeded += group.seeded;
        retained += group.retained;
        outcomes.push(families::provenance(&group, &data.provenance).await?);
        rss.sample();
    }

    outcomes.sort_by_key(|outcome| {
        ALL_FAMILIES
            .iter()
            .position(|name| *name == outcome.name)
            .unwrap_or(usize::MAX)
    });

    Ok(RunOutput {
        outcomes,
        embedding_space: embedding_space
            .unwrap_or_else(|| "unavailable (no database was opened)".to_string()),
        seeded,
        retained,
    })
}

fn profile_summary(profile: &RetrieverModelProfile) -> ModelProfileSummary {
    let spec = profile.metadata();
    ModelProfileSummary {
        model_id: spec.model_id.to_string(),
        revision: spec.revision.to_string(),
        role: spec.role.to_string(),
        runtime: spec.runtime.to_string(),
        quantization: spec.quantization.to_string(),
        output_dimensions: spec.output_dimensions,
        pooling: spec.pooling.to_string(),
        query_transform: spec.query_transform.to_string(),
        document_transform: spec.document_transform.to_string(),
        max_input_tokens: spec.max_input_tokens,
        licence: spec.licence.to_string(),
        checksums: spec
            .checksums
            .iter()
            .map(|checksum| (checksum.artifact.to_string(), checksum.sha256.to_string()))
            .collect(),
        expected_model_disk_bytes: spec.local_resources.model_disk_bytes,
        expected_peak_ram_bytes: spec.local_resources.peak_ram_bytes,
    }
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
        "memory_intelligence_eval",
        "repo-local",
        "data/memory_intelligence_eval/v1/dataset.json",
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
            report::print_validation(&data.dataset_version, &sha256, &checks, validity);
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
    let (backend, embedder_name) = build_backend(args.embedder)?;
    backend.warm_up().context("embedder warm-up failed")?;
    let model_load_ms = load_started.elapsed().as_secs_f64() * 1000.0;
    rss.sample();

    let db_dir = std::env::temp_dir().join(format!("mag-mieval-{}", std::process::id()));
    std::fs::create_dir_all(&db_dir)
        .with_context(|| format!("failed to create {}", db_dir.display()))?;

    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(run(&data, &backend, &db_dir, &selected, &mut rss));
    let _ = std::fs::remove_dir_all(&db_dir);
    let output = result?;
    rss.sample();

    let profile = backend.profile();
    let overall = report::overall(&output.outcomes, selected.len(), data.unimplemented.len());
    let summary = EvalSummary {
        metadata,
        dataset_version: data.dataset_version.clone(),
        dataset_sha256: sha256,
        schema_validity_percentage: validity,
        schema_checks: checks,
        embedder_name,
        embedding_dimension: backend.dimension(),
        embedding_space_identity: output.embedding_space,
        model_profile: profile.as_ref().map(profile_summary),
        model_profile_reason: if profile.is_some() {
            None
        } else {
            Some(format!(
                "--embedder {} reaches MAG through the role-neutral Embedder trait, whose compatibility adapter records no revision or checksum metadata",
                args.embedder.as_str()
            ))
        },
        tokens: None,
        tokens_reason: "no generative model in the evaluated path".to_string(),
        model_load_ms,
        total_duration_seconds: started.elapsed().as_secs_f64(),
        peak_rss_kb: rss.peak_kb,
        seeded_memories: output.seeded,
        retained_memories: output.retained,
        overall_percentage: overall.percentage,
        scored_families: overall.scored,
        selected_families: overall.selected,
        total_families: overall.total,
        not_measurable_families: overall.not_measurable,
        unimplemented_families: overall.unimplemented,
        overall_grade: overall.grade,
        overall_scope: overall.scope,
        families: output
            .outcomes
            .iter()
            .map(|outcome| (outcome.name.to_string(), FamilySummary::from(outcome)))
            .collect(),
        unimplemented: data.unimplemented.clone(),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        report::print_report(&summary, &output.outcomes, args.quiet);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_bge_small_profile_is_valid() {
        let profile = RetrieverModelProfile::new(BGE_SMALL_PROFILE).expect("profile validates");
        let spec = profile.metadata();
        assert_eq!(spec.output_dimensions, 384);
        assert_eq!(spec.checksums.len(), 2);
        for checksum in spec.checksums {
            assert_eq!(checksum.sha256.len(), 64);
            assert!(checksum.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        }
        assert!(
            profile
                .embedding_space_identity()
                .starts_with("retriever-profile:v1")
        );
    }

    #[test]
    fn referenced_date_shifts_by_whole_days() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 10).expect("valid date");
        assert_eq!(
            referenced_date(Some(-3), today).as_deref(),
            Some("2026-03-07T12:00:00Z")
        );
        assert_eq!(
            referenced_date(Some(0), today).as_deref(),
            Some("2026-03-10T12:00:00Z")
        );
        assert_eq!(referenced_date(None, today), None);
    }

    #[test]
    fn every_dataset_group_is_covered_by_a_family() {
        let dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/memory_intelligence_eval/v1");
        let (data, _, _) = dataset::load(&dir).expect("dataset loads");
        let groups: BTreeSet<&str> = data.seed.iter().map(|s| s.group.as_str()).collect();
        let supersession_groups: BTreeSet<&str> = data
            .supersession
            .iter()
            .filter_map(|case| {
                data.seed
                    .iter()
                    .find(|s| s.key == case.old)
                    .map(|s| s.group.as_str())
            })
            .collect();
        for group in &groups {
            let covered = *group == GROUP_CORPUS
                || *group == GROUP_GROUPING
                || *group == GROUP_LIFECYCLE
                || *group == GROUP_PROVENANCE
                || supersession_groups.contains(group);
            assert!(covered, "seed group {group} is never opened by the harness");
        }
    }
}
