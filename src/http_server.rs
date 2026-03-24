use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::routing::get;
use axum::{Json, Router};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::auth::AuthLayer;
use crate::daemon::DaemonInfo;
use crate::hook_handlers::{AppState, hook_router};
use crate::idle_timer::{IdleTimer, IdleTimerLayer};
use crate::mcp_server::McpMemoryServer;
use crate::memory_core::storage::SqliteStorage;

/// Configuration for the MAG HTTP daemon.
pub struct ServerConfig {
    pub port: u16,
    pub auth_token: String,
    pub idle_timeout_secs: u64,
    pub cross_encoder: bool,
}

/// Runs the MAG HTTP server with MCP-over-HTTP, hook endpoints, and health check.
///
/// This function blocks until the server shuts down (via ctrl-c or idle timeout).
pub async fn run_http_server(
    storage: SqliteStorage,
    #[cfg(feature = "real-embeddings")] onnx: Option<Arc<crate::memory_core::OnnxEmbedder>>,
    config: ServerConfig,
) -> Result<()> {
    // --- Cross-encoder reranking (optional) ---
    #[cfg(feature = "real-embeddings")]
    let storage = if config.cross_encoder {
        tracing::info!("Cross-encoder reranking enabled");
        let reranker = Arc::new(crate::memory_core::reranker::CrossEncoderReranker::new()?);
        reranker.warmup().await?;
        let reranker_for_tick = reranker.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                reranker_for_tick.maintenance_tick().await;
            }
        });
        storage.with_reranker(reranker)
    } else {
        storage
    };

    #[cfg(not(feature = "real-embeddings"))]
    if config.cross_encoder {
        anyhow::bail!("--cross-encoder requires the `real-embeddings` feature to be enabled");
    }

    // --- ONNX maintenance tick ---
    #[cfg(feature = "real-embeddings")]
    if let Some(ref onnx_ref) = onnx {
        let onnx_for_tick = onnx_ref.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                onnx_for_tick.maintenance_tick().await;
            }
        });
    }

    // --- SQLite periodic PRAGMA optimize ---
    {
        let storage_for_optimize = storage.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3600));
            loop {
                interval.tick().await;
                if let Err(e) = storage_for_optimize.optimize().await {
                    tracing::warn!("PRAGMA optimize failed: {e}");
                }
            }
        });
    }

    // --- Shutdown plumbing ---
    let shutdown_token = CancellationToken::new();

    // --- Idle timer ---
    let timer = IdleTimer::new(Duration::from_secs(config.idle_timeout_secs));
    timer.spawn_watchdog(shutdown_token.clone());

    // --- MCP over Streamable HTTP ---
    let mcp_storage = storage.clone();
    let mcp_service: StreamableHttpService<McpMemoryServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(McpMemoryServer::new(mcp_storage.clone())),
            Default::default(),
            StreamableHttpServerConfig {
                stateful_mode: true,
                cancellation_token: shutdown_token.child_token(),
                ..Default::default()
            },
        );

    // --- Axum router ---
    let app = Router::new()
        .nest_service("/mcp", mcp_service)
        .merge(hook_router().with_state(AppState {
            storage: storage.clone(),
        }))
        .route("/health", get(health_handler))
        .layer(AuthLayer::new(config.auth_token.clone()))
        .layer(IdleTimerLayer::new(timer));

    // --- Bind listener ---
    let listener = TcpListener::bind(format!("127.0.0.1:{}", config.port))
        .await
        .with_context(|| format!("binding to 127.0.0.1:{}", config.port))?;
    let local_addr = listener
        .local_addr()
        .context("getting local address from listener")?;
    tracing::info!("MAG daemon listening on {local_addr}");

    // --- Write daemon info to disk ---
    let info = DaemonInfo {
        port: local_addr.port(),
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        token: config.auth_token,
    };
    info.write().context("writing daemon info")?;

    // --- Ctrl-C handler ---
    let ctrl_c_token = shutdown_token.clone();
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("failed to listen for ctrl-c: {e}");
        }
        tracing::info!("Ctrl-C received, shutting down");
        ctrl_c_token.cancel();
    });

    // --- Serve ---
    axum::serve(listener, app)
        .with_graceful_shutdown(async move { shutdown_token.cancelled_owned().await })
        .await
        .context("running HTTP server")?;

    // --- Cleanup ---
    DaemonInfo::remove();
    tracing::info!("MAG daemon shut down cleanly");

    Ok(())
}

/// Health check handler — exempt from authentication (see [`AuthLayer`]).
async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
    }))
}
