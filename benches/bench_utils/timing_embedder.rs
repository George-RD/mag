use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use anyhow::Result;
use mag::memory_core::embedder::Embedder;

/// Wraps an inner `Embedder`, recording timing and call counts atomically.
#[allow(dead_code)]
pub struct TimingEmbedder {
    inner: Arc<dyn Embedder>,
    calls: AtomicU64,
    total_us: AtomicU64,
}

impl std::fmt::Debug for TimingEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimingEmbedder")
            .field("calls", &self.calls.load(Ordering::Relaxed))
            .field("total_us", &self.total_us.load(Ordering::Relaxed))
            .finish()
    }
}

#[allow(dead_code)]
impl TimingEmbedder {
    pub fn new(inner: Arc<dyn Embedder>) -> Self {
        Self {
            inner,
            calls: AtomicU64::new(0),
            total_us: AtomicU64::new(0),
        }
    }

    pub fn total_calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }

    /// Total embed time in milliseconds.
    pub fn total_embed_ms(&self) -> u64 {
        self.total_us.load(Ordering::Relaxed) / 1000
    }

    /// Average embed time per call in milliseconds.
    #[allow(clippy::cast_precision_loss)]
    pub fn avg_embed_ms(&self) -> f64 {
        let calls = self.calls.load(Ordering::Relaxed);
        if calls == 0 {
            0.0
        } else {
            self.total_us.load(Ordering::Relaxed) as f64 / calls as f64 / 1000.0
        }
    }
}

impl Embedder for TimingEmbedder {
    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let t = Instant::now();
        let r = self.inner.embed(text)?;
        #[allow(clippy::cast_possible_truncation)]
        self.total_us
            .fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(r)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let t = Instant::now();
        let r = self.inner.embed_batch(texts)?;
        #[allow(clippy::cast_possible_truncation)]
        self.total_us
            .fetch_add(t.elapsed().as_micros() as u64, Ordering::Relaxed);
        self.calls.fetch_add(texts.len() as u64, Ordering::Relaxed);
        Ok(r)
    }
}
