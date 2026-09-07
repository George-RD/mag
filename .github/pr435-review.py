from pathlib import Path
import os
import sys

SOURCE = Path('src/memory_core/embedder.rs')
TESTS = r'''

#[cfg(all(test, feature = "real-embeddings"))]
mod artifact_regressions {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
    use std::time::Duration;

    const ABC_SHA256: &str =
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const CHECKSUMS: ModelArtifactChecksums = ModelArtifactChecksums {
        model: ABC_SHA256,
        model_data: Some(ABC_SHA256),
        tokenizer: ABC_SHA256,
    };

    // An independent OS thread also serves current-thread Tokio tests. No
    // external network, production model, or shared environment is involved.
    struct ArtifactServer {
        url: String,
        hits: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }

    impl ArtifactServer {
        fn new(body: &'static [u8]) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let hits = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let thread_hits = Arc::clone(&hits);
            let thread_stop = Arc::clone(&stop);
            let worker = std::thread::spawn(move || {
                while !thread_stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                            stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                            let mut request = Vec::new();
                            let mut byte = [0_u8; 1];
                            while !request.ends_with(b"\r\n\r\n") {
                                if stream.read(&mut byte).unwrap_or(0) == 0 {
                                    break;
                                }
                                request.push(byte[0]);
                            }
                            if !request.ends_with(b"\r\n\r\n") {
                                continue;
                            }
                            thread_hits.fetch_add(1, Ordering::SeqCst);
                            let header = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            stream.write_all(header.as_bytes()).unwrap();
                            stream.write_all(body).unwrap();
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        Err(error) => panic!("artifact server failed: {error}"),
                    }
                }
            });
            Self { url, hits, stop, worker: Some(worker) }
        }

        fn hits(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }
    }

    impl Drop for ArtifactServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(worker) = self.worker.take() {
                let result = worker.join();
                if !std::thread::panicking() {
                    result.unwrap();
                }
            }
        }
    }

    fn seed_cache(directory: &Path) {
        std::fs::write(directory.join("model.onnx"), b"abc").unwrap();
        std::fs::write(directory.join("tokenizer.json"), b"abc").unwrap();
    }

    fn ensure_pinned(directory: &Path, server: &ArtifactServer) -> Result<ModelFiles> {
        ensure_model_files_blocking(
            directory.to_path_buf(), &server.url, None, &server.url, Some(CHECKSUMS),
        )
    }

    fn assert_cached_pinned_artifacts() {
        let directory = tempfile::tempdir().unwrap();
        let server = ArtifactServer::new(b"abc");
        seed_cache(directory.path());
        let files = ensure_pinned(directory.path(), &server).unwrap();
        assert_eq!(std::fs::read(files.model_path).unwrap(), b"abc");
        assert_eq!(std::fs::read(files.tokenizer_path).unwrap(), b"abc");
        assert_eq!(server.hits(), 0, "verified cached files must not be downloaded");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cached_pinned_artifacts_work_inside_current_thread_runtime() {
        assert_cached_pinned_artifacts();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cached_pinned_artifacts_work_inside_multithread_runtime() {
        assert_cached_pinned_artifacts();
    }

    #[tokio::test]
    async fn cached_pinned_artifacts_work_inside_spawn_blocking() {
        tokio::task::spawn_blocking(assert_cached_pinned_artifacts).await.unwrap();
    }

    #[test]
    fn cached_pinned_artifacts_work_without_runtime() {
        assert_cached_pinned_artifacts();
    }

    fn assert_cold_pinned_artifacts() {
        let directory = tempfile::tempdir().unwrap();
        let server = ArtifactServer::new(b"abc");
        let files = ensure_pinned(directory.path(), &server).unwrap();
        assert_eq!(std::fs::read(files.model_path).unwrap(), b"abc");
        assert_eq!(std::fs::read(files.tokenizer_path).unwrap(), b"abc");
        assert_eq!(server.hits(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cold_pinned_artifacts_download_inside_current_thread_runtime() {
        assert_cold_pinned_artifacts();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cold_pinned_artifacts_download_inside_multithread_runtime() {
        assert_cold_pinned_artifacts();
    }

    #[test]
    fn cold_pinned_artifacts_download_without_runtime() {
        assert_cold_pinned_artifacts();
    }

    #[tokio::test]
    async fn corrupt_cached_artifact_is_replaced_with_verified_download() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model.onnx");
        std::fs::write(&path, b"corrupt cached bytes").unwrap();
        let server = ArtifactServer::new(b"abc");
        ensure_model_artifact(&server.url, &path, Some(ABC_SHA256)).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        assert_eq!(server.hits(), 1);
        ensure_model_artifact(&server.url, &path, Some(ABC_SHA256)).await.unwrap();
        assert_eq!(server.hits(), 1, "valid cache should be reused");
    }

    #[tokio::test]
    async fn mismatched_download_is_rejected_and_removed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model.onnx");
        let server = ArtifactServer::new(b"wrong download");
        let error = ensure_model_artifact(&server.url, &path, Some(ABC_SHA256))
            .await.unwrap_err();
        assert!(error.to_string().contains("model artifact checksum mismatch"));
        assert!(!path.exists(), "unverified bytes must not remain cached");
        assert!(!directory.path().join("model.onnx.part").exists());
        assert_eq!(server.hits(), 1);
    }

    #[tokio::test]
    async fn failed_corrupt_cache_recovery_does_not_leave_invalid_artifact() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model.onnx");
        std::fs::write(&path, b"old corrupt bytes").unwrap();
        let server = ArtifactServer::new(b"new corrupt bytes");
        assert!(ensure_model_artifact(&server.url, &path, Some(ABC_SHA256)).await.is_err());
        assert!(!path.exists());
        assert_eq!(server.hits(), 1);
    }

    #[tokio::test]
    async fn later_cache_replacement_is_reverified_for_model_and_tokenizer() {
        let directory = tempfile::tempdir().unwrap();
        let server = ArtifactServer::new(b"abc");
        seed_cache(directory.path());
        ensure_pinned(directory.path(), &server).unwrap();
        for (index, name) in ["model.onnx", "tokenizer.json"].iter().enumerate() {
            let path = directory.path().join(name);
            std::fs::write(&path, b"changed after verification").unwrap();
            ensure_pinned(directory.path(), &server).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"abc");
            assert_eq!(server.hits(), index + 1);
        }
    }

    #[tokio::test]
    async fn pinned_sidecar_is_checked_and_repaired() {
        let directory = tempfile::tempdir().unwrap();
        let server = ArtifactServer::new(b"abc");
        seed_cache(directory.path());
        let sidecar = directory.path().join("weights.bin");
        std::fs::write(&sidecar, b"invalid sidecar").unwrap();
        let data_url = format!("{}/weights.bin", server.url);
        let files = ensure_model_files_blocking(
            directory.path().to_path_buf(), &server.url, Some(&data_url),
            &server.url, Some(CHECKSUMS),
        ).unwrap();
        assert_eq!(files.model_data_path.as_ref(), Some(&sidecar));
        assert_eq!(std::fs::read(&sidecar).unwrap(), b"abc");
        assert_eq!(server.hits(), 1);
    }

    #[tokio::test]
    async fn pinned_sidecar_without_checksum_is_rejected_even_when_cached() {
        let directory = tempfile::tempdir().unwrap();
        let server = ArtifactServer::new(b"abc");
        seed_cache(directory.path());
        std::fs::write(directory.path().join("weights.bin"), b"abc").unwrap();
        let data_url = format!("{}/weights.bin", server.url);
        let error = ensure_model_files_blocking(
            directory.path().to_path_buf(), &server.url, Some(&data_url),
            &server.url, Some(ModelArtifactChecksums { model_data: None, ..CHECKSUMS }),
        ).unwrap_err();
        assert!(error.to_string().contains("missing a SHA-256 checksum"));
        assert_eq!(server.hits(), 0);
    }

    #[tokio::test]
    async fn unpinned_custom_cache_keeps_legacy_no_download_behavior() {
        let directory = tempfile::tempdir().unwrap();
        let server = ArtifactServer::new(b"abc");
        seed_cache(directory.path());
        std::fs::write(directory.path().join("model.onnx"), b"custom model").unwrap();
        let files = ensure_model_files_blocking(
            directory.path().to_path_buf(), &server.url, None, &server.url, None,
        ).unwrap();
        assert_eq!(std::fs::read(files.model_path).unwrap(), b"custom model");
        assert_eq!(server.hits(), 0);
    }
}
'''

BLOCKING = r'''#[cfg(feature = "real-embeddings")]
fn ensure_model_files_blocking(
    model_dir: PathBuf,
    model_url: &str,
    model_data_url: Option<&str>,
    tokenizer_url: &str,
    artifact_checksums: Option<ModelArtifactChecksums>,
) -> Result<ModelFiles> {
    let files = model_files_for_dir(model_dir.clone(), model_data_url);
    if cached_model_files_are_ready(&files, artifact_checksums)? {
        return Ok(files);
    }

    let download = move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to create temporary tokio runtime for model download")?;
        runtime.block_on(ensure_model_files_async(
            model_dir,
            model_url,
            model_data_url,
            tokenizer_url,
            artifact_checksums,
        ))
    };

    // The synchronous Embedder compatibility API can be called from a Tokio
    // task as well as spawn_blocking. Never nest block_on in an async context;
    // current-thread runtimes cannot use block_in_place either. A scoped worker
    // owns and drops the download runtime outside the caller's async context.
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("mag-model-download".into())
                .spawn_scoped(scope, download)
                .context("failed to spawn model download worker")?
                .join()
                .map_err(|_| anyhow!("model download worker panicked"))?
        })
    } else {
        download()
    }
}

'''

CACHED = r'''#[cfg(feature = "real-embeddings")]
fn cached_model_files_are_ready(
    files: &ModelFiles,
    artifact_checksums: Option<ModelArtifactChecksums>,
) -> Result<bool> {
    let data_checksum = if files.model_data_path.is_some() {
        artifact_checksums
            .map(|checksums| {
                checksums.model_data.ok_or_else(|| {
                    anyhow!("pinned model data artifact is missing a SHA-256 checksum")
                })
            })
            .transpose()?
    } else {
        None
    };
    let artifacts = [
        (Some(&files.model_path), artifact_checksums.map(|c| c.model)),
        (files.model_data_path.as_ref(), data_checksum),
        (Some(&files.tokenizer_path), artifact_checksums.map(|c| c.tokenizer)),
    ];
    for (path, checksum) in artifacts {
        let Some(path) = path else { continue };
        if !path.try_exists()
            .with_context(|| format!("failed to check model artifact {}", path.display()))?
        {
            return Ok(false);
        }
        // Reverify on every session load, not on every embedding. A directory-
        // only memo would trust files replaced after the previous verification.
        if let Some(expected) = checksum
            && sha256_file_blocking(path)? != expected
        {
            return Ok(false);
        }
    }
    Ok(true)
}

'''

phase = sys.argv[1]
if phase == 'tests':
    source = SOURCE.read_text()
    assert 'mod artifact_regressions' not in source
    SOURCE.write_text(source + TESTS)
elif phase == 'fix':
    source = SOURCE.read_text()
    start = source.index('#[cfg(feature = "real-embeddings")]\nfn ensure_model_files_blocking(')
    end = source.index('#[cfg(feature = "real-embeddings")]\nasync fn ensure_model_files_async(', start)
    source = source[:start] + BLOCKING + source[end:]
    start = source.index('#[cfg(feature = "real-embeddings")]\nfn model_files_exist(')
    end = source.index('#[cfg(feature = "real-embeddings")]\nfn model_files_for_dir(', start)
    source = source[:start] + CACHED + source[end:]
    assert 'model_files_exist' not in source
    SOURCE.write_text(source)
    ci = Path('.github/workflows/ci.yml')
    text = ci.read_text()
    assert '\npermissions:' not in text
    ci.write_text(text.replace('\nenv:\n', '\npermissions:\n  contents: read\n\nenv:\n', 1))
elif phase == 'evidence':
    run = os.environ['GITHUB_RUN_ID']
    sha = os.environ['GITHUB_SHA']
    url = f'https://github.com/George-RD/mag/actions/runs/{run}'
    review = Path('meta/reviews/pr435-embedding-space-write-safety.md')
    text = review.read_text().replace('## Reproducible evidence', '## Committed regressions and historical run evidence')
    text += f'''

## Review follow-up: 7 September 2026

The cached pinned-artifact path created a nested Tokio runtime even when no
network operation was needed. The regression
`memory_core::embedder::artifact_regressions::cached_pinned_artifacts_work_inside_current_thread_runtime`
failed on the pre-fix source with `Cannot start a runtime from within a runtime`.
The verification run [review TDD and engineering gates]({url}) applied only the
tests first, required that specific runtime failure (not a build failure), and
then applied the fix. Its source/staging head was `{sha}`; this is patched-worktree
evidence, not a claim that that head already contained the production fix.

Verified caches are checked synchronously without creating a runtime. Cold or
corrupt-cache downloads invoked through the synchronous compatibility API use a
scoped worker when a Tokio handle is present. The worker owns and drops its
runtime outside the caller's async context; plain synchronous callers keep the
direct path. Production async entrypoints should continue using `spawn_blocking`.

Fourteen committed, hermetic `artifact_regressions` tests cover cached loads in
current-thread/multithread Tokio, plain sync and `spawn_blocking`; cold downloads
in both Tokio flavors and plain sync; bad-cache replacement; post-download
checksum rejection and removal; failed recovery; re-verification after model or
tokenizer replacement; external-data checksums; and unpinned-cache compatibility.
They use an independent loopback HTTP fixture, not production model downloads.
Re-run with `cargo test --all-features --lib artifact_regressions`.

CI now declares `permissions: contents: read`. The strict Clippy command remains
unchanged: the alleged `manual_filter` blocker was already fixed in the changed
CRUD code and the verified run passes without the allowance. The duplicate
permission reviews describe the same fixed issue.

The proposed directory-only checksum memo was not added: it would weaken the
on-disk replacement check. Hashing occurs at session initialization/reload, not
per query; no measured performance regression was supplied. The committed
replacement regression protects this intentional correctness trade-off.

Historical numeric run IDs above are GitHub Actions runs, not agent-session IDs.
Their retained logs are supporting observations, not permanent reproducibility
assets. The committed regressions, named commands, and final exact-head CI links
in PR #435 are the repeatable evidence. No claim of independent slash-command
review or completion of live read/cache safety is made.
'''
    review.write_text(text)
    todo = Path('meta/todos/implement-embedding-space-migration.md')
    text = todo.read_text()
    heading = '## Remaining boundary in this same todo'
    addition = f'''## PR #435 review follow-up: 7 September 2026

Fixed the pinned-cache nested Tokio runtime panic with synchronous cached-file
verification and an isolated download runtime for synchronous calls from Tokio.
Added fourteen hermetic artifact regressions for cached/cold runtime contexts,
checksum recovery/rejection, replaced-file re-verification, and sidecar identity.
Normal CI now grants read-only contents access. [Review TDD and engineering
gates]({url}) records RED before the fix and the ensuing focused/full gates.
This run verifies a patched worktree; PR #435 separately records final exact-head
CI after the temporary runner is removed. Existing read/cache work below remains
in progress; this follow-up adds no todo or new public runtime surface.

'''
    assert heading in text
    todo.write_text(text.replace(heading, addition + heading))
else:
    raise SystemExit('expected tests, fix, or evidence')
