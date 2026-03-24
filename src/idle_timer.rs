// Allow dead code: this module is built in parallel with its consumers (daemon HTTP server).
#![allow(dead_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio_util::sync::CancellationToken;

fn epoch_millis() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis();
    // Current epoch millis fits comfortably in u64 (won't overflow until ~year 584 million).
    #[allow(clippy::cast_possible_truncation)]
    let result = millis as u64;
    result
}

#[derive(Clone)]
pub struct IdleTimer {
    last_request: Arc<AtomicU64>,
    timeout_millis: u64,
}

impl IdleTimer {
    pub fn new(timeout: Duration) -> Self {
        #[allow(clippy::cast_possible_truncation)]
        let timeout_millis = timeout.as_millis() as u64;
        Self {
            last_request: Arc::new(AtomicU64::new(epoch_millis())),
            timeout_millis,
        }
    }

    pub fn touch(&self) {
        self.last_request.store(epoch_millis(), Ordering::Relaxed);
    }

    pub fn is_expired(&self) -> bool {
        let last = self.last_request.load(Ordering::Relaxed);
        epoch_millis().saturating_sub(last) > self.timeout_millis
    }

    pub fn spawn_watchdog(&self, shutdown: CancellationToken) {
        let timer = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if timer.is_expired() {
                            tracing::info!("Idle timeout reached, shutting down");
                            shutdown.cancel();
                            return;
                        }
                    }
                    () = shutdown.cancelled() => {
                        return;
                    }
                }
            }
        });
    }
}

// --- Tower middleware ---

#[derive(Clone)]
pub struct IdleTimerLayer {
    timer: IdleTimer,
}

impl IdleTimerLayer {
    pub fn new(timer: IdleTimer) -> Self {
        Self { timer }
    }
}

impl<S> tower::Layer<S> for IdleTimerLayer {
    type Service = IdleTimerService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        IdleTimerService {
            inner,
            timer: self.timer.clone(),
        }
    }
}

#[derive(Clone)]
pub struct IdleTimerService<S> {
    inner: S,
    timer: IdleTimer,
}

impl<S, ReqBody> tower::Service<axum::http::Request<ReqBody>> for IdleTimerService<S>
where
    S: tower::Service<axum::http::Request<ReqBody>>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: axum::http::Request<ReqBody>) -> Self::Future {
        self.timer.touch();
        let fut = self.inner.call(req);
        Box::pin(fut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_expired_immediately_after_creation() {
        let timer = IdleTimer::new(Duration::from_secs(60));
        assert!(!timer.is_expired());
    }

    #[test]
    fn touch_resets_expiry() {
        let timer = IdleTimer::new(Duration::from_secs(60));
        timer.touch();
        assert!(!timer.is_expired());
    }

    #[tokio::test]
    async fn expires_after_timeout() {
        let timer = IdleTimer::new(Duration::from_millis(1));
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(timer.is_expired());
    }
}
