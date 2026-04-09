use std::sync::Arc;

use ragloom::doc::DocumentLoader;
use ragloom::pipeline::runtime::{
    AckingExecutor, AsyncRuntime, PipelineExecutor, Runtime, run_worker,
};
use ragloom::sink::VectorPoint;
use ragloom::source::{FileVersionDiscovered, Source};

#[derive(Debug, Default)]
struct FakeSource {
    pending: Vec<FileVersionDiscovered>,
}

impl FakeSource {
    fn push(&mut self, canonical_path: &str, file_version_id: [u8; 32]) {
        self.pending.push(FileVersionDiscovered {
            fingerprint: ragloom::ids::FileFingerprint {
                canonical_path: canonical_path.to_string(),
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

#[derive(Debug, Clone, Default)]
struct RecordingEmbeddingProvider {
    inputs: Arc<tokio::sync::Mutex<Vec<Vec<String>>>>,
}

#[async_trait::async_trait]
impl ragloom::embed::EmbeddingProvider for RecordingEmbeddingProvider {
    async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, ragloom::RagloomError> {
        self.inputs.lock().await.push(inputs.to_vec());
        Ok(inputs.iter().map(|_| vec![1.0_f32, 2.0_f32]).collect())
    }
}

#[derive(Debug, Clone, Default)]
struct RecordingSink {
    points: Arc<tokio::sync::Mutex<Vec<VectorPoint>>>,
}

#[async_trait::async_trait]
impl ragloom::sink::Sink for RecordingSink {
    async fn upsert_points(&self, points: Vec<VectorPoint>) -> Result<(), ragloom::RagloomError> {
        self.points.lock().await.extend(points);
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
struct StubLoader {
    calls: Arc<tokio::sync::Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl DocumentLoader for StubLoader {
    async fn load_utf8(&self, path: &str) -> Result<String, ragloom::RagloomError> {
        self.calls.lock().await.push(path.to_string());
        Ok("hello\nfrom\nloader".to_string())
    }
}

#[tokio::test]
async fn pipeline_embeds_text_loaded_from_fingerprint_v2() {
    let embedding = RecordingEmbeddingProvider::default();
    let sink = RecordingSink::default();
    let loader = StubLoader::default();

    let wal = Arc::new(tokio::sync::Mutex::new(
        ragloom::state::wal::InMemoryWal::new(),
    ));

    let mut source = FakeSource::default();
    source.push("/x/a.txt", [77u8; 32]);

    let runtime = Runtime::with_shared_wal(source, Arc::clone(&wal));
    let (rx, shutdown) = AsyncRuntime::new(runtime, 1).start();

    let executor = AckingExecutor {
        inner: PipelineExecutor::new(
            Arc::new(embedding.clone()),
            Arc::new(sink.clone()),
            Arc::new(loader.clone()),
        ),
        wal: Arc::clone(&wal),
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

    let calls = loader.calls.lock().await.clone();
    assert_eq!(calls, vec!["/x/a.txt".to_string()]);

    // RED: until executor loads text via DocumentLoader, embed() cannot run.
    assert!(
        !embedding.inputs.lock().await.is_empty(),
        "expected embed() to be called"
    );

    let embedded_batches = embedding.inputs.lock().await.clone();
    assert!(!embedded_batches.is_empty(), "expected embed() calls");

    let points = sink.points.lock().await.clone();
    assert!(!points.is_empty(), "expected sink upserts");

    shutdown.shutdown();
}
