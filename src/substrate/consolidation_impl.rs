use crate::memory_core::EventType;
use crate::substrate::traits::{ConsolidationStrategy, MemoryStore};
use crate::substrate::types::ConsolidationReport;
use anyhow::Result;
use async_trait::async_trait;

pub struct DedupConsolidation {
    pub min_cluster_size: usize,
}

#[async_trait]
impl ConsolidationStrategy for DedupConsolidation {
    fn name(&self) -> &str {
        "dedup"
    }

    async fn run(&self, store: &dyn MemoryStore, dry_run: bool) -> Result<ConsolidationReport> {
        let mut memories_examined: usize = 0;
        let mut memories_modified: usize = 0;
        let mut detail = serde_json::Map::new();

        for event_type in EventType::types_with_dedup_threshold() {
            let event_type: EventType = event_type;
            let event_type_name = event_type.to_string();
            if let Some(threshold) = event_type.dedup_threshold() {
                let result = store
                    .compact(&event_type_name, threshold, self.min_cluster_size, dry_run)
                    .await?;

                if let Some(examined) = result.get("memories_examined").and_then(|v| v.as_u64()) {
                    memories_examined += usize::try_from(examined).unwrap_or(0);
                }
                if let Some(modified) = result.get("memories_modified").and_then(|v| v.as_u64()) {
                    memories_modified += usize::try_from(modified).unwrap_or(0);
                }
                detail.insert(event_type_name, result);
            }
        }

        Ok(ConsolidationReport {
            strategy: self.name().to_string(),
            memories_examined,
            memories_modified,
            dry_run,
            detail: serde_json::Value::Object(detail),
        })
    }
}

pub struct CompactConsolidation {
    pub prune_days: i64,
    pub max_summaries: i64,
}

#[async_trait]
impl ConsolidationStrategy for CompactConsolidation {
    fn name(&self) -> &str {
        "compact"
    }

    async fn run(&self, store: &dyn MemoryStore, dry_run: bool) -> Result<ConsolidationReport> {
        if dry_run {
            return Err(anyhow::anyhow!(
                "CompactConsolidation does not support dry_run"
            ));
        }

        let result = store
            .consolidate(self.prune_days, self.max_summaries)
            .await?;

        let memories_examined = usize::try_from(
            result
                .get("memories_examined")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        )
        .unwrap_or(0);
        let memories_modified = usize::try_from(
            result
                .get("memories_modified")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        )
        .unwrap_or(0);

        let detail = result.as_object().cloned().unwrap_or_default();

        Ok(ConsolidationReport {
            strategy: self.name().to_string(),
            memories_examined,
            memories_modified,
            dry_run,
            detail: serde_json::Value::Object(detail),
        })
    }
}

pub struct AutoRelateConsolidation {
    pub count_threshold: usize,
}

#[async_trait]
impl ConsolidationStrategy for AutoRelateConsolidation {
    fn name(&self) -> &str {
        "auto-relate"
    }

    async fn run(&self, store: &dyn MemoryStore, dry_run: bool) -> Result<ConsolidationReport> {
        let result = store.auto_compact(self.count_threshold, dry_run).await?;

        let memories_examined = usize::try_from(
            result
                .get("memories_examined")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        )
        .unwrap_or(0);
        let memories_modified = usize::try_from(
            result
                .get("memories_modified")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        )
        .unwrap_or(0);

        Ok(ConsolidationReport {
            strategy: self.name().to_string(),
            memories_examined,
            memories_modified,
            dry_run,
            detail: result,
        })
    }
}
