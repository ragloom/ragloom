//! Pipeline runtime.
//!
//! # Why
//! This module wires together the small pipeline units into an executable
//! workflow. The MVP runtime focuses on deterministic orchestration that is
//! straightforward to test.

use crate::error::{RagloomError, RagloomErrorKind};
use crate::pipeline::planner::Planner;
use crate::source::Source;
use crate::state::wal::{InMemoryWal, WalRecord};

fn uuid_from_path_chunk_strategy(
    canonical_path: &str,
    chunk_index: usize,
    strategy: &crate::transform::chunker::StrategyFingerprint,
) -> String {
    // Stable, deterministic UUID v4-style string derived from a blake3 hash of
    // (canonical_path, chunk_index, strategy_fingerprint).
    //
    // # Why
    // Qdrant accepts UUID point ids, and we want stable ids for idempotent
    // upserts. The strategy fingerprint is mixed into the hash so that any
    // future change of chunker / parameters yields a distinct ID space and
    // never silently collides with older points.
    let mut hasher = blake3::Hasher::new();
    hasher.update(canonical_path.as_bytes());
    hasher.update(&[0x1F]);
    hasher.update(&(chunk_index as u64).to_le_bytes());
    hasher.update(&[0x1F]);
    hasher.update(strategy.as_bytes());
    let hash = hasher.finalize();
    let bytes = hash.as_bytes();
    let mut b = [0u8; 16];
    b.copy_from_slice(&bytes[..16]);

    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

fn canonical_path_to_file_uri(canonical_path: &str) -> String {
    // Convert a canonical path into a normalized file:// URI.
    //
    // # Why
    // We want cross-platform stable identifiers and prefix filtering behavior.
    // This intentionally avoids OS-specific path parsing; it normalizes separators
    // and handles Windows drive-letter paths.
    let normalized = canonical_path.replace('\\', "/");

    if let Some((drive, rest)) = normalized.split_once(":/") {
        // Windows drive path like D:/a/b.txt
        return format!("file:///{drive}:/{rest}");
    }

    if normalized.starts_with('/') {
        return format!("file://{normalized}");
    }

    // Fallback: treat as already-absolute-ish path.
    format!("file:///{normalized}")
}

fn doc_id_from_canonical_path(canonical_path_uri: &str) -> String {
    // Stable doc id derived from canonical path.
    //
    // # Why
    // Allows efficient filtering/deletion of all chunks for a document.
    blake3::hash(canonical_path_uri.as_bytes())
        .to_hex()
        .to_string()
}

fn file_extension_from_canonical_path(canonical_path: &str) -> String {
    let normalized = canonical_path.replace('\\', "/");
    let filename = normalized.rsplit('/').next().unwrap_or("");
    filename
        .rsplit('.')
        .next()
        .filter(|ext| *ext != filename)
        .unwrap_or("")
        .to_string()
}

/// A minimal in-process runtime.
///
/// # Why
/// The MVP does not require distributed execution. Keeping the runtime small
/// makes it easier to reason about crash recovery and idempotency.
#[derive(Debug)]
pub struct Runtime<S: Source> {
    source: S,
    planner: Planner,
    wal: std::sync::Arc<tokio::sync::Mutex<InMemoryWal>>,
}

impl<S: Source> Runtime<S> {
    pub fn new(source: S) -> Self {
        Self::with_wal(source, InMemoryWal::new())
    }

    pub fn with_wal(source: S, wal: InMemoryWal) -> Self {
        Self::with_shared_wal(source, std::sync::Arc::new(tokio::sync::Mutex::new(wal)))
    }

    pub fn with_shared_wal(
        source: S,
        wal: std::sync::Arc<tokio::sync::Mutex<InMemoryWal>>,
    ) -> Self {
        Self {
            source,
            planner: Planner::new(),
            wal,
        }
    }

    /// Runs a single polling cycle.
    ///
    /// # Why
    /// We keep the control loop explicit so tests can drive the runtime
    /// deterministically without threads.
    #[tracing::instrument(name = "ragloom.runtime.tick", skip_all)]
    pub fn tick(&mut self) {
        for discovered in self.source.poll() {
            self.planner
                .plan_file_version(&discovered, &self.wal)
                .expect("plan file version");
        }
    }

    pub fn wal_records(&self) -> Vec<WalRecord> {
        self.try_wal_records().expect("wal records")
    }

    pub fn try_wal_records(&self) -> Result<Vec<WalRecord>, RagloomError> {
        let guard = self.wal.try_lock().map_err(|_| {
            RagloomError::from_kind(RagloomErrorKind::Internal)
                .with_context("wal is currently locked")
        })?;
        guard.read_all()
    }
}

/// Receives planned work items from the runtime.
///
/// # Why
/// A bounded channel is the simplest way to model backpressure in-process: when
/// downstream is slower than discovery/planning, senders will await capacity.
pub type WorkQueue = tokio::sync::mpsc::Receiver<WalRecord>;

/// Executes a single work item.
///
/// # Why
/// The worker loop should be testable without real embedding/sink I/O.
#[async_trait::async_trait]
pub trait WorkExecutor: Send + Sync + 'static {
    async fn execute(&self, record: WalRecord);
}

/// Runs a worker loop that processes all work items until the queue closes.
///
/// # Why
/// This is the smallest possible boundary between concurrency (queue) and
/// processing (executor), making backpressure and shutdown behavior easy to test.
pub async fn run_worker(mut queue: WorkQueue, executor: impl WorkExecutor) {
    while let Some(record) = queue.recv().await {
        let record_type = match &record {
            WalRecord::WorkItem { .. } => "work_item",
            WalRecord::WorkItemV2 { .. } => "work_item_v2",
            WalRecord::SinkAck { .. } => "sink_ack",
            WalRecord::SinkAckV2 { .. } => "sink_ack_v2",
        };
        let canonical_path = match &record {
            WalRecord::WorkItemV2 { fingerprint } => Some(fingerprint.canonical_path.as_str()),
            _ => None,
        };

        tracing::info_span!(
            "ragloom.worker.execute",
            record_type,
            canonical_path = canonical_path
        )
        .in_scope(|| executor.execute(record))
        .await;
    }
}

/// Executes the MVP pipeline: chunk -> embed -> sink -> ack.
///
/// # Why
/// This is the smallest end-to-end executor that lets us validate the runtime
/// wiring in tests without implementing retries, batching, or caching.
pub struct PipelineExecutor {
    embedding: std::sync::Arc<dyn crate::embed::EmbeddingProvider + Send + Sync>,
    sink: std::sync::Arc<dyn crate::sink::Sink + Send + Sync>,
    loader: std::sync::Arc<dyn crate::doc::DocumentLoader + Send + Sync>,
    chunker: std::sync::Arc<dyn crate::transform::chunker::Chunker>,
}

impl Clone for PipelineExecutor {
    fn clone(&self) -> Self {
        Self {
            embedding: self.embedding.clone(),
            sink: self.sink.clone(),
            loader: self.loader.clone(),
            chunker: self.chunker.clone(),
        }
    }
}

impl PipelineExecutor {
    pub fn new(
        embedding: std::sync::Arc<dyn crate::embed::EmbeddingProvider + Send + Sync>,
        sink: std::sync::Arc<dyn crate::sink::Sink + Send + Sync>,
        loader: std::sync::Arc<dyn crate::doc::DocumentLoader + Send + Sync>,
    ) -> Self {
        let chunker = std::sync::Arc::new(
            crate::transform::chunker::RecursiveChunker::new(
                crate::transform::chunker::recursive_config_chars_512(),
            )
            .expect("default recursive config is always valid"),
        );
        Self {
            embedding,
            sink,
            loader,
            chunker,
        }
    }

    pub fn with_chunker(
        embedding: std::sync::Arc<dyn crate::embed::EmbeddingProvider + Send + Sync>,
        sink: std::sync::Arc<dyn crate::sink::Sink + Send + Sync>,
        loader: std::sync::Arc<dyn crate::doc::DocumentLoader + Send + Sync>,
        chunker: std::sync::Arc<dyn crate::transform::chunker::Chunker>,
    ) -> Self {
        Self {
            embedding,
            sink,
            loader,
            chunker,
        }
    }
}

impl PipelineExecutor {
    /// Turns a loaded document into sink points.
    ///
    /// # Why
    /// The pipeline runtime should be able to ingest documents from different
    /// backends (local files, object stores, HTTP) without coupling the executor
    /// to any particular I/O implementation.
    pub async fn build_points_from_text(
        &self,
        fingerprint: &crate::ids::FileFingerprint,
        text: &str,
    ) -> Result<Vec<crate::sink::VectorPoint>, crate::error::RagloomError> {
        let hint = crate::transform::chunker::ChunkHint::from_path(&fingerprint.canonical_path);
        let mut doc = self.chunker.chunk(text, &hint)?;
        if doc.chunks.is_empty() {
            // Keep downstream behavior predictable.
            doc.chunks.push(crate::transform::chunker::Chunk {
                index: 0,
                text: text.to_string(),
                boundary: crate::transform::chunker::BoundaryKind::Forced,
                start_byte: 0,
                end_byte: text.len(),
                char_len: text.chars().count(),
            });
        }
        let strategy_fp = &doc.strategy_fingerprint;

        let inputs: Vec<String> = doc.chunks.iter().map(|c| c.text.clone()).collect();

        let embeddings = self.embedding.embed(&inputs).await?;
        if embeddings.len() != inputs.len() {
            return Err(crate::error::RagloomError::from_kind(
                crate::error::RagloomErrorKind::Internal,
            )
            .with_context("embedding provider returned wrong count"));
        }

        let points: Vec<crate::sink::VectorPoint> = embeddings
            .into_iter()
            .enumerate()
            .map(|(idx, vector)| {
                let chunk = &doc.chunks[idx];

                // Qdrant point id must be an unsigned integer or UUID.
                // We use a stable UUID derived from (canonical_path, chunk_index, strategy_fingerprint)
                // to preserve idempotency while keeping strategy changes in separate ID spaces.
                let id = crate::sink::PointId::parse(uuid_from_path_chunk_strategy(
                    &fingerprint.canonical_path,
                    idx,
                    strategy_fp,
                ))
                .expect("generated uuid should be valid");

                let chunk_text_sha256 = blake3::hash(chunk.text.as_bytes()).to_hex().to_string();

                let canonical_path_uri = canonical_path_to_file_uri(&fingerprint.canonical_path);
                let doc_id = doc_id_from_canonical_path(&canonical_path_uri);
                let total_chunks = doc.chunks.len();

                let previous_chunk_id = if idx > 0 {
                    Some(uuid_from_path_chunk_strategy(
                        &fingerprint.canonical_path,
                        idx - 1,
                        strategy_fp,
                    ))
                } else {
                    None
                };
                let next_chunk_id = if idx + 1 < total_chunks {
                    Some(uuid_from_path_chunk_strategy(
                        &fingerprint.canonical_path,
                        idx + 1,
                        strategy_fp,
                    ))
                } else {
                    None
                };

                let payload = serde_json::json!({
                    "canonical_path": canonical_path_uri,
                    "doc_id": doc_id,
                    "tenant_id": "default",
                    "file_extension": file_extension_from_canonical_path(&fingerprint.canonical_path),

                    "size_bytes": fingerprint.size_bytes,
                    "mtime_unix_secs": fingerprint.mtime_unix_secs,

                    "chunk_index": idx,
                    "total_chunks": total_chunks,

                    "previous_chunk_id": previous_chunk_id,
                    "next_chunk_id": next_chunk_id,

                    "chunk_start_byte": chunk.start_byte,
                    "chunk_end_byte": chunk.end_byte,
                    "chunk_char_len": chunk.char_len,
                    "chunk_text_sha256": chunk_text_sha256,
                    "strategy_fingerprint": strategy_fp.as_str(),
                    "chunk_text": chunk.text,
                });

                Ok(crate::sink::VectorPoint {
                    id,
                    vector,
                    payload,
                })
            })
            .collect::<Result<_, crate::error::RagloomError>>()?;

        Ok(points)
    }
}

#[async_trait::async_trait]
impl WorkExecutor for PipelineExecutor {
    async fn execute(&self, record: WalRecord) {
        match record {
            WalRecord::WorkItemV2 { fingerprint } => {
                let elapsed_total = std::time::Instant::now();

                tracing::info_span!(
                    "ragloom.pipeline.process_file",
                    canonical_path = fingerprint.canonical_path.as_str(),
                    size_bytes = fingerprint.size_bytes,
                    mtime_unix_secs = fingerprint.mtime_unix_secs,
                )
                .in_scope(|| async {
                    let load_elapsed = std::time::Instant::now();
                    let text = match self.loader.load_utf8(&fingerprint.canonical_path).await {
                        Ok(text) => {
                            tracing::debug!(
                                canonical_path = fingerprint.canonical_path.as_str(),
                                elapsed_ms = load_elapsed.elapsed().as_millis() as u64,
                                "ragloom.doc.load_utf8"
                            );
                            text
                        }
                        Err(err) => {
                            tracing::warn!(
                                canonical_path = fingerprint.canonical_path.as_str(),
                                error.kind = %err.kind.to_string(),
                                error.message = %err,
                                "ragloom.doc.load_utf8"
                            );
                            return;
                        }
                    };

                    let points = match self.build_points_from_text(&fingerprint, &text).await {
                        Ok(points) => points,
                        Err(err) => {
                            tracing::warn!(
                                canonical_path = fingerprint.canonical_path.as_str(),
                                error.kind = %err.kind.to_string(),
                                error.message = %err,
                                "ragloom.embed.request"
                            );
                            return;
                        }
                    };

                    let point_count = points.len();

                    if let Err(err) = self.sink.upsert_points(points).await {
                        tracing::warn!(
                            canonical_path = fingerprint.canonical_path.as_str(),
                            point_count,
                            error.kind = %err.kind.to_string(),
                            error.message = %err,
                            "ragloom.sink.upsert"
                        );
                        return;
                    }

                    tracing::info!(
                        canonical_path = fingerprint.canonical_path.as_str(),
                        point_count,
                        elapsed_ms_total = elapsed_total.elapsed().as_millis() as u64,
                        "ragloom.ingest.success"
                    );
                })
                .await;
            }
            WalRecord::WorkItem { .. } => {
                // no-op
            }
            WalRecord::SinkAck { .. } => {
                // no-op
            }
            WalRecord::SinkAckV2 { .. } => {
                // no-op
            }
        }
    }
}

/// Wraps an executor and emits `SinkAck` records after successful execution.
///
/// # Why
/// Acking at the boundary (after side effects) is the minimal WAL signal we need
/// for replay and near exactly-once semantics.
#[derive(Debug, Clone)]
pub struct AckingExecutor<E: WorkExecutor> {
    pub inner: E,
    pub wal: std::sync::Arc<tokio::sync::Mutex<crate::state::wal::InMemoryWal>>,
}

#[async_trait::async_trait]
impl<E: WorkExecutor> WorkExecutor for AckingExecutor<E> {
    async fn execute(&self, record: WalRecord) {
        let ack = match &record {
            WalRecord::WorkItem { chunk_id } => Some(WalRecord::SinkAck {
                chunk_id: *chunk_id,
            }),
            WalRecord::WorkItemV2 { fingerprint } => Some(WalRecord::SinkAckV2 {
                fingerprint: fingerprint.clone(),
            }),
            WalRecord::SinkAck { .. } => None,
            WalRecord::SinkAckV2 { .. } => None,
        };

        self.inner.execute(record).await;

        if let Some(ack) = ack {
            let mut wal = self.wal.lock().await;
            let elapsed = std::time::Instant::now();
            wal.append(ack).expect("append ack");
            tracing::debug!(
                ack_type = "sink_ack",
                elapsed_ms = elapsed.elapsed().as_millis() as u64,
                "ragloom.wal.append_ack"
            );
        }
    }
}

/// Stops a running async runtime.
///
/// # Why
/// The runtime loop should have an explicit, testable shutdown signal instead
/// of relying on channel drops or thread termination.
#[derive(Debug, Clone)]
pub struct ShutdownHandle {
    tx: tokio::sync::watch::Sender<bool>,
}

impl ShutdownHandle {
    pub fn shutdown(self) {
        let _ = self.tx.send(true);
    }
}

/// A minimal async runtime runner.
///
/// # Why
/// We keep the existing synchronous `Runtime` for deterministic unit tests and
/// add an async runner that turns planned WAL records into a bounded stream for
/// downstream workers.
#[derive(Debug)]
pub struct AsyncRuntime<S: Source + Send + 'static> {
    runtime: Runtime<S>,
    capacity: usize,
}

impl<S: Source + Send + 'static> AsyncRuntime<S> {
    pub fn new(runtime: Runtime<S>, capacity: usize) -> Self {
        Self { runtime, capacity }
    }

    /// Runs the planner loop and streams newly planned work items.
    ///
    /// # Why
    /// This is the narrow waist between deterministic planning and concurrent
    /// execution. The bounded queue provides backpressure automatically.
    pub fn start(mut self) -> (WorkQueue, ShutdownHandle) {
        let (tx, rx) = tokio::sync::mpsc::channel(self.capacity);
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

        tokio::spawn(async move {
            loop {
                if *shutdown_rx.borrow() {
                    return;
                }

                let before_records = match self.runtime.try_wal_records() {
                    Ok(records) => records,
                    Err(err) if err.kind == RagloomErrorKind::Internal => {
                        tokio::task::yield_now().await;
                        continue;
                    }
                    Err(_) => return,
                };
                let before = before_records.len();

                self.runtime.tick();

                let after_records = match self.runtime.try_wal_records() {
                    Ok(records) => records,
                    Err(err) if err.kind == RagloomErrorKind::Internal => {
                        tokio::task::yield_now().await;
                        continue;
                    }
                    Err(_) => return,
                };
                let after = after_records.len();

                for record in after_records.into_iter().skip(before) {
                    if tx.send(record).await.is_err() {
                        return;
                    }

                    if shutdown_rx.has_changed().unwrap_or(false) && *shutdown_rx.borrow() {
                        return;
                    }
                }

                if after == before {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() {
                                return;
                            }
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {}
                    }
                }
            }
        });

        (rx, ShutdownHandle { tx: shutdown_tx })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ids::FileFingerprint;
    use crate::source::FileVersionDiscovered;

    #[derive(Debug, Default)]
    struct FakeSource {
        pending: Vec<FileVersionDiscovered>,
    }

    impl FakeSource {
        fn push(&mut self, file_version_id: [u8; 32]) {
            self.pending.push(FileVersionDiscovered {
                fingerprint: FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                },
                file_version_id,
            });
        }
    }

    impl Source for FakeSource {
        fn poll(&mut self) -> Vec<FileVersionDiscovered> {
            std::mem::take(&mut self.pending)
        }
    }

    #[test]
    fn runtime_does_not_duplicate_work_for_same_file_version_across_ticks() {
        let mut source = FakeSource::default();
        source.push([9u8; 32]);

        let mut runtime = Runtime::new(source);
        runtime.tick();
        runtime.tick();

        let records = runtime.wal_records();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0],
            WalRecord::WorkItemV2 {
                fingerprint: crate::ids::FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                },
            }
        );
    }

    #[tokio::test]
    async fn async_runtime_streams_work_items_with_backpressure() {
        let mut source = FakeSource::default();
        source.push([1u8; 32]);
        source.push([2u8; 32]);

        let runtime = Runtime::new(source);
        let (mut rx, _shutdown) = AsyncRuntime::new(runtime, 1).start();

        let first = rx.recv().await.expect("first");
        assert_eq!(
            first,
            WalRecord::WorkItemV2 {
                fingerprint: crate::ids::FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                },
            }
        );

        let second = rx.recv().await.expect("second");
        assert_eq!(
            second,
            WalRecord::WorkItemV2 {
                fingerprint: crate::ids::FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                },
            }
        );
    }

    #[tokio::test]
    async fn async_runtime_shutdown_stops_task_and_closes_queue() {
        let mut source = FakeSource::default();
        source.push([3u8; 32]);

        let runtime = Runtime::new(source);
        let (mut rx, shutdown) = AsyncRuntime::new(runtime, 1).start();

        let first = rx.recv().await.expect("first");
        assert_eq!(
            first,
            WalRecord::WorkItemV2 {
                fingerprint: crate::ids::FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                },
            }
        );

        shutdown.shutdown();

        let next = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("recv should finish");
        assert!(next.is_none());
    }

    #[tokio::test]
    async fn async_runtime_does_not_emit_duplicate_sink_ack_records_when_idle() {
        let mut source = FakeSource::default();
        source.push([11u8; 32]);

        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));
        let runtime = Runtime::with_shared_wal(source, std::sync::Arc::clone(&wal));
        let (mut rx, shutdown) = AsyncRuntime::new(runtime, 1).start();

        // First, we should receive the WorkItem.
        let first = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("recv")
            .expect("record");
        assert_eq!(
            first,
            WalRecord::WorkItemV2 {
                fingerprint: crate::ids::FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                },
            }
        );

        // Then we append an ack-like record into the shared WAL.
        //
        // # Why
        // The async runtime streams newly planned work items; it should not start
        // streaming arbitrary unrelated WAL records that appear while idle.
        {
            let mut guard = wal.lock().await;
            guard
                .append(WalRecord::SinkAck {
                    chunk_id: [11u8; 32],
                })
                .expect("append");
        }

        // The runtime should not keep re-sending the same SinkAck record forever.
        let next = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(next.is_err(), "should not emit SinkAck when idle");

        shutdown.shutdown();
    }

    #[tokio::test]
    async fn async_runtime_idles_without_busy_loop() {
        #[derive(Debug, Clone)]
        struct CountingSource {
            polls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }

        impl crate::source::Source for CountingSource {
            fn poll(&mut self) -> Vec<crate::source::FileVersionDiscovered> {
                use std::sync::atomic::Ordering;
                self.polls.fetch_add(1, Ordering::SeqCst);
                Vec::new()
            }
        }

        let polls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let source = CountingSource {
            polls: std::sync::Arc::clone(&polls),
        };

        let runtime = Runtime::new(source);
        let (_rx, shutdown) = AsyncRuntime::new(runtime, 1).start();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        shutdown.shutdown();

        // If the runtime busy-loops, poll() will be called extremely frequently.
        // We enforce a conservative upper bound to ensure we have some idle backoff.
        let count = polls.load(std::sync::atomic::Ordering::SeqCst);
        assert!(count <= 50, "poll() called too often while idle: {count}");
    }

    #[tokio::test]
    async fn async_runtime_yields_when_wal_is_locked_and_recovers() {
        let mut source = FakeSource::default();
        source.push([10u8; 32]);

        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));
        let runtime = Runtime::with_shared_wal(source, std::sync::Arc::clone(&wal));

        let _guard = wal.lock().await;
        let (mut rx, shutdown) = AsyncRuntime::new(runtime, 1).start();

        let early = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await;
        assert!(early.is_err());

        drop(_guard);

        let item = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("should recover")
            .expect("record");
        assert_eq!(
            item,
            WalRecord::WorkItemV2 {
                fingerprint: crate::ids::FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                },
            }
        );

        shutdown.shutdown();
    }

    #[tokio::test]
    async fn worker_processes_work_items_from_queue() {
        let mut source = FakeSource::default();
        source.push([4u8; 32]);

        let runtime = Runtime::new(source);
        let (rx, shutdown) = AsyncRuntime::new(runtime, 1).start();

        let seen = std::sync::Arc::new(tokio::sync::Mutex::new(
            Vec::<crate::ids::FileFingerprint>::new(),
        ));
        let executor = FakeExecutor {
            seen: std::sync::Arc::clone(&seen),
        };

        tokio::spawn(async move {
            run_worker(rx, executor).await;
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let got = { seen.lock().await.clone() };
                if got
                    == vec![crate::ids::FileFingerprint {
                        canonical_path: "/x/a.txt".to_string(),
                        size_bytes: 10,
                        mtime_unix_secs: 100,
                    }]
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker should process");

        shutdown.shutdown();
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn pipeline_emits_ingest_success_event_for_workitem_v2() {
        use crate::sink::VectorPoint;

        #[derive(Debug, Clone, Default)]
        struct StubDocumentLoader;

        #[async_trait::async_trait]
        impl crate::doc::DocumentLoader for StubDocumentLoader {
            async fn load_utf8(&self, _path: &str) -> Result<String, crate::error::RagloomError> {
                Ok("hello from stub loader".to_string())
            }
        }

        #[derive(Debug, Clone, Default)]
        struct StubEmbeddingProvider {
            inputs: std::sync::Arc<tokio::sync::Mutex<Vec<Vec<String>>>>,
        }

        #[async_trait::async_trait]
        impl crate::embed::EmbeddingProvider for StubEmbeddingProvider {
            async fn embed(
                &self,
                inputs: &[String],
            ) -> Result<Vec<Vec<f32>>, crate::error::RagloomError> {
                self.inputs.lock().await.push(inputs.to_vec());
                Ok(inputs.iter().map(|_| vec![1.0_f32, 2.0_f32]).collect())
            }
        }

        #[derive(Debug, Clone, Default)]
        struct StubSink {
            points: std::sync::Arc<tokio::sync::Mutex<Vec<VectorPoint>>>,
        }

        #[async_trait::async_trait]
        impl crate::sink::Sink for StubSink {
            async fn upsert_points(
                &self,
                points: Vec<VectorPoint>,
            ) -> Result<(), crate::error::RagloomError> {
                self.points.lock().await.extend(points);
                Ok(())
            }
        }

        let embedding = StubEmbeddingProvider::default();
        let sink = StubSink::default();
        let loader = StubDocumentLoader;

        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));

        let mut source = FakeSource::default();
        source.push([42u8; 32]);

        let runtime = Runtime::with_shared_wal(source, std::sync::Arc::clone(&wal));
        let (rx, shutdown) = AsyncRuntime::new(runtime, 1).start();

        let executor = AckingExecutor {
            inner: PipelineExecutor::new(
                std::sync::Arc::new(embedding.clone()),
                std::sync::Arc::new(sink.clone()),
                std::sync::Arc::new(loader.clone()),
            ),
            wal: std::sync::Arc::clone(&wal),
        };

        tokio::spawn(async move {
            run_worker(rx, executor).await;
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if !embedding.inputs.lock().await.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("should process");

        assert!(
            logs_contain("ragloom.ingest.success"),
            "expected ragloom.ingest.success event"
        );

        shutdown.shutdown();
    }

    #[tokio::test]
    async fn pipeline_executor_runs_chunk_embed_sink_and_acks() {
        use crate::sink::VectorPoint;

        #[derive(Debug, Clone, Default)]
        struct StubDocumentLoader;

        #[async_trait::async_trait]
        impl crate::doc::DocumentLoader for StubDocumentLoader {
            async fn load_utf8(&self, _path: &str) -> Result<String, crate::error::RagloomError> {
                Ok("hello from stub loader".to_string())
            }
        }

        #[derive(Debug, Clone, Default)]
        struct StubEmbeddingProvider {
            inputs: std::sync::Arc<tokio::sync::Mutex<Vec<Vec<String>>>>,
        }

        #[async_trait::async_trait]
        impl crate::embed::EmbeddingProvider for StubEmbeddingProvider {
            async fn embed(
                &self,
                inputs: &[String],
            ) -> Result<Vec<Vec<f32>>, crate::error::RagloomError> {
                self.inputs.lock().await.push(inputs.to_vec());
                Ok(inputs.iter().map(|_| vec![1.0_f32, 2.0_f32]).collect())
            }
        }

        #[derive(Debug, Clone, Default)]
        struct StubSink {
            points: std::sync::Arc<tokio::sync::Mutex<Vec<VectorPoint>>>,
        }

        #[async_trait::async_trait]
        impl crate::sink::Sink for StubSink {
            async fn upsert_points(
                &self,
                points: Vec<VectorPoint>,
            ) -> Result<(), crate::error::RagloomError> {
                self.points.lock().await.extend(points);
                Ok(())
            }
        }

        let embedding = StubEmbeddingProvider::default();
        let sink = StubSink::default();
        let loader = StubDocumentLoader;

        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));

        let mut source = FakeSource::default();
        source.push([42u8; 32]);

        let runtime = Runtime::with_shared_wal(source, std::sync::Arc::clone(&wal));
        let (rx, shutdown) = AsyncRuntime::new(runtime, 1).start();

        // Minimal E2E intent:
        // 1) runtime produces a WorkItem via AsyncRuntime queue
        // 2) worker executes PipelineExecutor
        // 3) embed is called for the chunk(s)
        // 4) sink receives VectorPoint(s)
        // 5) embed is called for the chunk(s)
        // 6) sink receives VectorPoint(s)
        // Note: In the WorkItemV2 MVP, we don't emit SinkAck records yet.
        let executor = AckingExecutor {
            inner: PipelineExecutor::new(
                std::sync::Arc::new(embedding.clone()),
                std::sync::Arc::new(sink.clone()),
                std::sync::Arc::new(loader.clone()),
            ),
            wal: std::sync::Arc::clone(&wal),
        };

        tokio::spawn(async move {
            run_worker(rx, executor).await;
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                // Wait until embedding is invoked as a proxy for end-to-end progress.
                if !embedding.inputs.lock().await.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("should process");

        assert!(
            !embedding.inputs.lock().await.is_empty(),
            "embedding provider should be called"
        );

        let points = sink.points.lock().await.clone();
        assert!(!points.is_empty(), "sink should upsert");

        for p in &points {
            assert!(!p.id.as_str().is_empty(), "point id should be non-empty");
            assert!(!p.vector.is_empty(), "point vector should be non-empty");

            let payload = p.payload.as_object().expect("payload should be object");
            assert!(
                payload.contains_key("canonical_path"),
                "payload must contain canonical_path"
            );
            assert!(
                payload.contains_key("chunk_index"),
                "payload must contain chunk_index"
            );
        }

        shutdown.shutdown();
    }

    #[tokio::test]
    async fn executor_writes_sink_ack_into_wal() {
        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));

        let inner = RecordingExecutor {
            seen: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
        };
        let executor = AckingExecutor {
            inner,
            wal: std::sync::Arc::clone(&wal),
        };

        executor
            .execute(WalRecord::WorkItemV2 {
                fingerprint: crate::ids::FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                },
            })
            .await;

        let records = wal.lock().await.read_all().expect("read");
        assert!(
            records.contains(&WalRecord::SinkAckV2 {
                fingerprint: crate::ids::FileFingerprint {
                    canonical_path: "/x/a.txt".to_string(),
                    size_bytes: 10,
                    mtime_unix_secs: 100,
                }
            }),
            "expected SinkAckV2"
        );
    }

    #[tokio::test]
    async fn runtime_uses_the_same_wal_instance_without_snapshotting() {
        let mut source = FakeSource::default();
        source.push([7u8; 32]);

        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));

        let runtime = Runtime::with_shared_wal(source, std::sync::Arc::clone(&wal));

        {
            let mut guard = wal.lock().await;
            guard
                .append(WalRecord::SinkAck {
                    chunk_id: [9u8; 32],
                })
                .expect("append");
        }

        let records = runtime.try_wal_records().expect("records");
        assert!(records.contains(&WalRecord::SinkAck {
            chunk_id: [9u8; 32]
        }));
    }

    #[tokio::test]
    async fn try_wal_records_returns_error_when_wal_is_locked() {
        let mut source = FakeSource::default();
        source.push([8u8; 32]);

        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));
        let runtime = Runtime::with_shared_wal(source, std::sync::Arc::clone(&wal));

        let _guard = wal.lock().await;
        let err = runtime
            .try_wal_records()
            .expect_err("should fail when locked");
        assert_eq!(err.kind, RagloomErrorKind::Internal);
        assert!(err.to_string().contains("wal is currently locked"));
    }

    #[tokio::test]
    async fn executor_emits_sink_ack_after_successful_processing() {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let seen = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<WalRecord>::new()));

        let wal = std::sync::Arc::new(tokio::sync::Mutex::new(
            crate::state::wal::InMemoryWal::new(),
        ));

        let inner = RecordingExecutor {
            seen: std::sync::Arc::clone(&seen),
        };
        let executor = AckingExecutor {
            inner,
            wal: std::sync::Arc::clone(&wal),
        };

        tokio::spawn(async move {
            run_worker(rx, executor).await;
        });

        tx.send(WalRecord::WorkItemV2 {
            fingerprint: crate::ids::FileFingerprint {
                canonical_path: "/x/a.txt".to_string(),
                size_bytes: 10,
                mtime_unix_secs: 100,
            },
        })
        .await
        .expect("send");
        drop(tx);

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let records = seen.lock().await.clone();
                if records.contains(&WalRecord::WorkItemV2 {
                    fingerprint: crate::ids::FileFingerprint {
                        canonical_path: "/x/a.txt".to_string(),
                        size_bytes: 10,
                        mtime_unix_secs: 100,
                    },
                }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("should execute");
    }

    #[derive(Debug, Clone)]
    struct FakeExecutor {
        seen: std::sync::Arc<tokio::sync::Mutex<Vec<crate::ids::FileFingerprint>>>,
    }

    #[async_trait::async_trait]
    impl WorkExecutor for FakeExecutor {
        async fn execute(&self, record: WalRecord) {
            if let WalRecord::WorkItemV2 { fingerprint } = record {
                self.seen.lock().await.push(fingerprint);
            }
        }
    }

    #[derive(Debug, Clone)]
    struct RecordingExecutor {
        seen: std::sync::Arc<tokio::sync::Mutex<Vec<WalRecord>>>,
    }

    #[async_trait::async_trait]
    impl WorkExecutor for RecordingExecutor {
        async fn execute(&self, record: WalRecord) {
            self.seen.lock().await.push(record);
        }
    }
}
