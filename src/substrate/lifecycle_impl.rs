use crate::substrate::traits::{LifecyclePolicy, MemoryStore};
use crate::substrate::types::ScoredCandidate;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
impl LifecyclePolicy for crate::substrate::traits::TtlExpirationPolicy {
    fn name(&self) -> &str {
        "ttl"
    }

    fn is_alive(&self, candidate: &ScoredCandidate) -> bool {
        let expires_at = match candidate.result.metadata.get("expires_at") {
            Some(v) => match v.as_str() {
                Some(s) => s,
                None => return true,
            },
            None => return true,
        };

        let expires = match DateTime::parse_from_rfc3339(expires_at) {
            Ok(dt) => dt.with_timezone(&Utc),
            Err(_) => return true,
        };

        expires >= Utc::now()
    }

    async fn sweep(&self, store: &dyn MemoryStore) -> Result<usize> {
        store.sweep_expired().await
    }

    fn apply_decay(&self, _candidate: &mut ScoredCandidate, _now_secs: u64) {
        // No-op: TTL is binary alive/dead, not graduated decay.
    }
}
