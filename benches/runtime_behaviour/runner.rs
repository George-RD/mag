use crate::backend::Backend;
use crate::families::{self, FamilyOutcome, SeededGroup, SupersessionPair};
use crate::resources::PeakRss;
use crate::{ALL_FAMILIES, dataset};
use anyhow::{Context, Result};
use chrono::NaiveDate;
use mag::memory_core::MemoryInput;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;
const GROUP_CORPUS: &str = "corpus";
const GROUP_GROUPING: &str = "grouping";
const GROUP_LIFECYCLE: &str = "lifecycle";
const GROUP_PROVENANCE: &str = "provenance";
const LIFECYCLE_TTL_WAIT_SECONDS: u64 = 2;

/// Reads the embedding-space identity MAG persisted for a database.
///
/// This is a metadata read for the run header, not a behavioural observation:
/// no public runtime method exposes `runtime_metadata`, and reconstructing the
/// string in the harness would duplicate production logic.
async fn persisted_embedding_space(db_path: &Path) -> Result<String> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .with_context(|| format!("failed to open {}", db_path.display()))?;
        conn.query_row(
            "SELECT value FROM runtime_metadata WHERE key = 'embedding_space_identity'",
            [],
            |row| row.get::<_, String>(0),
        )
        .context("database has no persisted embedding-space identity")
    })
    .await
    .context("embedding-space metadata task failed")?
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
    let owned_backend = backend.clone();
    let runtime = tokio::task::spawn_blocking(move || owned_backend.open(db_path))
        .await
        .context("runtime initialization task failed")??;
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
            source_type: Some("memory_runtime_eval".to_string()),
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

pub struct RunOutput {
    pub outcomes: Vec<FamilyOutcome>,
    pub embedding_space: String,
    pub seeded: usize,
    pub retained: usize,
}

#[allow(clippy::too_many_lines)]
pub async fn run(
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
        note_space(&path, &mut embedding_space).await?;
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
        note_space(&path, &mut embedding_space).await?;
        seeded += group.seeded;
        retained += group.retained;
        tokio::time::sleep(std::time::Duration::from_secs(LIFECYCLE_TTL_WAIT_SECONDS)).await;
        outcomes.push(families::lifecycle(&group, &data.lifecycle).await?);
        rss.sample();
    }

    if selected.contains("supersession") {
        let mut groups = Vec::new();
        for (index, case) in data.supersession.iter().enumerate() {
            let seeds: Vec<&dataset::Seed> = data
                .seed
                .iter()
                .filter(|s| s.key == case.old || s.key == case.new)
                .collect();
            let path = db_dir.join(format!("supersession-{index}.db"));
            let group = seed_group(backend, path.clone(), &seeds, today).await?;
            note_space(&path, &mut embedding_space).await?;
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
        note_space(&path, &mut embedding_space).await?;
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
        note_space(&path, &mut embedding_space).await?;
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
        embedding_space: embedding_space.context("no database identity was observed")?,
        seeded,
        retained,
    })
}

async fn note_space(path: &Path, space: &mut Option<String>) -> Result<()> {
    let identity = persisted_embedding_space(path).await?;
    if let Some(expected) = space.as_ref() {
        anyhow::ensure!(
            expected == &identity,
            "seed groups have different embedding identities"
        );
    } else {
        *space = Some(identity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_offsets_use_the_supplied_anchor_and_reject_overflow() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 10).unwrap();
        assert_eq!(
            referenced_date(Some(-2), today).as_deref(),
            Some("2026-09-08T12:00:00Z")
        );
        assert!(referenced_date(None, today).is_none());
        assert!(referenced_date(Some(i64::MAX), today).is_none());
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use mag::memory_core::embedder::{Embedder, PlaceholderEmbedder};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    struct ExecutorGuard(std::thread::ThreadId);
    impl Embedder for ExecutorGuard {
        fn dimension(&self) -> usize {
            assert_ne!(
                std::thread::current().id(),
                self.0,
                "SQLite setup must not run on the async executor"
            );
            PlaceholderEmbedder.dimension()
        }
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            PlaceholderEmbedder.embed(text)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn review_sqlite_setup_leaves_the_executor() {
        let directory = tempfile::tempdir().unwrap();
        let backend = Backend::Placeholder(Arc::new(ExecutorGuard(std::thread::current().id())));
        let group = seed_group(
            &backend,
            directory.path().join("setup.db"),
            &[],
            NaiveDate::from_ymd_opt(2026, 9, 10).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(group.retained, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn review_locked_metadata_read_does_not_park_executor() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("metadata.db");
        let writer = rusqlite::Connection::open(&path).unwrap();
        writer.execute_batch("CREATE TABLE runtime_metadata (key TEXT PRIMARY KEY, value TEXT); INSERT INTO runtime_metadata VALUES ('embedding_space_identity', 'test-space'); BEGIN EXCLUSIVE;").unwrap();
        let (send_tick, receive_tick) = mpsc::channel();
        let release = std::thread::spawn(move || {
            let progressed = receive_tick.recv_timeout(Duration::from_secs(2)).is_ok();
            writer.execute_batch("COMMIT").unwrap();
            progressed
        });
        let (result, ()) = tokio::join!(persisted_embedding_space(&path), async move {
            tokio::task::yield_now().await;
            let _ = send_tick.send(());
        });
        assert!(
            release.join().unwrap(),
            "metadata read parked the async executor until the watchdog released SQLite"
        );
        assert_eq!(result.unwrap(), "test-space");
    }
}
